use super::mobile_hdr::Transfer;
use crate::render::{prores_profile, ProresProfile};
use anyhow::{anyhow, ensure, Context, Result};
use std::{
    ffi::{c_void, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) const PROFILES: [&str; 7] = [
    "prores_422_proxy",
    "prores_422_lt",
    "prores_422",
    "prores_422_hq",
    "prores_4444",
    "prores_4444_alpha",
    "prores_4444_xq",
];

type Open = unsafe extern "C" fn(
    *const libc::c_char,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    *mut *mut c_void,
) -> i32;
type Close = unsafe extern "C" fn(*mut c_void);
type Frame = unsafe extern "C" fn(*mut c_void, *const u8) -> i32;
type Audio = unsafe extern "C" fn(*mut c_void, *const u8, i32) -> i32;
type Finish = unsafe extern "C" fn(*mut c_void) -> i32;

struct Library(*mut c_void);

impl Library {
    fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T> {
        let pointer = unsafe { libc::dlsym(self.0, name.as_ptr().cast()) };
        ensure!(
            !pointer.is_null(),
            "missing Android ProRes entry point {}",
            String::from_utf8_lossy(name)
        );
        // Every use supplies the C signature for the named exported function.
        Ok(unsafe { std::mem::transmute_copy(&pointer) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}

pub(super) struct Writer {
    _library: Library,
    encoder: *mut c_void,
    close: Close,
    frame: Frame,
    audio: Audio,
    finish: Finish,
    fps: u32,
    rate: u32,
    frame_bytes: usize,
    video_frames: usize,
    audio_bytes: usize,
}

enum EncodeMessage {
    Frame(Vec<u8>),
    Finish,
}

pub(super) struct AsyncWriter {
    sender: Option<mpsc::SyncSender<EncodeMessage>>,
    worker: Option<JoinHandle<Result<WorkerStats>>>,
    frame_bytes: usize,
    backpressure_waits: AtomicU64,
    backpressure_ns: AtomicU64,
}

struct WorkerStats {
    frames: u64,
    encode_seconds: f64,
    wall_seconds: f64,
}

const PRORES_QUEUE_DEPTH: usize = 2;

fn audio_end_bytes(video_frames: usize, rate: u32, fps: u32, pcm_bytes: usize) -> usize {
    (video_frames * rate as usize / fps.max(1) as usize)
        .saturating_mul(4)
        .min(pcm_bytes)
}

fn status(code: i32, operation: &str) -> Result<()> {
    ensure!(code >= 0, "{operation} failed (FFmpeg {code})");
    Ok(())
}

impl Writer {
    pub(super) fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        rate: u32,
        profile: ProresProfile,
        transfer: Option<Transfer>,
    ) -> Result<Self> {
        let pointer = unsafe {
            libc::dlopen(
                c"libphitkc_prores.so".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        let library = Library(
            std::ptr::NonNull::new(pointer)
                .context("Android ProRes encoder library is unavailable")?
                .as_ptr(),
        );
        let open: Open = library.symbol(b"phitkc_prores_open\0")?;
        let close = library.symbol(b"phitkc_prores_close\0")?;
        let frame = library.symbol(b"phitkc_prores_frame\0")?;
        let audio = library.symbol(b"phitkc_prores_audio\0")?;
        let finish = library.symbol(b"phitkc_prores_finish\0")?;
        ensure!(!path.exists(), "output already exists: {}", path.display());
        let path = CString::new(path.as_os_str().as_bytes())?;
        let mut encoder = null_mut();
        let color = match transfer {
            None => 0,
            Some(Transfer::Pq) => 1,
            Some(Transfer::Hlg) => 2,
        };
        status(
            unsafe {
                open(
                    path.as_ptr(),
                    width.try_into()?,
                    height.try_into()?,
                    fps.try_into()?,
                    rate.try_into()?,
                    i32::from(profile.number),
                    i32::from(profile.alpha),
                    color,
                    &mut encoder,
                )
            },
            "Opening Android CPU ProRes MOV",
        )?;
        ensure!(
            !encoder.is_null(),
            "Android ProRes encoder returned no handle"
        );
        Ok(Self {
            _library: library,
            encoder,
            close,
            frame,
            audio,
            finish,
            fps,
            rate,
            frame_bytes: width as usize * height as usize * if color == 0 { 4 } else { 8 },
            video_frames: 0,
            audio_bytes: 0,
        })
    }

    pub(super) fn frame(&mut self, rgba: &[u8], pcm: &[u8]) -> Result<()> {
        ensure!(
            rgba.len() == self.frame_bytes,
            "wrong ProRes input frame size"
        );
        status(
            unsafe { (self.frame)(self.encoder, rgba.as_ptr()) },
            "Encoding Android CPU ProRes frame",
        )?;
        self.video_frames += 1;
        let wanted = audio_end_bytes(self.video_frames, self.rate, self.fps, pcm.len());
        self.write_audio(pcm, wanted)
    }

    fn write_audio(&mut self, pcm: &[u8], end: usize) -> Result<()> {
        for chunk in pcm[self.audio_bytes..end].chunks(16_384) {
            status(
                unsafe { (self.audio)(self.encoder, chunk.as_ptr(), chunk.len() as i32) },
                "Writing Android ProRes MOV audio",
            )?;
        }
        self.audio_bytes = end;
        Ok(())
    }

    pub(super) fn finish(mut self, pcm: &[u8]) -> Result<()> {
        self.write_audio(pcm, pcm.len())?;
        status(
            unsafe { (self.finish)(self.encoder) },
            "Finalizing Android ProRes MOV",
        )
    }
}

// The FFmpeg handle is exclusively owned by the worker after construction.
unsafe impl Send for Writer {}

impl AsyncWriter {
    pub(super) fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        rate: u32,
        profile: ProresProfile,
        transfer: Option<Transfer>,
        pcm: Arc<[u8]>,
    ) -> Result<Self> {
        let writer = Writer::new(path, width, height, fps, rate, profile, transfer)?;
        let frame_bytes = writer.frame_bytes;
        let (sender, receiver) = mpsc::sync_channel(PRORES_QUEUE_DEPTH);
        let worker = thread::Builder::new()
            .name("phi-prores-encode".into())
            .spawn(move || {
                let mut writer = writer;
                let started = Instant::now();
                let mut encode_seconds = 0.0;
                let mut frames = 0;
                while let Ok(message) = receiver.recv() {
                    match message {
                        EncodeMessage::Frame(frame) => {
                            let encode_start = Instant::now();
                            writer.frame(&frame, &pcm)?;
                            encode_seconds += encode_start.elapsed().as_secs_f64();
                            frames += 1;
                        }
                        EncodeMessage::Finish => {
                            writer.finish(&pcm)?;
                            return Ok(WorkerStats {
                                frames,
                                encode_seconds,
                                wall_seconds: started.elapsed().as_secs_f64(),
                            });
                        }
                    }
                }
                Ok(WorkerStats {
                    frames,
                    encode_seconds,
                    wall_seconds: started.elapsed().as_secs_f64(),
                })
            })
            .context("starting Android ProRes encoder worker")?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            frame_bytes,
            backpressure_waits: AtomicU64::new(0),
            backpressure_ns: AtomicU64::new(0),
        })
    }

    pub(super) fn frame(&self, rgba: &[u8], cancel: Option<&AtomicBool>) -> Result<()> {
        ensure!(
            rgba.len() == self.frame_bytes,
            "wrong ProRes input frame size"
        );
        let sender = self
            .sender
            .as_ref()
            .context("ProRes encoder worker is closed")?;
        let mut message = EncodeMessage::Frame(rgba.to_vec());
        loop {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Err(anyhow!("render canceled"));
            }
            match sender.try_send(message) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Full(returned)) => {
                    message = returned;
                    let wait_start = Instant::now();
                    thread::sleep(Duration::from_millis(4));
                    self.backpressure_waits.fetch_add(1, Ordering::Relaxed);
                    self.backpressure_ns.fetch_add(
                        wait_start.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                        Ordering::Relaxed,
                    );
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(anyhow!("Android ProRes encoder worker failed"));
                }
            }
        }
    }

    pub(super) fn finish(mut self) -> Result<()> {
        if let Some(sender) = self.sender.take() {
            sender
                .send(EncodeMessage::Finish)
                .map_err(|_| anyhow!("Android ProRes encoder worker failed"))?;
        }
        let stats = self
            .worker
            .take()
            .context("ProRes encoder worker is missing")?
            .join()
            .map_err(|_| anyhow!("Android ProRes encoder worker panicked"))??;
        let queue_wait_seconds = self.backpressure_ns.load(Ordering::Relaxed) as f64 / 1e9;
        log::info!(
            "Android ProRes worker: {} frames, encode {:.2}s ({:.1} fps), worker wall {:.2}s ({:.1} fps); render queue backpressure {:.2}s over {} waits",
            stats.frames,
            stats.encode_seconds,
            stats.frames as f64 / stats.encode_seconds.max(1e-9),
            stats.wall_seconds,
            stats.frames as f64 / stats.wall_seconds.max(1e-9),
            queue_wait_seconds,
            self.backpressure_waits.load(Ordering::Relaxed),
        );
        Ok(())
    }
}

impl Drop for AsyncWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        unsafe { (self.close)(self.encoder) };
    }
}

#[cfg(test)]
mod tests {
    use super::audio_end_bytes;

    #[test]
    fn audio_tracks_video_time_without_leading_or_trailing_drift() {
        let rate = 48_000;
        let fps = 30;
        let pcm_bytes = rate as usize * 4 * 4;
        assert_eq!(audio_end_bytes(1, rate, fps, pcm_bytes), 6_400);
        assert_eq!(audio_end_bytes(30, rate, fps, pcm_bytes), 192_000);
        assert_eq!(audio_end_bytes(60, rate, fps, pcm_bytes), 384_000);
        assert_eq!(audio_end_bytes(120, rate, fps, pcm_bytes), pcm_bytes);
    }

    #[test]
    fn audio_end_is_monotonic_and_never_exceeds_pcm() {
        let pcm_bytes = 51_200;
        let values: Vec<_> = (0..40)
            .map(|frame| audio_end_bytes(frame, 48_000, 30, pcm_bytes))
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(values.iter().all(|&end| end <= pcm_bytes));
        assert_eq!(audio_end_bytes(1, 0, 0, pcm_bytes), 0);
    }
}

pub(super) fn probe(name: &str, path: &Path, transfer: Option<Transfer>) -> Result<()> {
    let profile = prores_profile(name).ok_or_else(|| anyhow!("unknown ProRes profile"))?;
    let mut writer = Writer::new(path, 320, 240, 30, 48_000, profile, transfer)?;
    let pcm = [0; 6400];
    let rgba = match transfer {
        None => vec![128; 320 * 240 * 4],
        Some(_) => half::f16::from_f32(0.5)
            .to_bits()
            .to_le_bytes()
            .repeat(320 * 240 * 4),
    };
    writer.frame(&rgba, &pcm)?;
    writer.finish(&pcm)
}

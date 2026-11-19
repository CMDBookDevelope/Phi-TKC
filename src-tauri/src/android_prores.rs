use super::mobile_hdr::Transfer;
use crate::render::{prores_profile, ProresProfile};
use anyhow::{anyhow, ensure, Context, Result};
use std::{
    ffi::{c_void, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::null_mut,
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
        let wanted = (self.video_frames * self.rate as usize / self.fps as usize) * 4;
        self.write_audio(pcm, wanted.min(pcm.len()))
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

impl Drop for Writer {
    fn drop(&mut self) {
        unsafe { (self.close)(self.encoder) };
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

//! Transfer ownership of completed GPU readback slots to a blocking writer.
//! Slots return to the producer only after the entire frame was written.
use std::{
    io::{self, Write},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WriteStats {
    pub frames: u64,
    pub write_ns: u64,
    pub max_write_ns: u64,
}

pub(crate) struct FrameHandoff {
    jobs: Option<Sender<(u64, usize)>>,
    available: Receiver<usize>,
    worker: Option<JoinHandle<io::Result<WriteStats>>>,
    leased: Vec<bool>,
    next_frame: u64,
    finished: Option<WriteStats>,
}

impl FrameHandoff {
    /// Start a writer over stable, persistently mapped readback allocations.
    ///
    /// # Safety
    /// Each address must remain valid for `frame_bytes` bytes until this object
    /// is dropped. The producer may write a slot only after `acquire_slot`, and
    /// must finish its GPU fence/host visibility dependency before `submit`.
    /// No GPU or CPU writer may touch a submitted slot until it is acquired
    /// again. Drop closes the queue and joins the reader before returning.
    pub(crate) unsafe fn new_mapped<W: Write + Send + 'static>(
        addresses: Vec<usize>,
        frame_bytes: usize,
        mut output: W,
    ) -> io::Result<Self> {
        if addresses.is_empty() || frame_bytes == 0 || addresses.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty readback storage",
            ));
        }
        let count = addresses.len();
        let (jobs, incoming) = mpsc::channel::<(u64, usize)>();
        let (returned, available) = mpsc::channel();
        for slot in 0..count {
            returned.send(slot).unwrap();
        }
        let worker = thread::Builder::new()
            .name("ffmpeg-direct-writer".into())
            .spawn(move || {
                let mut stats = WriteStats::default();
                for (index, slot) in incoming {
                    if index != stats.frames {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "readback frame order changed",
                        ));
                    }
                    // The producer transferred this slot after waiting for its
                    // fence. The slot is unavailable to the producer until send.
                    let bytes = unsafe {
                        std::slice::from_raw_parts(addresses[slot] as *const u8, frame_bytes)
                    };
                    let start = Instant::now();
                    output.write_all(bytes)?;
                    let elapsed = start.elapsed().as_nanos() as u64;
                    stats.frames += 1;
                    stats.write_ns += elapsed;
                    stats.max_write_ns = stats.max_write_ns.max(elapsed);
                    if returned.send(slot).is_err() {
                        break;
                    }
                }
                let start = Instant::now();
                output.flush()?;
                stats.write_ns += start.elapsed().as_nanos() as u64;
                Ok(stats)
            })?;
        Ok(Self {
            jobs: Some(jobs),
            available,
            worker: Some(worker),
            leased: vec![false; count],
            next_frame: 0,
            finished: None,
        })
    }

    pub(crate) fn acquire_slot(&mut self) -> io::Result<usize> {
        let slot = match self.available.try_recv() {
            Ok(slot) => slot,
            Err(TryRecvError::Empty) => match self.available.recv() {
                Ok(slot) => slot,
                Err(_) => return Err(self.stopped_error()),
            },
            Err(TryRecvError::Disconnected) => return Err(self.stopped_error()),
        };
        assert!(!self.leased[slot], "readback slot acquired twice");
        self.leased[slot] = true;
        Ok(slot)
    }

    pub(crate) fn submit(&mut self, index: u64, slot: usize) -> io::Result<()> {
        if index != self.next_frame || self.leased.get(slot) != Some(&true) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid readback slot or frame order",
            ));
        }
        let Some(jobs) = self.jobs.as_ref() else {
            return Err(self.stopped_error());
        };
        if jobs.send((index, slot)).is_err() {
            return Err(self.stopped_error());
        }
        self.leased[slot] = false;
        self.next_frame += 1;
        Ok(())
    }

    fn stopped_error(&mut self) -> io::Error {
        self.join()
            .err()
            .unwrap_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "readback writer stopped"))
    }

    fn join(&mut self) -> io::Result<WriteStats> {
        // Never wait for a receiver which is itself waiting for our sender.
        self.jobs.take();
        if let Some(worker) = self.worker.take() {
            let result = worker
                .join()
                .map_err(|_| io::Error::other("readback writer thread panicked"))?;
            self.finished = Some(result?);
        }
        self.finished
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "readback writer failed"))
    }

    pub(crate) fn finish(&mut self) -> io::Result<WriteStats> {
        let stats = self.join()?;
        if self.leased.iter().any(|leased| *leased) || stats.frames != self.next_frame {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "readback frames were not submitted",
            ));
        }
        Ok(stats)
    }
}

impl Drop for FrameHandoff {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

/// The last two in-flight frames can occupy slots in reverse order when the
/// output has an odd frame count. Preserve presentation order when draining.
pub(crate) fn take_pending<const N: usize>(
    pending: &mut [Option<(u64, usize)>; N],
) -> Vec<(u64, usize)> {
    let mut frames: Vec<_> = pending.iter_mut().filter_map(Option::take).collect();
    frames.sort_unstable_by_key(|frame| frame.0);
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Collect(Arc<Mutex<Vec<u8>>>);
    impl Write for Collect {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            // Force write_all to handle partial writes before returning a slot.
            let n = bytes.len().min(1);
            self.0.lock().unwrap().extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn odd_tail_keeps_frame_order() {
        let buffers = [vec![0u8; 3], vec![1u8; 3], vec![2u8; 3]];
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut handoff = unsafe {
            FrameHandoff::new_mapped(
                buffers.iter().map(|v| v.as_ptr() as usize).collect(),
                3,
                Collect(written.clone()),
            )
        }
        .unwrap();
        let slots: Vec<_> = (0..3).map(|_| handoff.acquire_slot().unwrap()).collect();
        handoff.submit(0, slots[0]).unwrap();
        let mut tail = [Some((2, slots[2])), Some((1, slots[1]))];
        for (index, slot) in take_pending(&mut tail) {
            handoff.submit(index, slot).unwrap();
        }
        assert_eq!(handoff.finish().unwrap().frames, 3);
        assert_eq!(*written.lock().unwrap(), vec![0, 0, 0, 1, 1, 1, 2, 2, 2]);
        assert_eq!(tail, [None, None]);
    }

    struct Gate {
        entered: Sender<()>,
        release: Receiver<()>,
        fail: bool,
        panic: bool,
    }
    impl Write for Gate {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            assert!(!self.panic, "simulated writer panic");
            if self.fail {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "simulated pipe failure",
                ));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn slow_writer_holds_slot_until_write_completes() {
        let bytes = vec![42u8; 4];
        let (entered, writing) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut handoff = unsafe {
            FrameHandoff::new_mapped(
                vec![bytes.as_ptr() as usize],
                4,
                Gate {
                    entered,
                    release: gate,
                    fail: false,
                    panic: false,
                },
            )
        }
        .unwrap();
        let slot = handoff.acquire_slot().unwrap();
        handoff.submit(0, slot).unwrap();
        writing.recv().unwrap();
        assert!(matches!(
            handoff.available.try_recv(),
            Err(TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        let slot = handoff.acquire_slot().unwrap();
        assert_eq!(slot, 0);
        // An acquired, unsubmitted slot is safe during cancellation/Drop.
        drop(handoff);
    }

    #[test]
    fn failure_and_panic_wake_blocked_producer() {
        for panic in [false, true] {
            let bytes = vec![42u8; 4];
            let (entered, writing) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            let mut handoff = unsafe {
                FrameHandoff::new_mapped(
                    vec![bytes.as_ptr() as usize],
                    4,
                    Gate {
                        entered,
                        release: gate,
                        fail: !panic,
                        panic,
                    },
                )
            }
            .unwrap();
            let slot = handoff.acquire_slot().unwrap();
            handoff.submit(0, slot).unwrap();
            writing.recv().unwrap();
            let producer = thread::spawn(move || handoff.acquire_slot());
            release.send(()).unwrap();
            let error = producer.join().unwrap().unwrap_err();
            assert!(error
                .to_string()
                .contains(if panic { "panicked" } else { "pipe failure" }));
        }
    }

    #[test]
    fn early_drop_joins_reader_before_storage_can_be_released() {
        let bytes = vec![42u8; 4];
        let (entered, writing) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut handoff = unsafe {
            FrameHandoff::new_mapped(
                vec![bytes.as_ptr() as usize],
                4,
                Gate {
                    entered,
                    release: gate,
                    fail: false,
                    panic: false,
                },
            )
        }
        .unwrap();
        let slot = handoff.acquire_slot().unwrap();
        handoff.submit(0, slot).unwrap();
        writing.recv().unwrap();
        let (dropped, done) = mpsc::channel();
        let (dropping, started) = mpsc::channel();
        let cleanup = thread::spawn(move || {
            dropping.send(()).unwrap();
            drop(handoff);
            dropped.send(()).unwrap();
        });
        started.recv().unwrap();
        assert!(matches!(
            done.recv_timeout(std::time::Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        cleanup.join().unwrap();
        done.recv().unwrap();
        drop(bytes);
    }
}

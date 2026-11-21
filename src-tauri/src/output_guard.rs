//! Per-request output ownership: encode privately, publish only on success.
//!
//! A unique directory beside the destination owns every partial file, including
//! nested renderer/encoder temporary files. Callers must close and join their
//! writers before dropping this guard. Never clean a guessed destination path.
use std::{io, path::{Path, PathBuf}};

pub(crate) struct OutputGuard {
    directory: tempfile::TempDir,
    working: PathBuf,
    destination: PathBuf,
}

impl OutputGuard {
    pub(crate) fn new(destination: &Path) -> io::Result<Self> {
        // Reject an existing file (including dangling symlinks) before launching
        // an encoder. The publish operation also checks atomically for races.
        match std::fs::symlink_metadata(destination) {
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists,
                format!("output already exists: {}", destination.display()))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let parent = destination.parent().filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let directory = tempfile::Builder::new().prefix(".phi-tkc-output-").tempdir_in(parent)?;
        let name = destination.file_name().ok_or_else(||
            io::Error::new(io::ErrorKind::InvalidInput, "output has no file name"))?;
        let working = directory.path().join(name);
        Ok(Self { directory, working, destination: destination.to_owned() })
    }

    pub(crate) fn path(&self) -> &Path { &self.working }

    /// Publish without replacing an existing successful/user output. The working
    /// file is on the same filesystem, and all writer handles must already close.
    pub(crate) fn commit(&self) -> io::Result<()> {
        // tempfile uses renameat2(RENAME_NOREPLACE) on Linux/Android,
        // renameatx_np(RENAME_EXCL) on Apple, and MoveFileExW without replacement
        // on Windows. These also work on FAT/exFAT, unlike hard_link alone.
        // Older kernels fall back to an equally no-clobber hard link.
        tempfile::TempPath::try_from_path(self.working.clone())?
            .persist_noclobber(&self.destination)
            .map_err(|error| error.error)
    }

    /// Explicit cleanup reports failures (notably Windows file locks). Drop is
    /// still the fallback for unwinding and other early error returns.
    pub(crate) fn cleanup(self) -> io::Result<()> { self.directory.close() }
}

/// Tell the renderer to unwind cooperatively. A direct kill of the renderer
/// leaves its ffmpeg descendants running, holding and even recreating output.
pub(crate) async fn request_stop(stdin: &mut tokio::process::ChildStdin) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    stdin.write_all(b"\"cancel\"\n").await?;
    stdin.flush().await
}

/// Wait for encoder teardown even after stdout EOF. Never unlink an output
/// while a renderer or encoder might still have it open.
pub(crate) async fn wait_for_renderer(
    child: &mut tokio::process::Child,
    stdin: &mut tokio::process::ChildStdin,
    cancel: &std::sync::atomic::AtomicBool,
    stop_sent: &mut bool,
) -> io::Result<std::process::ExitStatus> {
    use std::{sync::atomic::Ordering, time::Duration};
    loop {
        if !*stop_sent && cancel.load(Ordering::Relaxed) {
            // BrokenPipe usually means the renderer has already exited. Still
            // reap it rather than returning early and racing output cleanup.
            let _ = request_stop(stdin).await;
            *stop_sent = true;
        }
        match tokio::time::timeout(Duration::from_millis(250), child.wait()).await {
            Ok(status) => return status,
            Err(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputGuard;

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_waits_after_stdout_eof_before_cleaning_output() {
        use std::{process::Stdio, sync::atomic::AtomicBool};
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("video.mp4");
        let closed = root.path().join("writer-closed");
        let guard = OutputGuard::new(&target).unwrap();
        let working = guard.path().to_owned();
        let mut child = tokio::process::Command::new("/bin/sh")
            .arg("-c").arg("printf partial > \"$1\"; exec 3>>\"$1\"; exec 1>&-; IFS= read -r stop; printf finalized >&3; exec 3>&-; printf closed > \"$2\"")
            .arg("renderer").arg(&working).arg(&closed)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        // A renderer can close progress stdout before its encoder finishes.
        super::super::process_output::drain(child.stdout.take().unwrap()).await.unwrap();
        assert!(!closed.exists());
        let mut stop_sent = false;
        let status = super::wait_for_renderer(&mut child, &mut stdin, &AtomicBool::new(true), &mut stop_sent).await.unwrap();
        assert!(status.success());
        assert!(stop_sent);
        assert_eq!(std::fs::read(&closed).unwrap(), b"closed");
        assert_eq!(std::fs::read(&working).unwrap(), b"partialfinalized");
        guard.cleanup().unwrap();
        assert!(!working.parent().unwrap().exists());
        assert!(!target.exists());
    }

    #[test]
    fn failed_request_removes_only_its_private_output() {
        let root = tempfile::tempdir().unwrap();
        let previous = root.path().join("previous.mp4");
        std::fs::write(&previous, b"successful video").unwrap();
        let target = root.path().join("new.mov");
        let guard = OutputGuard::new(&target).unwrap();
        let working = guard.path().to_owned();
        assert_eq!(working.extension(), target.extension());
        std::fs::write(&working, b"partial video").unwrap();
        // A killed renderer can leave its own nested encoder temporary files.
        std::fs::create_dir(working.parent().unwrap().join("nested")).unwrap();
        std::fs::write(working.parent().unwrap().join("nested/partial"), b"partial").unwrap();
        guard.cleanup().unwrap();
        assert!(!working.parent().unwrap().exists());
        assert!(!target.exists());
        assert_eq!(std::fs::read(previous).unwrap(), b"successful video");
    }

    #[test]
    fn existing_output_and_publish_race_are_never_clobbered() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("video.mp4");
        std::fs::write(&target, b"old success").unwrap();
        assert_eq!(OutputGuard::new(&target).err().unwrap().kind(), std::io::ErrorKind::AlreadyExists);
        let target = root.path().join("raced.mp4");
        let guard = OutputGuard::new(&target).unwrap();
        std::fs::write(guard.path(), b"new output").unwrap();
        std::fs::write(&target, b"other request success").unwrap();
        assert_eq!(guard.commit().unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        guard.cleanup().unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"other request success");
    }

    #[test]
    fn success_survives_cleanup_and_parallel_requests_are_isolated() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("video.mp4");
        let first = OutputGuard::new(&target).unwrap();
        let second = OutputGuard::new(&target).unwrap();
        assert_ne!(first.path(), second.path());
        std::fs::write(first.path(), b"complete video").unwrap();
        first.commit().unwrap();
        first.cleanup().unwrap();
        std::fs::write(second.path(), b"canceled partial").unwrap();
        second.cleanup().unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"complete video");
    }
}

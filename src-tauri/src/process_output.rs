//! Drain child diagnostics while its stdout carries progress events.
use std::{process::ExitStatus, sync::atomic::{AtomicBool, Ordering}, time::Duration};
use tokio::{io::{AsyncRead, AsyncReadExt}, process::Child};

pub(crate) async fn drain<R: AsyncRead + Unpin>(mut reader: R) -> std::io::Result<String> {
    const LIMIT: usize = 1 << 20;
    let mut log = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 { break; }
        // Keep consuming after the display limit, otherwise a full stderr pipe
        // prevents the renderer from making progress or exiting.
        let keep = read.min(LIMIT.saturating_sub(log.len()));
        log.extend_from_slice(&buffer[..keep]);
    }
    Ok(String::from_utf8_lossy(&log).into_owned())
}

pub(crate) async fn wait(child: &mut Child, cancel: &AtomicBool) -> std::io::Result<ExitStatus> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            child.kill().await?;
            return child.wait().await;
        }
        match tokio::time::timeout(Duration::from_millis(250), child.wait()).await {
            Ok(status) => return status,
            Err(_) => continue,
        }
    }
}

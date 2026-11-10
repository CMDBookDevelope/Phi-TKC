//! File logging for the in-process Apple renderer.
use anyhow::Result;
use std::{io::Write, path::PathBuf, sync::Mutex};
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
pub struct RenderLog;
static LOGGER: NativeLogger = NativeLogger;
struct NativeLogger;
impl log::Log for NativeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            debug_log(&format!("[{}] {}", record.level(), record.args()));
        }
    }
    fn flush(&self) {}
}
impl Drop for RenderLog {
    fn drop(&mut self) {
        if let Ok(mut path) = LOG_PATH.lock() {
            *path = None;
        }
    }
}
pub fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
pub fn start_render_log(path: PathBuf) -> Result<RenderLog> {
    std::fs::File::create(&path)?;
    *LOG_PATH
        .lock()
        .map_err(|_| anyhow::anyhow!("render log lock poisoned"))? = Some(path);
    Ok(RenderLog)
}
pub fn debug_log(message: &str) {
    eprintln!("{message}");
    if let Ok(path) = LOG_PATH.lock() {
        if let Some(path) = path.as_ref() {
            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
                let _ = writeln!(file, "{message}");
            }
        }
    }
}

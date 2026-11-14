//! File logging for the in-process Apple renderer.
use anyhow::Result;
use std::{io::Write, path::PathBuf, sync::Mutex};
#[cfg(target_os = "ios")]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "ios")]
pub fn register_app(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

#[cfg(target_os = "ios")]
pub fn save_to_photos(path: &std::path::Path) -> String {
    use tauri::Manager;
    crate::apple_lifecycle::wait_until_active();
    let Some(app) = APP.get() else {
        return "无法自动保存到相册：应用尚未初始化。视频已保留在应用内。".into();
    };
    match app
        .state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>()
        .save_video(&path.to_string_lossy())
    {
        Ok(result) if result.saved && result.asset_id.is_some() => result
            .message
            .unwrap_or_else(|| "已自动保存到相册。".into()),
        Ok(result) => result
            .message
            .unwrap_or_else(|| "无法自动保存到相册，视频已保留在应用内。".into()),
        Err(error) => format!("无法自动保存到相册，视频已保留在应用内：{error}"),
    }
}
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

//! File logging for the in-process Apple renderer.
use anyhow::Result;
use std::{io::Write, path::PathBuf, sync::Mutex};
#[cfg(target_os = "ios")]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "ios")]
pub fn register_app(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

pub(crate) struct PhotoPublication {
    pub target: String,
    pub note: String,
}

/// A placeholder ID without successful performChanges is not a saved video.
/// Preserve the private recovery path unless both proofs are present.
fn photo_publication(path: &std::path::Path, saved: bool, asset_id: Option<String>, message: Option<String>) -> PhotoPublication {
    let asset_id = asset_id.filter(|id| !id.trim().is_empty());
    if let Some(id) = asset_id.filter(|_| saved) {
        PhotoPublication {
            target: format!("ph://{id}"),
            note: message.unwrap_or_else(|| "已自动保存到相册。".into()),
        }
    } else {
        PhotoPublication {
            target: path.to_string_lossy().into_owned(),
            note: message.unwrap_or_else(|| "无法自动保存到相册，视频已保留在应用内。".into()),
        }
    }
}

#[cfg(target_os = "ios")]
pub(crate) fn save_to_photos(path: &std::path::Path) -> PhotoPublication {
    use tauri::Manager;
    crate::apple_lifecycle::wait_until_active();
    let Some(app) = APP.get() else {
        return photo_publication(path, false, None, Some("无法自动保存到相册：应用尚未初始化。视频已保留在应用内。".into()));
    };
    match app
        .state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>()
        .save_video(&path.to_string_lossy())
    {
        Ok(result) => photo_publication(path, result.saved, result.asset_id, result.message),
        Err(error) => photo_publication(path, false, None, Some(format!("无法自动保存到相册，视频已保留在应用内：{error}"))),
    }
}
#[cfg(test)]
mod tests {
    use super::photo_publication;

    #[test]
    fn photos_failure_or_missing_identifier_preserves_recovery_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("video.mov");
        std::fs::write(&path, b"complete video").unwrap();
        for (saved, id) in [(false, Some("placeholder")), (true, None), (true, Some("")), (true, Some("  "))] {
            let publication = photo_publication(&path, saved, id.map(str::to_owned), None);
            assert_eq!(publication.target, path.to_string_lossy());
            assert!(publication.note.contains("保留"));
            assert_eq!(std::fs::read(&path).unwrap(), b"complete video");
        }
    }

    #[test]
    fn photos_success_reports_asset_uri_instead_of_private_file() {
        let publication = photo_publication(std::path::Path::new("/private/video.mov"), true,
            Some("F00D/L0/001".into()), None);
        assert_eq!(publication.target, "ph://F00D/L0/001");
        assert!(publication.note.contains("已自动保存"));
        assert!(!publication.target.contains("/private/"));
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

//! Android-side helpers for the in-process renderer.
//!
//! The upstream `Winamin/Phi-TK` android branch routes chart picking and video
//! publishing through a Tauri mobile plugin. Phi-TKC keeps those out of the
//! render path: a render never depends on the WebView bridge, so this module
//! only provides logcat/file logging and plain path handling. Files picked by
//! the Tauri dialog plugin arrive as normal filesystem paths (the Android
//! content resolver is handled by the plugin), which is all the renderer needs.
use anyhow::{Context, Result};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

static RENDER_LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
static LOGGER: AndroidLogger = AndroidLogger;

struct AndroidLogger;

impl log::Log for AndroidLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            debug_log(&format!("[{}] {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

/// Install the logcat logger. `log` only accepts one logger per process, so a
/// second call (a later render) is a no-op.
pub(crate) fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

pub(crate) struct RenderLog;

impl Drop for RenderLog {
    fn drop(&mut self) {
        if let Ok(mut path) = RENDER_LOG.lock() {
            *path = None;
        }
    }
}

/// Mirror every log line into a file next to the output video, so a crash on a
/// device without `adb` still leaves something to look at.
pub(crate) fn start_render_log(path: PathBuf) -> Result<RenderLog> {
    std::fs::File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
    *RENDER_LOG
        .lock()
        .map_err(|error| anyhow::anyhow!("{error}"))? = Some(path);
    Ok(RenderLog)
}

pub(crate) fn debug_log(message: &str) {
    if let Ok(path) = RENDER_LOG.lock() {
        if let Some(path) = path.as_ref() {
            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
                let _ = writeln!(file, "{message}");
            }
        }
    }
    extern "C" {
        fn __android_log_write(
            prio: i32,
            tag: *const std::os::raw::c_char,
            text: *const std::os::raw::c_char,
        ) -> i32;
    }
    if let Ok(text) = std::ffi::CString::new(message) {
        unsafe {
            // 4 = ANDROID_LOG_INFO
            __android_log_write(4, b"PhiTK\0".as_ptr() as _, text.as_ptr());
        }
    }
}

/// The Android file picker may hand back `content://` URIs. The Tauri dialog
/// plugin already imports those into the cache directory, so anything left is a
/// plain path that only needs an existence check.
pub fn resolve(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    anyhow::ensure!(
        !text.starts_with("content://"),
        "content:// URIs must be imported by the file picker"
    );
    anyhow::ensure!(
        path.exists(),
        "The chart is no longer available. Please select it again."
    );
    Ok(path.to_path_buf())
}

/// Move the finished video somewhere the user can find it.
///
/// The desktop version just writes next to the chosen output path; on Android
/// the app-private directory is not browsable, so try the public Movies folder
/// and fall back to the source path when scoped storage forbids it.
pub fn publish_video(source: &Path, display_name: &str) -> Result<String> {
    anyhow::ensure!(source.is_file(), "the rendered video is missing");
    let movies = Path::new("/storage/emulated/0/Movies");
    if movies.is_dir() {
        let name = if display_name.to_ascii_lowercase().ends_with(".mp4") {
            display_name.to_owned()
        } else {
            format!("{display_name}.mp4")
        };
        let target = movies.join(name);
        if std::fs::copy(source, &target).is_ok() {
            return Ok(target.to_string_lossy().into_owned());
        }
    }
    Ok(source.to_string_lossy().into_owned())
}

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

/// Make sure `path` is a real file the renderer can open.
///
/// Android's document picker returns `content://` URIs, not filesystem paths
/// (the Tauri dialog plugin passes the URI through). Those are imported into the
/// app cache through the ContentResolver so everything downstream — chart
/// parsing, the render subprocess, the illustration loader — only ever sees a
/// normal path.
pub fn resolve(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    if !text.starts_with("content://") {
        anyhow::ensure!(
            path.exists(),
            "The chart is no longer available. Please select it again."
        );
        return Ok(path.to_path_buf());
    }
    import_content_uri(&text)
}

#[cfg(target_os = "android")]
fn import_content_uri(uri: &str) -> Result<PathBuf> {
    use jni::{
        objects::{JObject, JValue},
        JavaVM,
    };
    use std::{
        hash::{Hash, Hasher},
        io::Read,
        os::fd::FromRawFd,
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut hasher);
    let hash = hasher.finish();

    let ctx = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    // ParcelFileDescriptor.detachFd() hands us a raw fd, which is far simpler
    // (and cheaper) than streaming a Java InputStream through JNI byte arrays.
    let resolver = env
        .call_method(&context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?
        .l()?;
    let uri_text = JObject::from(env.new_string(uri)?);
    let uri_obj = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&uri_text)],
        )?
        .l()?;
    let mode = JObject::from(env.new_string("r")?);
    let fd_obj = env
        .call_method(
            &resolver,
            "openFileDescriptor",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;",
            &[JValue::Object(&uri_obj), JValue::Object(&mode)],
        )?
        .l()?;
    if fd_obj.is_null() {
        anyhow::bail!("The picker did not return a readable file");
    }
    let fd = env.call_method(&fd_obj, "detachFd", "()I", &[])?.i()?;
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    // The URI usually has no useful extension; sniff the archive magic instead.
    let ext = if data.starts_with(b"PK\x03\x04") {
        "pez"
    } else if data.first() == Some(&b'{') {
        "json"
    } else {
        "bin"
    };
    let path = std::env::temp_dir().join(format!("picked-{hash:016x}.{ext}"));
    std::fs::write(&path, &data)?;
    debug_log(&format!("imported {uri} -> {} ({} bytes)", path.display(), data.len()));
    Ok(path)
}

#[cfg(not(target_os = "android"))]
fn import_content_uri(uri: &str) -> Result<PathBuf> {
    anyhow::bail!("content URIs are only used on Android: {uri}")
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

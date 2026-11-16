//! Android-side helpers for the in-process renderer.
//!
//! Chart imports and finished videos use Android's ContentResolver through JNI.
//! Publishing runs on the native worker, independently of the WebView bridge.
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

/// Registered by the Android plugin before any import or render can start.
/// A global application reference stays valid on attached native workers.
#[cfg(target_os = "android")]
struct AndroidContext {
    vm: jni::JavaVM,
    application: jni::objects::GlobalRef,
}

#[cfg(target_os = "android")]
static ANDROID_CONTEXT: std::sync::OnceLock<AndroidContext> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
fn android_context() -> Result<&'static AndroidContext> {
    ANDROID_CONTEXT.get().context("Android media bridge has not been initialized")
}

#[cfg(target_os = "android")]
fn java_vm() -> Result<&'static jni::JavaVM> {
    Ok(&android_context()?.vm)
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_cnmdbook_chartpicker_VideoPublisher_initialize(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    application: jni::objects::JObject,
) {
    let result = (|| -> Result<()> {
        anyhow::ensure!(!application.is_null(), "no Android application context");
        let context = AndroidContext {
            vm: env.get_java_vm()?,
            application: env.new_global_ref(application)?,
        };
        let _ = ANDROID_CONTEXT.set(context);
        Ok(())
    })();
    if let Err(error) = result {
        debug_log(&format!("media bridge initialization failed: {error:#}"));
        let _ = env.throw_new("java/lang/IllegalStateException", format!("Cannot initialize Android media bridge: {error:#}"));
    }
}

#[cfg(target_os = "android")]
fn import_content_uri(uri: &str) -> Result<PathBuf> {
    use jni::objects::{JObject, JValue};
    use std::{
        hash::{Hash, Hasher},
        io::Read,
        os::fd::FromRawFd,
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut hasher);
    let hash = hasher.finish();

    let vm = java_vm()?;
    let mut env = vm.attach_current_thread()?;

    let app = android_context()?.application.as_obj();

    // ParcelFileDescriptor.detachFd() hands us a raw fd, which is far simpler
    // (and cheaper) than streaming a Java InputStream through JNI byte arrays.
    let resolver = env
        .call_method(&app, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?
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
    debug_log(&format!("import: fd={fd}"));
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    debug_log(&format!("import: read {} bytes", data.len()));

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

/// Publish the finished video into Android's media library. Only a completed
/// media transaction returns success; failures retain the private source file.
pub fn publish_video(source: &Path, display_name: &str) -> Result<String> {
    anyhow::ensure!(source.is_file(), "the rendered video is missing");
    #[cfg(target_os = "android")]
    {
        publish_to_media_store(source, display_name)
            .with_context(|| format!("Failed to save to the media library; original video: {}", source.display()))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = display_name;
        Ok(source.to_string_lossy().into_owned())
    }
}

#[cfg(target_os = "android")]
fn publish_to_media_store(source: &Path, display_name: &str) -> Result<String> {
    use jni::objects::{JClass, JObject, JString, JValue};

    let mut env = java_vm()?.attach_current_thread()?;
    let result = (|| -> Result<String> {
        let app = android_context()?.application.as_obj();
        // JNI FindClass on an attached native thread uses the system loader.
        // Resolve the packaged publisher with the application's loader instead.
        let loader = env.call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
        let class_name = JObject::from(env.new_string("com.cnmdbook.chartpicker.VideoPublisher")?);
        let class = JClass::from(env.call_method(
            &loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[JValue::Object(&class_name)],
        )?.l()?);
        let source_text = JObject::from(env.new_string(source.to_string_lossy().as_ref())?);
        let name_text = JObject::from(env.new_string(display_name)?);
        let uri = JString::from(env.call_static_method(
            class, "publishVideo", "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&app), JValue::Object(&source_text), JValue::Object(&name_text)],
        )?.l()?);
        let uri: String = env.get_string(&uri)?.into();
        anyhow::ensure!(uri.starts_with("content://"), "media publisher returned no content URI");
        debug_log(&format!("gallery published: {uri}"));
        Ok(uri)
    })();
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let exception = env.exception_occurred()?;
        env.exception_clear()?;
        let message = env.call_method(exception, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let message = JString::from(message);
        let message: String = env.get_string(&message)?.into();
        anyhow::bail!("Android media publisher: {message}");
    }
    result
}

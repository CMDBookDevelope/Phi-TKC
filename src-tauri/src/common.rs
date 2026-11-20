use anyhow::{bail, Result};
use std::{path::PathBuf, sync::OnceLock};

pub static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
pub static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
pub enum RenderProgress {
    Preparing,
    Mixing(f32),
    Encoding { fraction: f32, fps: f64 },
    Finalizing,
}

/// Completed output frames and the renderer's measured wall time through
/// encoder finalization, matching the duration shown on the task.
pub struct RenderStats {
    pub frames: u64,
    pub duration: f64,
}

impl RenderStats {
    pub fn average_fps(&self) -> f64 {
        if self.duration.is_finite() && self.duration > 0.0 {
            self.frames as f64 / self.duration
        } else {
            0.0
        }
    }
}

/// Clear texture owners before macroquad tears down the native GPU context.
/// Android's pthread-key destructors can run after that context has gone away.
#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
pub(crate) struct NativeRenderResources;

#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
impl Drop for NativeRenderResources {
    fn drop(&mut self) {
        prpr::scene::clear_thread_resources();
    }
}

// macroquad owns process-global graphics state; a preview and a video task
// must never initialize it concurrently.
#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
static NATIVE_GRAPHICS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
pub(crate) struct NativeGraphicsGuard;

#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
impl NativeGraphicsGuard {
    pub fn try_acquire() -> Option<Self> {
        NATIVE_GRAPHICS_BUSY.compare_exchange(false, true,
            std::sync::atomic::Ordering::Acquire, std::sync::atomic::Ordering::Relaxed)
            .ok().map(|_| Self)
    }
}

#[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
impl Drop for NativeGraphicsGuard {
    fn drop(&mut self) {
        NATIVE_GRAPHICS_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub fn ensure_dir(path: PathBuf) -> PathBuf {
    if path.exists() {
        if !path.is_dir() {
            panic!("{} is not a directory", path.display());
        }
    } else {
        std::fs::create_dir_all(&path).unwrap();
    }
    path
}

pub fn output_dir() -> Result<PathBuf> {
    #[cfg(target_os = "ios")]
    let dir = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("iOS sandbox home is unavailable"))?).join("Documents/output");
    #[cfg(not(target_os = "ios"))]
    let dir = DATA_DIR.get().unwrap().join("output");
    if dir.exists() {
        if !dir.is_dir() {
            bail!("output directory is not a directory");
        }
    } else {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn respack_dir() -> Result<PathBuf> {
    let dir = CONFIG_DIR.get().unwrap().join("respack");
    if dir.exists() {
        if !dir.is_dir() {
            bail!("resource pack directory is not a directory");
        }
    } else {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

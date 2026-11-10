use anyhow::{bail, Result};
use std::{path::PathBuf, sync::OnceLock};

pub static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
pub static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

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

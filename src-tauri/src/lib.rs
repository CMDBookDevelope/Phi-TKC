// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

prpr::tl_file!("main" mtl);

mod av1_level;
mod frame_handoff;
mod common;
mod chart_archive;
mod gpu_info;
pub mod ipc;
pub mod logger;
mod preview;
mod process_output;
pub mod render;
pub mod server;
mod task;
#[cfg(any(target_os = "ios", feature = "apple-render-check"))]
mod apple_media;
#[cfg(any(target_os = "ios", feature = "apple-render-check"))]
mod apple_platform;
#[cfg(any(target_os = "ios", feature = "apple-render-check"))]
mod apple_render;
mod vulkan_encode;
mod vulkan_readback;
mod vulkan_video;

// The Android build renders in-process with the NDK MediaCodec/MediaMuxer
// pipeline instead of spawning `render` + ffmpeg (Android forbids fork/exec).
#[cfg(target_os = "android")]
mod android_media;
#[cfg(target_os = "android")]
pub mod android_render;
#[cfg(target_os = "android")]
pub mod android_uri;

use anyhow::{bail, Context, Result};
use common::{ensure_dir, output_dir, respack_dir, CONFIG_DIR, DATA_DIR};
use fs4::tokio::AsyncFileExt;
use gpu_info::get_msaa_sample_counts;
use macroquad::prelude::set_pc_assets_folder;
use prpr::{
    fs::{self, FileSystem},
    info::ChartInfo,
};
use render::{find_ffmpeg, RenderConfig, RenderParams};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::File,
    future::Future,
    io::{BufRead, BufReader, BufWriter},
    ops::DerefMut,
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::SystemTime,
};
use task::{TaskQueue, TaskView};
use tauri::{ipc::InvokeError, Manager, State, WindowEvent};
use tokio::{io::AsyncWriteExt, process::Command};

static ASSET_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK_FILE: OnceLock<tokio::fs::File> = OnceLock::new();

#[inline]
async fn wrap_async<R>(f: impl Future<Output = Result<R>>) -> Result<R, InvokeError> {
    f.await.map_err(|e| {
        log::error!("{e:?}");
        InvokeError::from_anyhow(e)
    })
}

pub async fn run_wrapped(f: impl Future<Output = Result<()>>) -> ! {
    if let Err(err) = f.await {
        log::error!("{err:?}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

/// Write a line to logcat (`adb logcat -s Phi-TKC`). On Android the process has no
/// stderr anyone can read, so panics and early-startup failures would otherwise be
/// invisible — this mirrors what Winamin/Phi-TK does on its `android` branch.
#[cfg(target_os = "android")]
fn android_log(message: &str) {
    use std::ffi::CString;

    extern "C" {
        fn __android_log_write(
            prio: i32,
            tag: *const std::os::raw::c_char,
            text: *const std::os::raw::c_char,
        ) -> i32;
    }

    if let Ok(text) = CString::new(message) {
        unsafe {
            __android_log_write(4, b"Phi-TKC\0".as_ptr() as _, text.as_ptr());
        }
    }
}

#[cfg(target_os = "android")]
fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        android_log(&format!("PANIC {info}"));
    }));
}

/// `miniquad`'s legacy Android backend references an external `quad_main` symbol
/// (`src/native/android.rs`), even though Tauri/wry never calls it. Without a
/// definition the dynamic loader refuses to load `libapp_lib.so`:
///
///   UnsatisfiedLinkError: dlopen failed: cannot locate symbol "quad_main"
///
/// Winamin/Phi-TK's android branch defines it for its native preview; we keep a
/// no-op so the library loads. Nothing in the Tauri app calls it.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn quad_main() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() -> Result<()> {
    #[cfg(not(mobile))]
    logger::init();
    #[cfg(target_os = "android")]
    install_panic_logger();

    //dynamic cpu core
    let num_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_workers)
        .enable_all()
        .build()?;
    let _guard = rt.enter();

    // Android may hand the process argv values it never asked for (and there is no
    // CLI there anyway); dispatching on them could start a render on the UI thread.
    #[cfg(not(mobile))]
    {
        match std::env::args().skip(1).next().as_deref() {
            Some("render") => {
                run_wrapped(render::main()).await;
            }
            Some("preview") => {
                run_wrapped(preview::main()).await;
            }
            // Not an internal subcommand: fall through to the GUI. On macOS Finder may
            // pass `-psn_...`; CLI flags are handled by main.rs before reaching here.
            _ => {}
        }
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_chart_picker::init())
        .setup(|app| {
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let cover_dir = Some(app.path().app_cache_dir()?.join("task-covers"));
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            let cover_dir = None;
            app.manage(TaskQueue::new(cover_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            is_the_only_instance,
            exit_program,
            show_folder,
            show_in_folder,
            preview_chart,
            parse_chart,
            pick_chart,
            pick_chart_folder,
            batch_add_tasks,
            post_render,
            get_tasks,
            cancel_task,
            get_respacks,
            open_respack_folder,
            get_presets,
            add_preset,
            remove_preset,
            set_rpe_dir,
            unset_rpe_dir,
            get_rpe_charts,
            test_ffmpeg,
            open_app_folder,
            list_chart_files,
            save_text_file,
            save_binary_file,
            load_chart_illustration,
            get_msaa_sample_counts,
            remote_status,
            remote_jobs,
            remote_post_render,
            remote_cancel,
            start_render_server,
            stop_render_server,
            render_server_status,
        ])
        .on_window_event(|_, event| match event {
            //WindowEvent::CloseRequested { api, .. } => {
            WindowEvent::CloseRequested { .. } => {
                /*event
                .window()
                .app_handle()
                .tray_handle()
                .get_item("toggle")
                .set_title(mtl!("tray-show"))
                .unwrap();*/
                exit_program();
                //event.window().hide().unwrap();
                //api.prevent_close();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    let resolver = app.path();
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap();

    let cache_dir = ensure_dir(
        resolver
            .app_cache_dir()
            .unwrap_or_else(|_| exe_dir.to_owned()),
    );
    let lock_file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(cache_dir.join("app.lock"))
        .await?;
    if lock_file.try_lock_exclusive().is_ok() {
        LOCK_FILE.set(lock_file).unwrap();
    } else {
        log::warn!("Lock failed");
    }

    CONFIG_DIR
        .set(ensure_dir(
            resolver
                .app_config_dir()
                .unwrap_or_else(|_| exe_dir.to_owned()),
        ))
        .unwrap();
    DATA_DIR
        .set(ensure_dir(
            resolver
                .app_data_dir()
                .unwrap_or_else(|_| exe_dir.to_owned()),
        ))
        .unwrap();

    // Resolve the renderer's assets. The layout differs per packaging:
    //   1. normal Tauri bundle / deb / rpm / AppImage -> <resource_dir>/assets
    //   2. legacy Phi-TKC deb -> /usr/lib/ptkc-assets
    //   3. `pnpm tauri dev` -> src-tauri/assets
    //   4. iOS -> Tauri's resource_dir() already ends with /assets
    //   5. Android -> the assets live inside the APK, exposed as `asset://localhost/`,
    //      which is not a filesystem path (rendering is disabled there anyway).
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(not(mobile))]
    if exe_dir.join("assets/ending.mp3").is_file() {
        candidates.push(exe_dir.join("assets"));
    }
    if let Ok(dir) = resolver.resource_dir() {
        #[cfg(target_os = "ios")]
        candidates.push(dir.clone());
        candidates.push(dir.join("assets"));
    }
    #[cfg(target_os = "linux")]
    candidates.push(PathBuf::from("/usr/lib/ptkc-assets"));
    candidates.push(exe_dir.join("assets"));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));

    if let Some(asset_dir) = candidates.into_iter().find(|dir| dir.exists()) {
        ASSET_PATH.set(asset_dir.clone()).ok();
        // Macroquad's asset folder is only meaningful on desktop; on iOS/Android the
        // renderer and preview window are gated off (see MOBILE.md).
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        set_pc_assets_folder(&asset_dir.display().to_string());
    } else {
        log::error!("错误：资源目录不存在");
        // Mobile builds must still start so the settings/tasks/about pages work even if
        // the resource layout is unexpected; never make that a fatal error there.
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        bail!("资源目录未找到，请检查安装路径");
    }

    app.run(|_, _| {});

    Ok(())
}

#[tauri::command]
fn is_the_only_instance() -> bool {
    // Mobile apps have no multi-instance concept, and a failed `flock` in the sandbox would
    // make the frontend call `exit_program()` and kill the app on launch.
    #[cfg(mobile)]
    {
        true
    }
    #[cfg(not(mobile))]
    {
        LOCK_FILE.get().is_some()
    }
}

#[tauri::command]
fn exit_program() {
    std::process::exit(0);
}

#[tauri::command]
fn show_folder() -> Result<(), InvokeError> {
    let dir = output_dir().unwrap();
    show_in_folder(&dir)
}

#[tauri::command]
fn show_in_folder(path: &Path) -> Result<(), InvokeError> {
    (move || {
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .args(["/select,", &path.display().to_string()]) // The comma after select is not a typo
                .spawn()?;
        }

        #[cfg(target_os = "linux")]
        {
            Command::new("gdbus")
                .args([
                    "call",
                    "--session",
                    "--dest",
                    "org.freedesktop.FileManager1",
                    "--object-path",
                    "/org/freedesktop/FileManager1",
                    "--method",
                    "org.freedesktop.FileManager1.ShowItems",
                    &format!("['file://{}']", path.canonicalize()?.display()),
                    "",
                ])
                .spawn()?;
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .args(["-R", &path.display().to_string()])
                .spawn()?;
        }

        Ok(())
    })()
    .map_err(InvokeError::from_anyhow)
}

/// Turn a picker result into a real file path.
///
/// Native pickers import into the app cache. Also accept legacy picker results:
/// Android `content://` URIs and Apple percent-encoded `file://` URLs.
fn localize_chart_path(path: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "android")]
    {
        crate::android_uri::resolve(path)
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Some(url) = path.to_str().filter(|value| value.starts_with("file:")) {
            return url.parse::<tauri_plugin_fs::FilePath>()?
                .into_path().context("Invalid chart file URL");
        }
        Ok(path.to_path_buf())
    }
}

#[tauri::command]
async fn pick_chart(app: tauri::AppHandle) -> Result<Option<String>, InvokeError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        return tokio::task::spawn_blocking(move || {
            app.state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>()
                .pick().map_err(|error| InvokeError::from(error.to_string()))
        }).await.map_err(|error| InvokeError::from(error.to_string()))?;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        Err(InvokeError::from("The document picker is only used on mobile"))
    }
}

#[tauri::command]
async fn pick_chart_folder(app: tauri::AppHandle) -> Result<Option<String>, InvokeError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        return tokio::task::spawn_blocking(move || {
            app.state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>()
                .pick_folder().map_err(|error| InvokeError::from(error.to_string()))
        }).await.map_err(|error| InvokeError::from(error.to_string()))?;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        Err(InvokeError::from("The folder picker is only used on mobile"))
    }
}

#[tauri::command]
async fn parse_chart(path: &Path) -> Result<ChartInfo, InvokeError> {
    wrap_async(async move {
        let path = localize_chart_path(path)?;
        let mut fs: Box<dyn FileSystem + Send + Sync + 'static> =
            fs::fs_from_file(&path).with_context(|| mtl!("read-chart-failed"))?;
        let info = fs::load_info(fs.deref_mut())
            .await
            .with_context(|| mtl!("load-info-failed"))?;
        Ok(info)
    })
    .await
}

#[tauri::command]
async fn preview_chart(mut params: RenderParams) -> Result<(), InvokeError> {
    wrap_async(async move {
        params.path = localize_chart_path(&params.path)?;
        let mut command = Command::new(std::env::current_exe()?);
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        let mut child = command
            .arg("preview")
            .arg(ASSET_PATH.get().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&params)?).as_bytes())
            .await?;

        Ok(())
    })
    .await
}
#[tauri::command]
async fn post_render(app: tauri::AppHandle, queue: State<'_, TaskQueue>, mut params: RenderParams, output_path: Option<String>) -> Result<u32, InvokeError> {
    prepare_platform_assets(app).await?;
    wrap_async(async move {
        params.path = localize_chart_path(&params.path)?;
        // Mobile output lives in the app's accessible output directory. A stale
        // desktop setting cannot grant Files/SAF access to an external folder.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let output_path: Option<String> = None;
        let output_path = output_path.map(PathBuf::from);
        Ok(queue.post(params, output_path).await?)
    })
    .await
}

#[tauri::command]
async fn get_tasks(queue: State<'_, TaskQueue>) -> Result<Vec<TaskView>, InvokeError> {
    wrap_async(async move { Ok(queue.tasks().await) }).await
}

#[tauri::command]
async fn cancel_task(queue: State<'_, TaskQueue>, id: u32) -> Result<(), InvokeError> {
    queue.cancel(id).await;
    Ok(())
}

#[derive(Serialize)]
struct RespackInfo {
    name: String,
    path: String,
}
#[tauri::command]
fn get_respacks() -> Result<Vec<RespackInfo>, InvokeError> {
    (|| {
        let dir = respack_dir()?;
        let mut names: Vec<RespackInfo> = dir
            .read_dir()?
            .filter_map(|it| {
                let it = it.ok()?;
                if !it.path().is_file() {
                    return None;
                }
                Some(RespackInfo {
                    name: it.file_name().to_str()?.to_owned(),
                    path: it.path().canonicalize().ok()?.display().to_string(),
                })
            })
            .collect();
        names.sort_by(|x, y| x.name.cmp(&y.name));
        Ok(names)
    })()
    .map_err(InvokeError::from_anyhow)
}

#[tauri::command]
async fn batch_add_tasks(
    app: tauri::AppHandle,
    queue: State<'_, TaskQueue>,
    tasks: Vec<(String, String, Option<String>)> // (path, preset_name, output_path)
) -> Result<(), InvokeError> {
    prepare_platform_assets(app).await?;
    wrap_async(async move {
        for (path, preset_name, output_path) in tasks {
            let mut presets = get_presets().await.map_err(|e| anyhow::anyhow!("Failed to get presets: {:?}", e))?;
            let config = presets.remove(&preset_name).unwrap_or_else(|| {
                create_default_render_config()
            });
            let params = RenderParams {
                path: localize_chart_path(Path::new(&path))?,
                config,
                info: ChartInfo::default(),
            };
            let output_path = output_path.map(PathBuf::from);
            queue.post(params, output_path).await.map_err(|e| anyhow::anyhow!("Failed to post task: {:?}", e))?;
        }
        Ok(())
    }).await
}

#[tauri::command]
async fn list_chart_files(path: PathBuf) -> Result<Vec<String>, InvokeError> {
    wrap_async(async move {
        let mut files = Vec::new();
        let mut dirs = vec![path];
        while let Some(dir) = dirs.pop() {
            let mut entries = tokio::fs::read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ["zip", "json", "pez"].contains(&ext.to_ascii_lowercase().as_str()) {
                        files.push(path.to_str().context("Invalid path")?.to_owned());
                    }
                }
            }
        }
        Ok(files)
    })
        .await
}

pub fn create_default_render_config() -> RenderConfig {
    RenderConfig::default()
}

async fn prepare_platform_assets(app: tauri::AppHandle) -> Result<(), InvokeError> {
    #[cfg(target_os = "android")]
    {
        let path = tokio::task::spawn_blocking(move || {
            app.state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>().assets()
                .map_err(|error| InvokeError::from(error.to_string()))
        }).await.map_err(|error| InvokeError::from(error.to_string()))??;
        ASSET_PATH.set(PathBuf::from(path)).ok();
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    Ok(())
}

#[tauri::command]
fn open_respack_folder() -> Result<(), InvokeError> {
    (|| {
        open::that_detached(respack_dir()?)?;
        Ok(())
    })()
    .map_err(InvokeError::from_anyhow)
}

fn get_presets_file() -> Result<PathBuf> {
    let file = CONFIG_DIR.get().unwrap().join("presets.json");
    if file.exists() && !file.is_file() {
        bail!("presets.json is not a file");
    }
    Ok(file)
}

#[tauri::command]
async fn get_presets() -> Result<HashMap<String, RenderConfig>, InvokeError> {
    (|| {
        let file = get_presets_file()?;
        Ok(if !file.exists() {
            HashMap::new()
        } else {
            serde_json::from_reader(BufReader::new(File::open(file)?))?
        })
    })()
    .map_err(InvokeError::from_anyhow)
}

async fn save_presets(presets: &HashMap<String, RenderConfig>) -> Result<()> {
    serde_json::to_writer(BufWriter::new(File::create(get_presets_file()?)?), presets)?;
    Ok(())
}

#[tauri::command]
async fn add_preset(name: String, config: RenderConfig) -> Result<(), InvokeError> {
    let mut presets = get_presets().await?;
    wrap_async(async move {
        if presets.insert(name, config).is_some() {
            bail!(mtl!("preset-exists"));
        }
        save_presets(&presets).await?;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn remove_preset(name: String) -> Result<(), InvokeError> {
    let mut presets = get_presets().await?;
    wrap_async(async move {
        if presets.remove(&name).is_none() {
            bail!(mtl!("preset-not-found"));
        }
        save_presets(&presets).await?;
        Ok(())
    })
    .await
}

fn rpe_dir() -> Result<Option<PathBuf>> {
    let file = CONFIG_DIR.get().unwrap().join("rpe_path.txt");
    if file.exists() {
        if !file.is_file() {
            bail!("rpe_path.txt is not a file");
        }
    } else {
        return Ok(None);
    }
    let dir: PathBuf = std::fs::read_to_string(file)?.into();
    Ok(if dir.exists() { Some(dir) } else { None })
}

#[derive(Serialize)]
pub struct RPEChartInfo {
    name: String,
    id: String,
    path: String,
    illustration: String,
    charter: String,
    #[serde(skip)]
    modified: SystemTime,
}

#[tauri::command]
fn set_rpe_dir(path: PathBuf) -> Result<(), InvokeError> {
    (|| {
        if !path.is_dir()
            || ["PhiEdit.exe", "Resources"]
                .iter()
                .any(|it| !path.join(*it).exists())
        {
            bail!(mtl!("not-valid-rpe"));
        }
        std::fs::write(
            CONFIG_DIR.get().unwrap().join("rpe_path.txt"),
            path.canonicalize()?.display().to_string().as_bytes(),
        )?;
        Ok(())
    })()
    .map_err(InvokeError::from_anyhow)
}

#[tauri::command]
fn unset_rpe_dir() -> Result<(), InvokeError> {
    (|| {
        std::fs::remove_file(CONFIG_DIR.get().unwrap().join("rpe_path.txt"))?;
        Ok(())
    })()
    .map_err(InvokeError::from_anyhow)
}

#[tauri::command]
fn get_rpe_charts() -> Result<Option<Vec<RPEChartInfo>>, InvokeError> {
    (|| {
        let Some(dir) = rpe_dir()? else {
            return Ok(None);
        };
        let mut results = Vec::new();
        let mut name = None;
        let mut id = None;
        let mut chart = None;
        let mut illustration = None;
        let mut charter = None;
        macro_rules! commit {
            () => {
                (|| {
                    let id = id.take()?;
                    let path = dir.join("Resources").join(&id);
                    let metadata = path.join(chart.take()?).metadata();
                    results.push(RPEChartInfo {
                        name: name.take()?,
                        id,
                        path: path.display().to_string(),
                        illustration: path.join(illustration.take()?).display().to_string(),
                        charter: charter.take()?,
                        modified: metadata
                            .and_then(|it| it.modified())
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                    });
                    Some(())
                })()
            };
        }
        if dir.join("Chartlist.txt").exists() {
            for line in BufReader::new(File::open(dir.join("Chartlist.txt"))?).lines() {
                let line = line?;
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "#" {
                    commit!();
                    continue;
                }
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                *(match key {
                    "Name" => &mut name,
                    "Path" => &mut id,
                    "Chart" => &mut chart,
                    "Picture" => &mut illustration,
                    "Charter" => &mut charter,
                    _ => continue,
                }) = Some(value.trim().to_owned());
            }
            commit!();
        } else {
            use regex::Regex;
            let onely_num = Regex::new(r"^\d+$").unwrap();
            let mut folders = Vec::new();
            let path = dir.join("Resources");
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    if let Some(folder_name) = path.file_name() {
                        if onely_num.is_match(folder_name.to_str().unwrap_or("")) {
                            folders.push(path);
                        }
                    }
                }
            }
            for folder in folders {
                log::debug!("Found numeric folder: {:?}", folder);
                for line in BufReader::new(File::open(folder.join("info.txt"))?).lines() {
                    let line = line?;
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "#" {
                        commit!();
                        continue;
                    }
                    let Some((key, value)) = line.split_once(':') else {
                        continue;
                    };
                    *(match key {
                        "Name" => &mut name,
                        "Path" => &mut id,
                        "Chart" => &mut chart,
                        "Picture" => &mut illustration,
                        "Charter" => &mut charter,
                        _ => continue,
                    }) = Some(value.trim().to_owned());
                }
                commit!();
            }
        }

        results.sort_by_key(|it| it.modified);
        results.reverse();

        Ok(Some(results))
    })()
    .map_err(InvokeError::from_anyhow)
}

#[tauri::command]
fn test_ffmpeg() -> Result<bool, InvokeError> {
    (|| Ok(find_ffmpeg()?.is_some()))().map_err(InvokeError::from_anyhow)
}

#[tauri::command]
fn open_app_folder() -> Result<(), InvokeError> {
    (|| {
        open::that_detached(std::env::current_exe()?.parent().unwrap())?;
        Ok(())
    })()
    .map_err(InvokeError::from_anyhow)
}

#[tauri::command]
fn save_text_file(path: PathBuf, content: String) -> Result<(), InvokeError> {
    std::fs::write(path, content).map_err(|e| InvokeError::from_anyhow(e.into()))
}

#[tauri::command]
fn save_binary_file(path: PathBuf, data: Vec<u8>) -> Result<(), InvokeError> {
    std::fs::write(path, data).map_err(|e| InvokeError::from_anyhow(e.into()))
}

/// Copy a chart's illustration into the app cache and return the cached path,
/// so the webview can display covers that live inside a `.pez`/`.zip`.
#[tauri::command]
async fn load_chart_illustration(
    app: tauri::AppHandle,
    path: PathBuf,
    illustration: String,
) -> Result<String, InvokeError> {
    wrap_async(async move {
        use std::hash::{Hash, Hasher};
        let path = localize_chart_path(&path)?;
        let mut fs: Box<dyn FileSystem + Send + Sync + 'static> =
            fs::fs_from_file(&path).with_context(|| mtl!("read-chart-failed"))?;
        let data = fs
            .load_file(&illustration)
            .await
            .with_context(|| format!("Failed to load chart illustration: {illustration}"))?;

        let cache_dir = ensure_dir(app.path().app_cache_dir()?.join("chart-covers"));
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        illustration.hash(&mut hasher);
        let extension = Path::new(&illustration)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| value.chars().all(|ch| ch.is_ascii_alphanumeric()))
            .unwrap_or("img");
        let cover_path = cache_dir.join(format!("{:016x}.{extension}", hasher.finish()));
        tokio::fs::write(&cover_path, data).await?;
        Ok(cover_path.display().to_string())
    })
    .await
}

/// Probe a remote `phi-tkc server` from the app.
///
/// Done in Rust rather than the WebView so it is not subject to CORS, the
/// `http:default` capability scope, or Android's cleartext-HTTP block.
#[tauri::command]
async fn remote_status(base: String) -> Result<serde_json::Value, InvokeError> {
    wrap_async(async move {
        let url = format!("{}/api/status", base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()?;
        let response = client.get(&url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("HTTP {}", response.status());
        }
        Ok(response.json::<serde_json::Value>().await?)
    })
    .await
}

#[tauri::command]
async fn start_render_server(host: String, port: u16) -> Result<server::ServerStatus, InvokeError> {
    wrap_async(server::start_managed(host, port)).await
}

#[tauri::command]
fn stop_render_server() -> bool {
    server::stop_managed()
}

#[tauri::command]
fn render_server_status() -> server::ServerStatus {
    server::managed_status()
}

/// `GET /api/jobs` on the remote renderer (polling fallback when the WebSocket
/// cannot be established, e.g. a WebView that blocks cleartext `ws://`).
#[tauri::command]
async fn remote_jobs(base: String) -> Result<serde_json::Value, InvokeError> {
    wrap_async(async move {
        let url = format!("{}/api/jobs", base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()?;
        let response = client.get(&url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("HTTP {}", response.status());
        }
        Ok(response.json::<serde_json::Value>().await?)
    })
    .await
}

fn multipart_text(boundary: &str, name: &str, value: &str, body: &mut Vec<u8>) {
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
            .as_bytes(),
    );
}

fn multipart_file(boundary: &str, name: &str, file_name: &str, data: &[u8], body: &mut Vec<u8>) {
    let file_name = file_name.replace(['"', '\r', '\n'], "_");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");
}

/// Submit a render to a `phi-tkc server` instance.
///
/// Done in Rust (not the WebView) for three reasons: the chart file lives on the
/// native side, Android blocks cleartext HTTP from the WebView, and sending the
/// whole `RenderConfig` as JSON keeps every phone-side option intact instead of
/// re-encoding a subset as query fields.
#[tauri::command]
async fn remote_post_render(base: String, params: RenderParams) -> Result<serde_json::Value, InvokeError> {
    wrap_async(remote_render_upload(base, params)).await
}

/// The actual multipart upload behind [`remote_post_render`], split out so it can
/// be exercised without a running Tauri app.
async fn remote_render_upload(base: String, params: RenderParams) -> Result<serde_json::Value> {
    {
        let base = base.trim_end_matches('/').to_string();
        let boundary = format!("----PhiTKC{:016x}", {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
                .hash(&mut hasher);
            hasher.finish()
        });

        let mut body = Vec::new();
        // `path` stays local: the server reads the uploaded copy.
        let posted = serde_json::json!({ "info": params.info, "config": params.config });
        multipart_text(&boundary, "params", &serde_json::to_string(&posted)?, &mut body);

        let path = localize_chart_path(&params.path)?;
        let (chart_name, chart) = tokio::task::spawn_blocking(move || chart_archive::read_upload(&path)).await??;
        multipart_file(&boundary, "file", &chart_name, &chart, &mut body);

        for (field, path) in [
            ("respack", params.config.res_pack_path.as_ref()),
            ("avatar", params.config.player_avatar.as_ref()),
        ] {
            let Some(path) = path else { continue };
            let path = Path::new(path);
            let Ok(data) = tokio::fs::read(path).await else { continue };
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or(field);
            multipart_file(&boundary, field, name, &data, &mut body);
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        let response = client
            .post(format!("{base}/api/render"))
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {text}");
        }
        Ok(serde_json::from_str::<serde_json::Value>(&text)?)
    }
}

#[tauri::command]
async fn remote_cancel(base: String, id: String) -> Result<(), InvokeError> {
    wrap_async(remote_cancel_request(base, id)).await
}

async fn remote_cancel_request(base: String, id: String) -> Result<()> {
        let url = format!("{}/api/jobs/{id}/cancel", base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()?;
        let response = client.post(&url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await?;
            anyhow::bail!("HTTP {status}: {detail}");
        }
        // The server returns 204 No Content when cancellation is accepted.
        Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end smoke test for the phone → server upload path.
    ///
    /// Ignored by default because it needs a real `phi-tkc server` on 127.0.0.1:6787
    /// and the test chart. Run with:
    ///   cargo test --offline remote_render_upload -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn remote_render_upload_smoke() {
        let Ok(chart) = std::env::var("PHI_TKC_TEST_CHART") else {
            eprintln!("PHI_TKC_TEST_CHART is not set; skipping");
            return;
        };
        let chart = std::path::Path::new(&chart);
        assert!(chart.exists(), "test chart is missing");

        let mut config = RenderConfig::default();
        config.resolution = (320, 180);
        config.fps = 10;
        config.bitrate = "45".to_owned();
        let mut info = ChartInfo::default();
        info.name = "remote-upload-test".to_owned();

        let params = RenderParams {
            path: chart.to_path_buf(),
            info,
            config,
        };
        let response = remote_render_upload("http://127.0.0.1:6787".to_owned(), params)
            .await
            .expect("upload failed");
        let id = response.get("id").and_then(|id| id.as_str()).expect("no job id");
        println!("submitted job {id}");

        // Wait for the job to finish and make sure it actually rendered.
        let client = reqwest::Client::new();
        for _ in 0..120 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let jobs: serde_json::Value = client
                .get("http://127.0.0.1:6787/api/jobs")
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let job = jobs
                .as_array()
                .unwrap()
                .iter()
                .find(|job| job.get("id").and_then(|id| id.as_str()) == Some(id))
                .expect("job disappeared");
            match job.get("status").and_then(|s| s.as_str()).unwrap_or("") {
                "done" => {
                    println!("done: {}", job.get("duration").unwrap_or(&serde_json::Value::Null));
                    return;
                }
                "failed" | "canceled" => panic!("job ended as {}", job["status"]),
                _ => {}
            }
        }
        panic!("job did not finish in time");
    }
}

use crate::{
    common::output_dir,
    render::{IPCEvent, RenderParams},
    ASSET_PATH,
};
use anyhow::Result;
use chrono::Local;
use prpr::fs;
use serde::Serialize;
use std::{
    io::Write,
    ops::DerefMut,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, Mutex},
    task::JoinHandle,
};
use tracing::{error, info};

#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TaskStatus {
    Pending,
    Loading,
    Mixing,
    Rendering {
        progress: f64,
        fps: u64,
        estimate: f64,
    },
    Done {
        duration: f64,
        output: String,
    },
    Canceled,
    Failed {
        error: String,
    },
}

pub struct Task {
    id: u32,
    name: String,
    cover: NamedTempFile,
    output: PathBuf,

    params: RenderParams,
    status: Mutex<TaskStatus>,
    request_cancel: AtomicBool,
    last_fps_update: Mutex<Instant>,
}

impl Task {
    async fn new(id: u32, params: RenderParams, output_path: Option<PathBuf>) -> Result<Self> {
        let mut fs = fs::fs_from_file(&params.path)?;
        let info = fs::load_info(fs.deref_mut()).await?;
        let mut cover = NamedTempFile::new()?;
        let video = match params.config.video {
            true => "mov",
            false => "mp4",
        };
        cover.write_all(&fs.load_file(&info.illustration).await?)?;

        let level: String = info
            .level
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        let safe_name: String = info
            .name
            .chars()
            .filter(|&it| it == '-' || it == '_' || it == ' ' || it.is_alphanumeric())
            .collect();
        let file_name = format!(
            "{} {safe_name}_{level}-{id}.{video}",
            Local::now().format("%Y-%m-%d %H-%M-%S%.3f")
        );

        let output = if let Some(path) = output_path {
            // The custom output directory comes from a free-text setting and may not
            // exist yet; create it instead of letting ffmpeg fail with a vague error.
            if !path.exists() {
                std::fs::create_dir_all(&path)?;
            } else if !path.is_dir() {
                anyhow::bail!("output path is not a directory: {}", path.display());
            }
            path.join(file_name)
        } else {
            output_dir()?.join(file_name)
        };

        Ok(Self {
            id,
            name: info.name,
            cover,
            output,

            params,
            status: Mutex::new(TaskStatus::Pending),
            request_cancel: AtomicBool::default(),
            last_fps_update: Mutex::new(Instant::now()),
        })
    }

    /// Android has no `fork`/`exec`, so the renderer runs in-process with the
    /// NDK MediaCodec/MediaMuxer pipeline (no ffmpeg). Progress is polled from
    /// atomic counters that the render thread updates.
    #[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
    async fn run_mobile(&self) -> Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
        #[cfg(target_os = "android")]
        use crate::android_render as native_render;
        #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
        use crate::apple_render as native_render;

        if self.request_cancel.load(Ordering::Relaxed) {
            *self.status.lock().await = TaskStatus::Canceled;
            return Ok(());
        }
        *self.status.lock().await = TaskStatus::Loading;

        let progress = Arc::new(AtomicU64::new(0));
        let current_fps = Arc::new(AtomicU64::new(0));
        let progress_out = Arc::clone(&progress);
        let fps_out = Arc::clone(&current_fps);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_out = Arc::clone(&cancel);
        let request = native_render::RenderRequest {
            params: self.params.clone(),
            output_path: self.output.clone(),
            asset_dir: ASSET_PATH
                .get()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("assets")),
            request_cancel: Some(cancel_out),
        };

        *self.status.lock().await = TaskStatus::Mixing;
        let finished = Arc::new(std::sync::Mutex::new(None::<Result<()>>));
        let finished_out = Arc::clone(&finished);
        let runtime = tokio::runtime::Handle::current();
        std::thread::Builder::new().name("native-render".into()).spawn(move || {
            let _runtime = runtime.enter();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native_render::run(request, move |value, fps| {
                progress_out.store((value.max(0.) * 10000.) as u64, AtomicOrdering::Relaxed);
                fps_out.store(fps.max(0.) as u64, AtomicOrdering::Relaxed);
            }))).unwrap_or_else(|panic| {
                let message = panic.downcast_ref::<String>().map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("unknown panic");
                Err(anyhow::anyhow!("Native renderer panicked: {message}"))
            });
            if let Ok(mut guard) = finished_out.lock() {
                *guard = Some(result);
            }
        })?;

        let start = Instant::now();
        loop {
            if self.request_cancel.load(Ordering::Relaxed) {
                cancel.store(true, Ordering::Relaxed);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            let value = progress.load(AtomicOrdering::Relaxed) as f64 / 10000.;
            let fps = current_fps.load(AtomicOrdering::Relaxed);
            let done = finished.lock().ok().and_then(|mut guard| guard.take());
            let elapsed = start.elapsed().as_secs_f64();
            let estimate = if value > 0.01 {
                elapsed / value - elapsed
            } else {
                0.0
            };
            match done {
                Some(Ok(())) => {
                    if self.request_cancel.load(Ordering::Relaxed) {
                        let _ = tokio::fs::remove_file(&self.output).await;
                        *self.status.lock().await = TaskStatus::Canceled;
                        return Ok(());
                    }
                    let log = tokio::fs::read_to_string(self.output.with_extension("log"))
                        .await
                        .unwrap_or_default();
                    let name = self
                        .output
                        .file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "output.mp4".to_string());
                    let source = self.output.clone();
                    let published = tokio::task::spawn_blocking(move || {
                        #[cfg(target_os = "android")]
                        { crate::android_uri::publish_video(&source, &name) }
                        #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
                        { let _ = name; Ok(source.to_string_lossy().into_owned()) }
                    })
                    .await
                    .unwrap_or_else(|error| Err(anyhow::anyhow!("publish task failed: {error}")));
                    let target = match published {
                        Ok(target) => target,
                        Err(error) => {
                            *self.status.lock().await = TaskStatus::Failed {
                                error: format!("无法保存视频：{error:#}\n\n{log}"),
                            };
                            return Ok(());
                        }
                    };
                    // Scoped storage may retain the private output. Never delete
                    // the only copy after the publisher returns the source path.
                    if PathBuf::from(&target) != self.output {
                        let _ = tokio::fs::remove_file(&self.output).await;
                    }
                    *self.status.lock().await = TaskStatus::Done {
                        duration: elapsed,
                        output: format!("{log}\n\nSaved to: {target}"),
                    };
                    return Ok(());
                }
                Some(Err(error)) => {
                    if self.request_cancel.load(Ordering::Relaxed) {
                        let _ = tokio::fs::remove_file(&self.output).await;
                        *self.status.lock().await = TaskStatus::Canceled;
                        return Ok(());
                    }
                    let log = tokio::fs::read_to_string(self.output.with_extension("log"))
                        .await
                        .unwrap_or_default();
                    *self.status.lock().await = TaskStatus::Failed {
                        error: format!("{error:#}\n\n{log}"),
                    };
                    return Ok(());
                }
                None => {
                    *self.status.lock().await = if value == 0. { TaskStatus::Mixing } else { TaskStatus::Rendering {
                        progress: value,
                        fps,
                        estimate,
                    } };
                }
            }
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Task #{} started ({})", self.id, self.params.path.display());

        // Android cannot spawn itself plus ffmpeg, so it renders in-process.
        #[cfg(any(target_os = "android", target_os = "ios", feature = "apple-render-check"))]
        return self.run_mobile().await;

        // A task canceled while it was still queued must not spawn a renderer at all.
        if self.request_cancel.load(Ordering::Relaxed) {
            *self.status.lock().await = TaskStatus::Canceled;
            return Ok(());
        }

        *self.status.lock().await = TaskStatus::Loading;

        let mut child = tokio::process::Command::new(std::env::current_exe()?)
            .arg("render")
            .env("PHI_TKC_IPC", "1")
            .arg(ASSET_PATH.get().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        stdin
            .write_all(format!("{}\n", serde_json::to_string(&self.params)?).as_bytes())
            .await?;
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&self.output)?).as_bytes())
            .await?;
        stdin.flush().await?;

        let mut lines = BufReader::new(stdout).lines();
        // Non-JSON lines on stdout are the renderer's log records (they go through
        // `log`, which `init_colored_logger` sends to stdout). Collect them so the
        // task's output dialog shows the render log instead of dropping it.
        let mut child_log = String::new();
        let mut total = 0;
        let mut frame_count: u64 = 0;
        let start = Instant::now();
        // A task can sit in the queue for minutes; measure FPS from when it actually
        // starts, and seed the estimate with the requested frame rate.
        *self.last_fps_update.lock().await = Instant::now();
        let mut last_fps: usize = self.params.config.fps as usize;
        loop {
            // If the renderer stalls (or emits no lines), a plain `next_line().await`
            // would make cancellation impossible. Wake up periodically to check the flag.
            let line = match tokio::time::timeout(Duration::from_millis(250), lines.next_line()).await {
                Ok(line) => line?,
                Err(_) => {
                    if self.request_cancel.load(Ordering::Relaxed) {
                        let _ = child.kill().await;
                        *self.status.lock().await = TaskStatus::Canceled;
                        return Ok(());
                    }
                    continue;
                }
            };
            let Some(line) = line else { break };
            let Ok(event): Result<IPCEvent, _> = serde_json::from_str(line.trim()) else {
                if child_log.len() < (1 << 20) {
                    child_log.push_str(&line);
                    child_log.push('\n');
                }
                continue;
            };
            match event {
                IPCEvent::StartMixing => {
                    *self.status.lock().await = TaskStatus::Mixing;
                }
                IPCEvent::StartRender(total_frame) => {
                    *self.status.lock().await = TaskStatus::Rendering {
                        progress: 0.,
                        fps: 0,
                        estimate: 0.,
                    };
                    total = total_frame;
                }
                IPCEvent::Frame => {
                    frame_count += 1;
                    // Re-measure every 10 frames instead of once a second, so the
                    // displayed FPS and ETA react to speed changes much sooner.
                    if frame_count % 10 == 0 {
                        let now = Instant::now();
                        let mut last_update = self.last_fps_update.lock().await;
                        let elapsed = now.duration_since(*last_update).as_secs_f64();
                        if elapsed > 0.0 {
                            last_fps = (10.0 / elapsed) as usize;
                        }
                        *last_update = now;
                    }

                    let cur = start.elapsed().as_secs_f64();
                    let estimate =
                        total.saturating_sub(frame_count).max(1) as f64 / last_fps.max(1) as f64;
                    if total > 0 && frame_count >= total {
                        let output = child.wait_with_output().await?;
                        let stdout = String::from_utf8(output.stdout)
                            .unwrap_or_else(|_| "Invalid output".to_owned());
                        let stderr = String::from_utf8(output.stderr)
                            .unwrap_or_else(|_| "Invalid output".to_owned());
                        *self.status.lock().await = TaskStatus::Done {
                            duration: cur,
                            output: format!("[STDOUT]\n{stdout}\n\n[STDERR]\n{stderr}"),
                        };
                        return Ok(());
                    } else {
                        *self.status.lock().await = TaskStatus::Rendering {
                            progress: if total > 0 { frame_count as f64 / total as f64 } else { 0. },
                            fps: last_fps as u64,
                            estimate,
                        };
                    }
                }
                IPCEvent::EncoderFps(fps) => {
                    // Encoder-side FPS (frames actually written to the pipe); it is a
                    // better estimate than the render loop's frame counter when the
                    // encoder is the bottleneck.
                    let mut status = self.status.lock().await;
                    let next = match &*status {
                        TaskStatus::Rendering { progress, estimate, .. } => Some(TaskStatus::Rendering {
                            progress: *progress,
                            fps: fps.max(0.0) as u64,
                            estimate: *estimate,
                        }),
                        _ => None,
                    };
                    if let Some(next) = next {
                        *status = next;
                    }
                }
                IPCEvent::Done(duration) => {
                    let output = child.wait_with_output().await?;
                    let stdout = String::from_utf8(output.stdout)
                        .unwrap_or_else(|_| "Invalid output".to_owned());
                    let stderr = String::from_utf8(output.stderr)
                        .unwrap_or_else(|_| "Invalid output".to_owned());
                    *self.status.lock().await = TaskStatus::Done {
                        duration,
                        output: format!("[LOG]\n{child_log}\n\n[STDOUT]\n{stdout}\n\n[STDERR]\n{stderr}"),
                    };
                    return Ok(());
                }
            }
            if self.request_cancel.load(Ordering::Relaxed) {
                let _ = child.kill().await;
                *self.status.lock().await = TaskStatus::Canceled;
                return Ok(());
            }
        }

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            *self.status.lock().await = TaskStatus::Failed {
                error: format!(
                    "Child process exited abnormally ({:?})\n\n[LOG]\n{}\n\n[STDERR]\n{}",
                    output.status.code(),
                    child_log,
                    String::from_utf8_lossy(&output.stderr)
                ),
            };
            return Ok(());
        }

        // A successful exit without an IPCEvent::Done leaves the task in Loading/
        // Rendering forever, so the UI would show a render that never finishes.
        let mut status = self.status.lock().await;
        if !matches!(&*status, TaskStatus::Done { .. } | TaskStatus::Canceled | TaskStatus::Failed { .. }) {
            *status = TaskStatus::Failed {
                error: "The renderer exited without reporting completion".to_owned(),
            };
        }

        Ok(())
    }

    pub fn cancel(&self) {
        self.request_cancel.store(true, Ordering::Relaxed);
    }

    pub async fn to_view(&self) -> TaskView {
        TaskView {
            id: self.id,
            name: self.name.clone(),
            output: self.output.clone(),
            path: self.params.path.display().to_string(),
            cover: self.cover.path().display().to_string(),
            status: self.status.lock().await.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct TaskView {
    id: u32,
    name: String,
    output: PathBuf,
    path: String,
    cover: String,
    status: TaskStatus,
}

pub struct TaskQueue {
    sender: mpsc::UnboundedSender<Arc<Task>>,
    worker: JoinHandle<()>,

    tasks: Mutex<Vec<Arc<Task>>>,
}
impl TaskQueue {
    pub fn new() -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Arc<Task>>();
        let task = tokio::spawn(async move {
            while let Some(task) = receiver.recv().await {
                if let Err(err) = task.run().await {
                    error!("Failed to render: {err:?}");
                    *task.status.lock().await = TaskStatus::Failed {
                        error: format!("{err:?}"),
                    };
                }
            }
        });

        Self {
            sender,
            worker: task,

            tasks: Mutex::default(),
        }
    }

    pub async fn post(&self, params: RenderParams, output_path: Option<PathBuf>) -> Result<u32> {
        let mut guard = self.tasks.lock().await;
        let id = guard.len() as u32;
        let task = Arc::new(Task::new(id, params, output_path).await?);
        guard.push(Arc::clone(&task));
        self.sender.send(task)?;

        Ok(id)
    }

    pub async fn tasks(&self) -> Vec<TaskView> {
        let guard = self.tasks.lock().await;
        let mut result = Vec::with_capacity(guard.capacity());
        for task in guard.iter() {
            result.push(task.to_view().await);
        }
        result.reverse();
        result
    }

    pub async fn cancel(&self, id: u32) {
        if let Some(task) = self.tasks.lock().await.get(id as usize) {
            task.cancel();
        }
    }
}

impl Drop for TaskQueue {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

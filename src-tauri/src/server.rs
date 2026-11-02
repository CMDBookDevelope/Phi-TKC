//! Phone-facing render server: `phi-tkc server --host 0.0.0.0 --port 6787`.
//!
//! Runs an HTTP API plus a small WebUI (`/` and `/render`) so an Android/iOS device
//! on the same network can upload a chart and let this machine render it. The server
//! process itself never touches the GPU: every job is handed to the existing
//! `current_exe render` child pipeline (the same one the GUI task queue uses), which
//! is what owns the Vulkan context.

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Multipart, Path as UrlPath, State,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use prpr::fs;
use serde::Serialize;
use std::{
    collections::HashMap,
    ops::DerefMut,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{broadcast, Mutex},
};
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

use crate::render::{IPCEvent, RenderConfig, RenderParams};

const WEBUI: &str = include_str!("server_webui.html");
const MAX_UPLOAD: usize = 256 * 1024 * 1024;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "phi-tkc server",
    about = "Serve the phone render WebUI + JSON API",
    long_about = "Starts an HTTP server so Android/iOS clients can upload charts and \
                  let this machine render them.\n\n\
                  The default host only accepts local connections; pass \
                  `--host 0.0.0.0` to reach it from a phone on the same network."
)]
pub struct ServerArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 6787)]
    pub port: u16,
    /// Assets folder; auto-detected (exe dir, /usr/lib/ptkc-assets) when empty.
    #[arg(long, default_value = "")]
    pub assets: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobView {
    id: String,
    name: String,
    status: &'static str,
    progress: f64,
    fps: u64,
    eta: f64,
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<String>,
    created_at: u64,
}

struct Job {
    view: JobView,
    input: PathBuf,
    output: PathBuf,
    config: RenderConfig,
    cancel: Arc<AtomicBool>,
}

type Jobs = Arc<Mutex<HashMap<String, Job>>>;

#[derive(Clone)]
struct AppState {
    jobs: Jobs,
    /// Fan-out for the WebSocket clients (mobile app shows live progress).
    events: broadcast::Sender<String>,
    assets: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
}

struct Managed {
    host: String,
    port: u16,
    stop: tokio::sync::watch::Sender<bool>,
}

static MANAGED: std::sync::OnceLock<std::sync::Mutex<Option<Managed>>> =
    std::sync::OnceLock::new();

fn managed() -> &'static std::sync::Mutex<Option<Managed>> {
    MANAGED.get_or_init(|| std::sync::Mutex::new(None))
}

/// Start the server from the GUI and remember the handle so Settings can stop it.
///
/// Binds before spawning, so a busy port is reported to the caller instead of
/// silently leaving a broken "running" state in the UI.
pub async fn start_managed(host: String, port: u16) -> Result<ServerStatus> {
    if managed().lock().unwrap().is_some() {
        anyhow::bail!("server is already running");
    }
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .with_context(|| format!("cannot bind {host}:{port}"))?;
    let bound_port = listener.local_addr()?.port();

    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let app = build_app(resolve_assets(""));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stop_rx.changed().await;
            })
            .await;
    });

    *managed().lock().unwrap() = Some(Managed {
        host: host.clone(),
        port: bound_port,
        stop: stop_tx,
    });
    Ok(ServerStatus {
        running: true,
        host,
        port: bound_port,
    })
}

/// Stop a server started with [`start_managed`]; returns whether one was running.
pub fn stop_managed() -> bool {
    match managed().lock().unwrap().take() {
        Some(managed) => {
            let _ = managed.stop.send(true);
            true
        }
        None => false,
    }
}

pub fn managed_status() -> ServerStatus {
    match managed().lock().unwrap().as_ref() {
        Some(managed) => ServerStatus {
            running: true,
            host: managed.host.clone(),
            port: managed.port,
        },
        None => ServerStatus {
            running: false,
            host: "127.0.0.1".to_string(),
            port: 6787,
        },
    }
}

/// CLI entry: serve until the process is terminated.
pub async fn run(args: ServerArgs) -> Result<()> {
    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("cannot bind {addr}"))?;

    println!("Phi-TKC render server listening on http://{addr}");
    println!("  WebUI: http://{addr}/");
    println!("  WebSocket: ws://{addr}/render");
    if args.host == "127.0.0.1" || args.host == "localhost" {
        println!("  Only local connections are accepted; use `--host 0.0.0.0` for phones.");
    }

    let app = build_app(resolve_assets(&args.assets));
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_app(assets: PathBuf) -> Router {
    let (events, _) = broadcast::channel::<String>(32);
    let state = AppState {
        jobs: Arc::new(Mutex::new(HashMap::new())),
        events: events.clone(),
        assets,
    };

    // Push a full snapshot twice a second; clients do not need per-frame events and
    // a snapshot is idempotent if a client reconnects mid-render.
    let ticker = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            if ticker.events.receiver_count() == 0 {
                continue;
            }
            let mut jobs: Vec<JobView> = {
                let guard = ticker.jobs.lock().await;
                guard.values().map(|job| job.view.clone()).collect()
            };
            jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
            let payload = serde_json::json!({ "type": "jobs", "jobs": jobs }).to_string();
            let _ = ticker.events.send(payload);
        }
    });

    Router::new()
        // `/` is the human-facing WebUI; `/render` is the WebSocket endpoint the
        // mobile app connects to. Keep them distinct — do not alias one to the other.
        .route("/", get(webui))
        .route("/api/status", get(status))
        .route("/api/jobs", get(list_jobs))
        .route("/api/render", post(upload))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/download", get(download))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/render", get(ws))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn resolve_assets(arg: &str) -> PathBuf {
    let exe_assets = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")));
    let candidates = [
        PathBuf::from(arg),
        exe_assets.unwrap_or_default(),
        PathBuf::from("/usr/lib/ptkc-assets"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
    ];
    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(arg))
}

async fn webui() -> Html<&'static str> {
    Html(WEBUI)
}

/// Live job snapshots for the mobile app (`ws://<host>/render`).
async fn ws(upgrade: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| ws_loop(socket, st.events.subscribe()))
}

async fn ws_loop(mut socket: WebSocket, mut events: broadcast::Receiver<String>) {
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(text) => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            },
        }
    }
}

async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    let jobs = st.jobs.lock().await;
    let running = jobs.values().any(|job| job.view.status == "running");
    let queued = jobs.values().filter(|job| job.view.status == "queued").count();
    Json(serde_json::json!({
        "name": "Phi-TKC",
        "version": env!("CARGO_PKG_VERSION"),
        "busy": running,
        "queued": queued,
        "jobs": jobs.len(),
    }))
}

async fn list_jobs(State(st): State<AppState>) -> Json<Vec<JobView>> {
    let jobs = st.jobs.lock().await;
    let mut out: Vec<_> = jobs.values().map(|job| job.view.clone()).collect();
    out.sort_by_key(|job| std::cmp::Reverse(job.created_at));
    Json(out)
}

async fn get_job(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let jobs = st.jobs.lock().await;
    match jobs.get(&id) {
        Some(job) => Json(job.view.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "job not found").into_response(),
    }
}

async fn cancel(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let jobs = st.jobs.lock().await;
    match jobs.get(&id) {
        Some(job) => {
            job.cancel.store(true, Ordering::Relaxed);
            StatusCode::NO_CONTENT.into_response()
        }
        None => (StatusCode::NOT_FOUND, "job not found").into_response(),
    }
}

async fn download(State(st): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let output = {
        let jobs = st.jobs.lock().await;
        match jobs.get(&id) {
            Some(job) if job.view.status == "done" => job.output.clone(),
            _ => return (StatusCode::NOT_FOUND, "job not ready").into_response(),
        }
    };
    let Ok(file) = tokio::fs::File::open(&output).await else {
        return (StatusCode::NOT_FOUND, "output missing").into_response();
    };
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output.mp4");
    let stream = ReaderStream::new(file);
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        )
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn upload(State(st): State<AppState>, mut multipart: Multipart) -> Response {
    let id = next_id();
    let dir = jobs_dir().join(&id);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response();
    }

    let mut input: Option<PathBuf> = None;
    let mut fields: HashMap<String, String> = HashMap::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => return (StatusCode::BAD_REQUEST, format!("bad multipart: {e}")).into_response(),
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let original = field.file_name().unwrap_or("chart.pez").to_string();
            let ext = Path::new(&original)
                .extension()
                .and_then(|ext| ext.to_str())
                .filter(|ext| ext.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("pez");
            let path = dir.join(format!("input.{ext}"));
            let data = match field.bytes().await {
                Ok(data) => data,
                Err(e) => return (StatusCode::BAD_REQUEST, format!("upload failed: {e}")).into_response(),
            };
            if data.len() > MAX_UPLOAD {
                return (StatusCode::PAYLOAD_TOO_LARGE, "chart archive is too large").into_response();
            }
            if let Err(e) = tokio::fs::write(&path, &data).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response();
            }
            input = Some(path);
        } else {
            if let Ok(text) = field.text().await {
                fields.insert(name, text);
            }
        }
    }

    let Some(input) = input else {
        return (StatusCode::BAD_REQUEST, "missing `file` part").into_response();
    };

    let mut fs = match fs::fs_from_file(&input) {
        Ok(fs) => fs,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("cannot open chart: {e}")).into_response(),
    };
    let info = match fs::load_info(fs.deref_mut()).await {
        Ok(info) => info,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("cannot read chart info: {e}")).into_response(),
    };

    let video = fields.get("video").map(|v| v == "mov").unwrap_or(false);
    let ext = if video { "mov" } else { "mp4" };
    let safe_name: String = info
        .name
        .chars()
        .filter(|c| *c == '-' || *c == '_' || *c == ' ' || c.is_alphanumeric())
        .collect();
    let output = dir.join(format!("{}.{ext}", if safe_name.is_empty() { "render" } else { &safe_name }));

    let config = build_config(&fields);
    let job = Job {
        view: JobView {
            id: id.clone(),
            name: info.name.clone(),
            status: "queued",
            progress: 0.0,
            fps: 0,
            eta: 0.0,
            error: None,
            download: None,
            created_at: now_secs(),
        },
        input,
        output,
        config,
        cancel: Arc::new(AtomicBool::new(false)),
    };
    st.jobs.lock().await.insert(id.clone(), job);

    // One process per job: there is no in-server queue. A client that wants N
    // parallel renders submits N jobs and gets N `Phi-TKC render` processes.
    let job_state = st.clone();
    let job_id = id.clone();
    tokio::spawn(async move {
        run_job(job_state, &job_id).await;
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response()
}

fn build_config(fields: &HashMap<String, String>) -> RenderConfig {
    let mut config = RenderConfig::default();
    if let Some(res) = fields.get("resolution") {
        let parts: Vec<&str> = res.split('x').collect();
        if parts.len() == 2 {
            if let (Ok(w), Ok(h)) = (parts[0].parse(), parts[1].parse()) {
                config.resolution = (w, h);
            }
        }
    }
    if let Some(fps) = fields.get("fps").and_then(|v| v.parse().ok()) {
        config.fps = fps;
    }
    config.bitrate = fields
        .get("crf")
        .cloned()
        .unwrap_or_else(|| "23".to_string());
    config.video = fields.get("video").map(|v| v == "mov").unwrap_or(false);
    config.disable_loading = !fields.get("load").map(|v| v == "true").unwrap_or(false);
    if fields.get("finish").map(|v| v == "true").unwrap_or(false) {
        config.ending_length = 3.0;
        config.ui_score = true;
        config.ui_combo = true;
        config.ui_pb = true;
        config.ui_level = true;
        config.ui_name = true;
    }
    config.hardware_accel = true;
    config.video_codec = "h264".to_string();
    config.encoder = "auto".to_string();
    config.target_audio = 48000;
    config.audio_format = "flac".to_string();
    config
}

async fn run_job(state: AppState, id: &str) {
    let (input, output, config, cancel, name) = {
        let jobs = state.jobs.lock().await;
        let Some(job) = jobs.get(id) else { return };
        (
            job.input.clone(),
            job.output.clone(),
            job.config.clone(),
            job.cancel.clone(),
            job.view.name.clone(),
        )
    };

    set_status(&state, id, |view| {
        view.status = "running";
    })
    .await;

    let result = render_one(&state, id, &input, &output, &config, &cancel).await;
    match result {
        Ok(()) => {
            set_status(&state, id, |view| {
                view.status = "done";
                view.progress = 1.0;
                view.download = Some(format!("/api/jobs/{id}/download"));
            })
            .await;
        }
        Err(e) => {
            let canceled = cancel.load(Ordering::Relaxed);
            let message = format!("{e:#}");
            set_status(&state, id, |view| {
                view.status = if canceled { "canceled" } else { "failed" };
                view.error = Some(message.clone());
            })
            .await;
        }
    }
    eprintln!("[server] job {id} ({name}) finished");
}

async fn set_status(state: &AppState, id: &str, f: impl FnOnce(&mut JobView)) {
    let mut jobs = state.jobs.lock().await;
    if let Some(job) = jobs.get_mut(id) {
        f(&mut job.view);
    }
}

async fn render_one(
    state: &AppState,
    id: &str,
    input: &Path,
    output: &Path,
    config: &RenderConfig,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut chart_fs = fs::fs_from_file(input)?;
    let info = fs::load_info(chart_fs.deref_mut()).await?;
    let params = RenderParams {
        path: input.to_path_buf(),
        info,
        config: config.clone(),
    };

    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .arg("render")
        .arg(&state.assets)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn the renderer")?;

    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(format!("{}\n", serde_json::to_string(&params)?).as_bytes())
        .await?;
    stdin
        .write_all(format!("{}\n", serde_json::to_string(output)?).as_bytes())
        .await?;
    stdin.flush().await?;
    drop(stdin);

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::debug!("[render] {line}");
            }
        });
    }

    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let mut total: u64 = 0;
    let mut frames: u64 = 0;
    let start = Instant::now();
    let mut fps: u64 = params.config.fps as u64;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            anyhow::bail!("canceled by client");
        }
        let next = tokio::time::timeout(Duration::from_millis(250), lines.next_line()).await;
        let Ok(line) = next else { continue };
        let Some(line) = line? else { break };
        let Ok(event) = serde_json::from_str::<IPCEvent>(line.trim()) else {
            continue;
        };
        let mut jobs = state.jobs.lock().await;
        let Some(job) = jobs.get_mut(id) else { continue };
        match event {
            IPCEvent::StartMixing => {
                job.view.status = "mixing";
            }
            IPCEvent::StartRender(count) => {
                total = count;
                job.view.status = "running";
            }
            IPCEvent::Frame => {
                frames += 1;
                if frames % 10 == 0 {
                    let elapsed = start.elapsed().as_secs_f64();
                    if elapsed > 0.0 {
                        fps = (frames as f64 / elapsed).round() as u64;
                    }
                }
                job.view.progress = if total > 0 {
                    (frames as f64 / total as f64).min(1.0)
                } else {
                    0.0
                };
                job.view.fps = fps;
                job.view.eta = if total > frames && fps > 0 {
                    (total - frames) as f64 / fps as f64
                } else {
                    0.0
                };
            }
            IPCEvent::EncoderFps(fps) => {
                job.view.fps = fps.max(0.0) as u64;
            }
            IPCEvent::Done(_) => {}
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        anyhow::bail!("renderer exited with {status}");
    }
    Ok(())
}

fn jobs_dir() -> PathBuf {
    std::env::temp_dir().join("phi-tkc-server")
}

fn next_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", now_secs())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

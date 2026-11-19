//! An embedded native view hosts the regular prpr game, without a child process.
//! RGBA presentation is deliberately confined to interactive preview; video
//! rendering keeps its GPU-to-encoder path.
use crate::{
    common::{NativeGraphicsGuard, NativeRenderResources},
    render::{build_player, RenderParams},
};
use anyhow::{bail, Context, Result};
use macroquad::prelude::*;
use prpr::{
    config::Mods,
    core::MSRenderTarget,
    fs,
    scene::{GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{FontArc, TextPainter, Ui},
    Main,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

struct PreviewBase(Option<NextScene>);
impl Scene for PreviewBase {
    fn on_result(&mut self, _: &mut TimeManager, result: Box<dyn std::any::Any>) -> Result<()> {
        match result.downcast::<anyhow::Error>() {
            Ok(error) => Err((*error).context("Failed to load the preview chart")),
            Err(_) => {
                self.0 = Some(NextScene::Exit);
                Ok(())
            }
        }
    }
    fn enter(&mut self, _: &mut TimeManager, _: Option<RenderTarget>) -> Result<()> {
        if self.0.is_none() {
            self.0 = Some(NextScene::Exit);
        }
        Ok(())
    }
    fn update(&mut self, _: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn render(&mut self, _: &mut TimeManager, _: &mut Ui, _: Option<RenderTarget>) -> Result<()> {
        Ok(())
    }
    fn next_scene(&mut self, _: &mut TimeManager) -> NextScene {
        self.0.take().unwrap_or_default()
    }
}

struct Session {
    id: u64,
    closed: AtomicBool,
    active: AtomicBool,
    finished: AtomicBool,
    // The producer replaces the pending frame; the main thread cannot build a
    // backlog even when its display loop is slower than the renderer.
    frame: Mutex<Option<Vec<u8>>>,
    hdr_frames: AtomicBool,
    hdr_selected: AtomicBool,
    #[cfg(target_os = "android")]
    android_hdr: Option<crate::android_preview_hdr::Converter>,
    touches: Mutex<VecDeque<macroquad::input::Touch>>,
    reset_touches: AtomicBool,
    fps: AtomicU32,
    pacing: Mutex<()>,
    pacing_wake: Condvar,
    result: Mutex<Option<Result<()>>>,
}

impl Session {
    fn set_active(&self, active: bool) {
        let _guard = self.pacing.lock().unwrap();
        self.active.store(active, Ordering::Release);
        self.pacing_wake.notify_all();
    }

    fn close(&self) {
        let _guard = self.pacing.lock().unwrap();
        self.closed.store(true, Ordering::Release);
        self.pacing_wake.notify_all();
    }

    // Only the requested FPS paces rendering. Display callbacks consume the
    // latest frame independently, including when rendering exceeds display Hz.
    fn wait_for_next_frame(&self, began: Instant, frame_time: Duration) {
        let mut guard = self.pacing.lock().unwrap();
        while self.active.load(Ordering::Acquire)
            && !self.closed.load(Ordering::Acquire)
        {
            let Some(wait) = frame_time.checked_sub(began.elapsed()) else {
                break;
            };
            guard = self.pacing_wake.wait_timeout(guard, wait).unwrap().0;
        }
    }
}

struct PreviewFps {
    started: Instant,
    frames: u32,
}
impl PreviewFps {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            frames: 0,
        }
    }
    fn reset(&mut self, session: &Session) {
        self.started = Instant::now();
        self.frames = 0;
        session.fps.store(0f32.to_bits(), Ordering::Relaxed);
    }
    fn rendered(&mut self, session: &Session) {
        self.frames += 1;
        let elapsed = self.started.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            session
                .fps
                .store((self.frames as f32 / elapsed).to_bits(), Ordering::Relaxed);
            self.started = Instant::now();
            self.frames = 0;
        }
    }
}
static SESSION: OnceLock<Mutex<Option<Arc<Session>>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn current(id: u64) -> Option<Arc<Session>> {
    SESSION
        .get()?
        .lock()
        .ok()?
        .as_ref()
        .filter(|s| s.id == id)
        .cloned()
}

#[cfg(target_os = "ios")]
pub(crate) fn suspend_for_background() {
    let session = SESSION.get().and_then(|slot| slot.lock().ok()?.as_ref().cloned());
    if let Some(session) = session {
        session.set_active(false);
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub async fn start(
    app: tauri::AppHandle,
    params: RenderParams,
    asset_dir: PathBuf,
    show_fps: bool,
) -> Result<()> {
    let guard =
        NativeGraphicsGuard::try_acquire().context("请等待当前渲染或预览结束后再打开预览／游玩")?;
    let (width, height) = preview_size(params.config.resolution)?;
    let color_space = params.config.color_space.clone();
    let session = Arc::new(Session {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        closed: AtomicBool::new(false),
        active: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        frame: Mutex::new(None),
        hdr_frames: AtomicBool::new(false),
        hdr_selected: AtomicBool::new(false),
        #[cfg(target_os = "android")]
        android_hdr: crate::android_preview_hdr::Converter::new(&color_space),
        touches: Mutex::new(VecDeque::new()),
        reset_touches: AtomicBool::new(false),
        fps: AtomicU32::new(0f32.to_bits()),
        pacing: Mutex::new(()),
        pacing_wake: Condvar::new(),
        result: Mutex::new(None),
    });
    *SESSION.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Arc::clone(&session));
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _graphics = guard;
        let worker_session = Arc::clone(&session);
        let worker = std::thread::Builder::new()
            .name("native-preview".into())
            .spawn(move || {
                let _runtime = runtime.enter();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        params,
                        asset_dir,
                        width,
                        height,
                        Arc::clone(&worker_session),
                    )
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Native preview panicked")));
                *worker_session.result.lock().unwrap() = Some(result);
                worker_session.finished.store(true, Ordering::Release);
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                SESSION.get().unwrap().lock().unwrap().take();
                return Err(error.into());
            }
        };
        // Native invocation resolves when the full-screen view is dismissed.
        let shown = app
            .state::<tauri_plugin_chart_picker::ChartPicker<tauri::Wry>>()
            .preview(session.id, width, height, show_fps, &color_space)
            .map_err(|error| anyhow::anyhow!(error.to_string()));
        session.close();
        let joined = worker
            .join()
            .map_err(|_| anyhow::anyhow!("Preview thread did not shut down"));
        SESSION.get().unwrap().lock().unwrap().take();
        shown?;
        joined?;
        let outcome = session.result.lock().unwrap().take().unwrap_or(Ok(()));
        outcome
    })
    .await?
}

fn preview_size((width, height): (u32, u32)) -> Result<(u32, u32)> {
    if width < 16 || height < 16 {
        bail!("Preview resolution must be at least 16x16");
    }
    // Preserve the selected aspect ratio while bounding interactive readback.
    let scale = (1280.0 / width.max(height) as f64).min(1.0);
    Ok((
        (width as f64 * scale).round().max(16.0) as u32,
        (height as f64 * scale).round().max(16.0) as u32,
    ))
}

fn run(
    params: RenderParams,
    assets: PathBuf,
    width: u32,
    height: u32,
    session: Arc<Session>,
) -> Result<()> {
    if !wait_for_view(&session) {
        return Ok(());
    }
    let conf = macroquad::window::Conf {
        window_width: width as i32,
        window_height: height as i32,
        headless: true,
        platform: macroquad::miniquad::conf::Platform {
            swap_interval: Some(0),
            #[cfg(target_os = "android")]
            rendering_backend: macroquad::miniquad::conf::RenderingBackend::Vulkan,
            #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
            rendering_backend: macroquad::miniquad::conf::RenderingBackend::Metal,
            ..Default::default()
        },
        ..Default::default()
    };
    let result = Arc::new(Mutex::new(None));
    let result_out = Arc::clone(&result);
    let draw = move || {
        macroquad::Window::from_config(conf, async move {
            let _resources = NativeRenderResources;
            set_pc_assets_folder(&assets.display().to_string());
            let outcome = play(params, width, height, session).await;
            *result_out.lock().unwrap() = Some(outcome);
            macroquad::window::miniquad_context().request_quit();
        })
    };
    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    objc::rc::autoreleasepool(draw);
    #[cfg(target_os = "android")]
    draw();
    let outcome = result
        .lock()
        .unwrap()
        .take()
        .unwrap_or_else(|| Err(anyhow::anyhow!("Preview did not report a result")));
    outcome
}

async fn play(params: RenderParams, width: u32, height: u32, session: Arc<Session>) -> Result<()> {
    if session.closed.load(Ordering::Acquire) {
        return Ok(());
    }
    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    let activity = crate::apple_lifecycle::RenderActivity::begin();
    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    let check_open = || -> Result<()> {
        if session.closed.load(Ordering::Acquire) {
            bail!("Preview closed");
        }
        Ok(())
    };
    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    if let Err(error) = activity.checkpoint(check_open, finish_preview_gpu) {
        if session.closed.load(Ordering::Acquire) {
            return Ok(());
        }
        return Err(error);
    }
    let chart_fs = fs::fs_from_file(&params.path)?;
    let mut config = params.config.to_config();
    config.interactive = true;
    config.hdr_output = session.hdr_selected.load(Ordering::Acquire);
    let hdr_output = config.hdr_output;
    session.hdr_frames.store(hdr_output, Ordering::Release);
    config.mods = if config.autoplay.unwrap_or(false) {
        config.mods | Mods::AUTOPLAY
    } else {
        config.mods & !Mods::AUTOPLAY
    };
    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font);
    let player = build_player(&params.config).await?;
    if !wait_for_view(&session) {
        return Ok(());
    }
    let target = Rc::new(if hdr_output {
        MSRenderTarget::new_hdr((width, height), 1)
    } else {
        MSRenderTarget::new((width, height), params.config.sample_count)
    });
    let context = macroquad::window::miniquad_context();
    context.begin_pass(
        target.input().render_pass,
        macroquad::miniquad::PassAction::Clear {
            color: Some((0., 0., 0., 1.)),
            depth: Some(1.),
            stencil: Some(0),
        },
    );
    context.end_render_pass();
    let presentation = (!hdr_output).then(|| render_target(width, height));
    let tm = TimeManager::from_config(&config);
    let scene = LoadingScene::new(
        GameMode::Normal,
        params.info,
        config,
        chart_fs,
        Some(player),
        None,
        None,
    )
    .await?;
    let target_in = Rc::clone(&target);
    let mut main = Main::new(
        Box::new(PreviewBase(Some(NextScene::Overlay(Box::new(scene))))),
        tm,
        move || Some(target_in.input()),
    )
    .await?;
    main.top_level = false;
    main.viewport = Some((0, 0, width as i32, height as i32));
    let mut paused = false;
    let mut fps = PreviewFps::new();
    let frame_time = Duration::from_secs_f64(1.0 / params.config.fps.max(1) as f64);
    while !session.closed.load(Ordering::Acquire) {
        #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
        {
            let result = activity.checkpoint(check_open, || {
                macroquad::input::cancel_touches();
                session.touches.lock().unwrap().clear();
                if !paused {
                    main.pause()?;
                    paused = true;
                    fps.reset(&session);
                }
                finish_preview_gpu()
            });
            match result {
                Ok(true) if paused => {
                    main.resume()?;
                    paused = false;
                    fps.reset(&session);
                }
                Ok(_) => {}
                Err(_) if session.closed.load(Ordering::Acquire) => break,
                Err(error) => return Err(error),
            }
        }
        let active = session.active.load(Ordering::Acquire);
        if !active && !paused {
            macroquad::input::cancel_touches();
            session.touches.lock().unwrap().clear();
            main.pause()?;
            paused = true;
            fps.reset(&session);
        } else if active && paused {
            main.resume()?;
            paused = false;
            fps.reset(&session);
        }
        if paused {
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        let began = Instant::now();
        let mut touches = session.touches.lock().unwrap();
        if session.reset_touches.swap(false, Ordering::AcqRel) {
            macroquad::input::cancel_touches();
        }
        for touch in touches.drain(..) {
            macroquad::input::inject_touch(touch);
        }
        drop(touches);
        main.update()?;
        main.render(&mut painter)?;
        unsafe {
            macroquad::window::get_internal_gl().flush();
        }
        let mut pixels = if let Some(presentation) = presentation {
            macroquad::window::miniquad_context()
                .copy_render_target(target.output().render_pass, presentation.render_pass);
            presentation.texture.get_texture_data().bytes
        } else {
            let mut pixels = vec![0; width as usize * height as usize * 8];
            target.output().texture.raw_miniquad_texture_handle().read_pixels(
                macroquad::window::miniquad_context(),
                &mut pixels,
            );
            pixels
        };
        // Metal's shader conversion flips vertex Y to preserve GL coordinates.
        // UIKit needs top-first rows. Vulkan already produces that orientation.
        #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
        {
            let stride = width as usize * if hdr_output { 8 } else { 4 };
            for row in 0..height as usize / 2 {
                let (upper, lower) = pixels.split_at_mut((height as usize - row - 1) * stride);
                upper[row * stride..(row + 1) * stride].swap_with_slice(&mut lower[..stride]);
            }
        }
        if !hdr_output {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        *session.frame.lock().unwrap() = Some(pixels);
        fps.rendered(&session);
        if main.should_exit() {
            break;
        }
        session.wait_for_next_frame(began, frame_time);
        next_frame().await;
    }
    // Pausing also stops the native audio stream before textures/scenes drop.
    main.pause()?;
    if let Some(presentation) = presentation {
        presentation.delete();
    }
    Ok(())
}

fn wait_for_view(session: &Session) -> bool {
    while !session.active.load(Ordering::Acquire) {
        if session.closed.load(Ordering::Acquire) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !session.closed.load(Ordering::Acquire)
}

#[cfg(any(target_os = "ios", feature = "apple-render-check"))]
fn finish_preview_gpu() -> Result<()> {
    unsafe {
        macroquad::window::get_internal_gl().flush();
    }
    macroquad::window::miniquad_context()
        .metal_context()
        .context("Metal context is unavailable")?
        .finish();
    Ok(())
}

/// The view calls these only with its session token; a late callback from an old
/// view can never stop or inject input into a newer game.
#[no_mangle]
pub unsafe extern "C" fn phi_tkc_preview_read_frame(
    id: u64,
    bytes: *mut u8,
    capacity: usize,
) -> i32 {
    preview_read_frame(id, bytes, capacity, false)
}

#[no_mangle]
pub unsafe extern "C" fn phi_tkc_preview_read_frame_hdr(id: u64, bytes: *mut u8, capacity: usize) -> i32 {
    preview_read_frame(id, bytes, capacity, true)
}

#[no_mangle]
pub extern "C" fn phi_tkc_preview_set_hdr(id: u64, active: i32) {
    if let Some(session) = current(id) {
        session.hdr_selected.store(active != 0, Ordering::Release);
    }
}

unsafe fn preview_read_frame(id: u64, bytes: *mut u8, capacity: usize, hdr: bool) -> i32 {
    let Some(session) = current(id) else {
        return -1;
    };
    if session.finished.load(Ordering::Acquire) {
        return -1;
    }
    let mut frame = session.frame.lock().unwrap();
    let Some(data) = frame.as_ref() else {
        return 0;
    };
    let hdr_source = session.hdr_frames.load(Ordering::Acquire);
    if bytes.is_null() || (hdr && !hdr_source) {
        return 0;
    }
    if hdr_source && !hdr {
        if capacity < data.len() / 2 {
            return 0;
        }
        let output = std::slice::from_raw_parts_mut(bytes, data.len() / 2);
        for (destination, pixel) in output.chunks_exact_mut(4).zip(data.chunks_exact(8)) {
            let [r, g, b] = read_linear_bt2020(pixel);
            let srgb = [
                1.660491 * r - 0.587641 * g - 0.072850 * b,
                -0.124550 * r + 1.132900 * g - 0.008349 * b,
                -0.018151 * r - 0.100579 * g + 1.118730 * b,
            ];
            for (channel, linear) in destination.iter_mut().zip(srgb) {
                let linear = linear.clamp(0., 1.);
                let encoded = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1. / 2.4) - 0.055 };
                *channel = (encoded * 255.).round() as u8;
            }
            destination[3] = 255;
        }
    } else {
        if capacity < data.len() {
            return 0;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), bytes, data.len());
    }
    frame.take();
    1
}

pub(crate) fn read_linear_bt2020(pixel: &[u8]) -> [f32; 3] {
    [0, 2, 4].map(|offset| {
        half::f16::from_bits(u16::from_ne_bytes([pixel[offset], pixel[offset + 1]])).to_f32()
    })
}
#[no_mangle]
pub extern "C" fn phi_tkc_preview_close(id: u64) {
    if let Some(session) = current(id) {
        session.close();
    }
}
#[no_mangle]
pub extern "C" fn phi_tkc_preview_active(id: u64, active: i32) {
    if let Some(session) = current(id) {
        session.set_active(active != 0);
    }
}
#[no_mangle]
pub extern "C" fn phi_tkc_preview_fps(id: u64) -> f32 {
    current(id).map_or(0., |session| {
        f32::from_bits(session.fps.load(Ordering::Relaxed))
    })
}
#[no_mangle]
pub extern "C" fn phi_tkc_preview_touch(
    id: u64,
    contact: u64,
    phase: i32,
    x: f32,
    y: f32,
    time: f64,
) {
    let Some(session) = current(id) else {
        return;
    };
    if !session.active.load(Ordering::Acquire)
        || session.closed.load(Ordering::Acquire)
        || !x.is_finite()
        || !y.is_finite()
        || !time.is_finite()
    {
        return;
    }
    let phase = match phase {
        0 => macroquad::input::TouchPhase::Started,
        1 => macroquad::input::TouchPhase::Moved,
        2 => macroquad::input::TouchPhase::Ended,
        3 => macroquad::input::TouchPhase::Cancelled,
        _ => return,
    };
    let mut touches = session.touches.lock().unwrap();
    if touches.len() >= 512 {
        // Cancel all contacts rather than leaving an unseen release stuck down.
        touches.clear();
        session.reset_touches.store(true, Ordering::Release);
        return;
    }
    touches.push_back(macroquad::input::Touch {
        id: contact,
        phase,
        position: vec2(x, y),
        time,
    });
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use jni::{
        objects::{JByteBuffer, JObject},
        sys::{jfloat, jint, jlong},
        JNIEnv,
    };
    #[no_mangle]
    pub extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_fps(
        _: JNIEnv,
        _: JObject,
        id: jlong,
    ) -> jfloat {
        phi_tkc_preview_fps(id as u64)
    }
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_readFrame(
        env: JNIEnv,
        _: JObject,
        id: jlong,
        buffer: JByteBuffer,
    ) -> jint {
        let Ok(bytes) = env.get_direct_buffer_address(&buffer) else {
            return 0;
        };
        let Ok(size) = env.get_direct_buffer_capacity(&buffer) else {
            return 0;
        };
        phi_tkc_preview_read_frame(id as u64, bytes, size)
    }
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_readHdrFrame(
        env: JNIEnv,
        _: JObject,
        id: jlong,
        buffer: JObject,
    ) -> jint {
        let Some(session) = current(id as u64) else { return -1 };
        if session.finished.load(Ordering::Acquire) {
            return -1;
        }
        let mut frame = session.frame.lock().unwrap();
        let Some(data) = frame.as_ref() else { return 0 };
        let Some(converter) = session.android_hdr.as_ref() else { return -2 };
        let hdr_source = session.hdr_frames.load(Ordering::Acquire);
        if crate::android_preview_hdr::write_buffer(env, buffer, data, converter, hdr_source).is_err() {
            return -2;
        }
        frame.take();
        1
    }
    #[no_mangle]
    pub extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_setHdr(
        _: JNIEnv, _: JObject, id: jlong, active: jint,
    ) {
        phi_tkc_preview_set_hdr(id as u64, active);
    }
    #[no_mangle]
    pub extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_close(
        _: JNIEnv,
        _: JObject,
        id: jlong,
    ) {
        phi_tkc_preview_close(id as u64);
    }
    #[no_mangle]
    pub extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_active(
        _: JNIEnv,
        _: JObject,
        id: jlong,
        active: jint,
    ) {
        phi_tkc_preview_active(id as u64, active);
    }
    #[no_mangle]
    pub extern "system" fn Java_com_cnmdbook_chartpicker_PreviewBridge_touch(
        _: JNIEnv,
        _: JObject,
        id: jlong,
        contact: jlong,
        phase: jint,
        x: jfloat,
        y: jfloat,
        time: f64,
    ) {
        phi_tkc_preview_touch(id as u64, contact as u64, phase, x, y, time);
    }
}

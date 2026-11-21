#[path = "preview_hdr.rs"]
mod hdr;

use crate::render::{build_player, RenderParams};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    config::Mods,
    fs,
    scene::{show_error, GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{FontArc, TextPainter, Ui},
    Main,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::BufRead,
    rc::Rc,
    time::{Duration, Instant},
};
struct BaseScene(Option<NextScene>, bool);
impl Scene for BaseScene {
    fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn std::any::Any>) -> Result<()> {
        show_error(
            result
                .downcast::<anyhow::Error>()
                .unwrap()
                .context("loading failed"),
        );
        self.1 = true;
        Ok(())
    }
    fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.0.is_none() && !self.1 {
            self.0 = Some(NextScene::Exit);
        }
        Ok(())
    }
    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn render(
        &mut self,
        _tm: &mut TimeManager,
        _ui: &mut Ui,
        _target: Option<RenderTarget>,
    ) -> Result<()> {
        Ok(())
    }
    fn next_scene(&mut self, _tm: &mut TimeManager) -> prpr::scene::NextScene {
        self.0.take().unwrap_or_default()
    }
}

fn draw_preview_fps(text: &str, overlay: Option<RenderTarget>) {
    if let Some(target) = overlay {
        let mut camera = Camera2D::from_display_rect(Rect::new(0., 0., screen_width(), screen_height()));
        camera.render_target = Some(target);
        set_camera(&camera);
    } else {
        set_default_camera();
    }
    const FONT_SIZE: u16 = 20;
    let text_width = measure_text(text, None, FONT_SIZE, 1.).width;
    draw_rectangle(12., 12., text_width + 16., 30., Color::new(0., 0., 0., 0.7));
    draw_text(text, 20., 34., f32::from(FONT_SIZE), WHITE);
}

pub async fn main() -> Result<()> {
    set_pc_assets_folder(&std::env::args().nth(2).unwrap());

    let mut stdin = std::io::stdin().lock();
    let stdin = &mut stdin;

    let mut line = String::new();
    stdin.read_line(&mut line)?;
    let params: RenderParams = serde_json::from_str(line.trim())?;
    let frame_time = Duration::from_secs_f64(1.0 / params.config.fps.max(1) as f64);

    let fs = fs::fs_from_file(&params.path)?;
    let info = params.info;
    let mut config = params.config.to_config();
    // The native Vulkan window may move between HDR and SDR outputs. Keep the
    // scene in linear HDR and read the current swapchain encoding every frame.
    config.hdr_output = params.config.is_hdr()
        && macroquad::window::miniquad_context().rendering_backend()
            != macroquad::miniquad::conf::RenderingBackend::OpenGL;
    let hdr_material = config.hdr_output.then(hdr::load).transpose()?;
    let hdr_frame = Rc::new(RefCell::new(hdr_material.map(|_| {
        hdr::Frame::new((screen_width() as u32, screen_height() as u32))
    })));
    let scene_frame = Rc::clone(&hdr_frame);
    config.mods = config.autoplay.unwrap_or(false).then(|| config.mods | Mods::AUTOPLAY).unwrap_or_else(|| config.mods & !Mods::AUTOPLAY);

    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font);

    let player = build_player(&params.config).await?;

    let ctm = TimeManager::from_config(&config); // strange variable name...
    let config_ref = &config;
    let mut main = Main::new(
        Box::new(BaseScene(
            Some(NextScene::Overlay(Box::new(
                LoadingScene::new(
                    GameMode::Normal,
                    info,
                    config_ref.clone(),
                    fs,
                    Some(player),
                    None,
                    None,
                )
                .await?,
            ))),
            false,
        )),
        ctm,
        move || scene_frame.borrow().as_ref().map(hdr::Frame::scene),
    )
    .await?;
    let mut is_fullscreen = false;
    let started = Instant::now();
    let duration = std::env::var("PRPR_PREVIEW_SECONDS").ok().and_then(|it| it.parse::<f64>().ok());
    let warmup = std::env::var("PRPR_PREVIEW_WARMUP").ok().and_then(|it| it.parse::<f64>().ok()).unwrap_or(5.0);
    let mut report_time = started;
    let mut frame_times = VecDeque::with_capacity(100);
    let mut frame_sum = 0.0;
    let mut measured = Vec::new();
    let mut last_color_space = None;
    let show_fps = std::env::var_os("PHI_TKC_PREVIEW_SHOW_FPS").is_some();
    let mut fps_sample_start = started;
    let mut fps_sample_frames = 0_u32;
    let mut fps_text = String::from("FPS: --");
    loop {
        let frame_start = Instant::now();
        if is_key_pressed(KeyCode::F11) {
            is_fullscreen = !is_fullscreen;
            set_fullscreen(is_fullscreen);
        }
        let updated;
        if let Some(material) = hdr_material {
            let size = (screen_width() as u32, screen_height() as u32);
            if size.0 == 0 || size.1 == 0 {
                next_frame().await;
                continue;
            }
            let mut frame = hdr_frame.borrow_mut();
            if frame.as_ref().is_some_and(|frame| frame.size() != size) {
                *frame = Some(hdr::Frame::new(size));
            }
            drop(frame);
            let color_space = hdr::color_space();
            if last_color_space != Some(color_space) {
                log::info!("Preview presentation color space: {color_space:?}");
                last_color_space = Some(color_space);
            }
            main.update()?;
            updated = Instant::now();
            let frame = hdr_frame.borrow();
            let frame = frame.as_ref().unwrap();
            main.render_with_overlay_target(&mut painter, Some(frame.overlay()))?;
            if show_fps {
                draw_preview_fps(&fps_text, Some(frame.overlay()));
            }
            frame.present(material, color_space);
        } else {
            main.update()?;
            updated = Instant::now();
            main.render(&mut painter)?;
            if show_fps {
                draw_preview_fps(&fps_text, None);
            }
        }
        let rendered = Instant::now();
        if main.should_exit() {
            break;
        }
        if let Some(wait) = frame_time.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(wait);
        }
        next_frame().await;
        let now = Instant::now();
        if show_fps {
            fps_sample_frames += 1;
            let interval = now.duration_since(fps_sample_start);
            if interval >= Duration::from_millis(500) {
                fps_text = format!("FPS: {:.0}", f64::from(fps_sample_frames) / interval.as_secs_f64());
                fps_sample_start = now;
                fps_sample_frames = 0;
            }
        }
        let update = (updated - frame_start).as_secs_f64();
        let render = (rendered - updated).as_secs_f64();
        let frame = (now - frame_start).as_secs_f64();
        let sample = (update, render, frame);
        if frame_times.len() == 100 {
            let old: (f64, f64, f64) = frame_times.pop_front().unwrap();
            frame_sum -= old.0 + old.1;
        }
        frame_times.push_back(sample);
        frame_sum += update + render;
        let elapsed = (now - started).as_secs_f64();
        if elapsed >= warmup && duration.is_some() {
            measured.push(sample);
        }
        if (now - report_time).as_secs_f64() >= 1.0 {
            let count = frame_times.len() as f64;
            let update = frame_times.iter().map(|it| it.0).sum::<f64>() / count;
            let render = frame_times.iter().map(|it| it.1).sum::<f64>() / count;
            let frame = frame_times.iter().map(|it| it.2).sum::<f64>() / count;
            log::debug!("preview t={elapsed:.1} cpu_fps={:.1} frame_fps={:.1} update_us={:.1} render_us={:.1}", count / frame_sum, 1.0 / frame, update * 1e6, render * 1e6);
            report_time = now;
        }
        if duration.map_or(false, |it| elapsed >= warmup + it) {
            break;
        }
    }
    if !measured.is_empty() {
        let count = measured.len() as f64;
        let update = measured.iter().map(|it| it.0).sum::<f64>() / count;
        let render = measured.iter().map(|it| it.1).sum::<f64>() / count;
        let frame = measured.iter().map(|it| it.2).sum::<f64>() / count;
        let mut cpu: Vec<_> = measured.iter().map(|it| it.0 + it.1).collect();
        cpu.sort_by(f64::total_cmp);
        let p95 = cpu[((cpu.len() - 1) as f64 * 0.95) as usize];
        log::debug!("preview summary samples={} cpu_fps={:.1} frame_fps={:.1} update_us={:.1} render_us={:.1} cpu_p95_us={:.1}", measured.len(), 1.0 / (update + render), 1.0 / frame, update * 1e6, render * 1e6, p95 * 1e6);
    }

    Ok(())
}

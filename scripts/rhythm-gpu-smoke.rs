//! Apple CI regression for the real GPU rhythm strip compositor. All chart and
//! image content is synthetic; no chart fixture or video encoder is required.
use anyhow::{ensure, Context, Result};
use macroquad::{miniquad as mq, prelude::*};
use prpr::{
    core::{ChartExtra, MSRenderTarget, RhythmComposite, RhythmNoteSelection, RhythmStrip},
    ui::{FontArc, TextPainter},
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const WIDTH: u32 = 320;
const GAME_HEIGHT: u32 = 180;
const OUTPUT_HEIGHT: u32 = 226;

/// The production target can be RGB8. Copy through a real RGBA8 render pass
/// before readback so RGB channels are never reinterpreted as RGBA pixels.
fn read_rgba(target: RenderTarget, width: u32, height: u32) -> Vec<u8> {
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    let texture = mq::Texture::new_render_texture(
        gl.quad_context,
        mq::TextureParams {
            width,
            height,
            format: mq::TextureFormat::RGBA8,
            ..Default::default()
        },
    );
    let pass = mq::RenderPass::new(gl.quad_context, texture, None);
    gl.quad_context.copy_render_target(target.render_pass, pass);
    let mut bytes = vec![0; width as usize * height as usize * 4];
    texture.read_pixels(gl.quad_context, &mut bytes);
    pass.delete(gl.quad_context);
    texture.delete(gl.quad_context);
    // The Metal shader translator preserves OpenGL's bottom-first target
    // rows. Match the production Apple export's flip_y=true before treating
    // the prefix as the game rectangle and the suffix as its appended strip.
    let row_bytes = width as usize * 4;
    for y in 0..height as usize / 2 {
        let opposite = (height as usize - 1 - y) * row_bytes;
        let (first, last) = bytes.split_at_mut(opposite);
        first[y * row_bytes..(y + 1) * row_bytes].swap_with_slice(&mut last[..row_bytes]);
    }
    bytes
}

struct ResourceScope(Arc<AtomicBool>);
impl Drop for ResourceScope {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn verify_composite(resources_dropped: Arc<AtomicBool>) -> Result<()> {
    // Declare first so this guard drops after the chart, targets, and painter.
    let _resources = ResourceScope(resources_dropped);
    ensure!(
        macroquad::window::miniquad_context()
            .metal_context()
            .is_some(),
        "rhythm compositor test requires a real Metal context"
    );
    let font = FontArc::try_from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/font.ttf"
    )))
    .context("failed to load the bundled smoke-test font")?;
    let mut painter = TextPainter::new(font);
    // Distinct glyphs and a sustain band exercise vector shapes, text, and the
    // beat clock without adding an image dependency to the application.
    let chart = prpr::parse::parse_phigros(
        r#"{
        "offset":0,"judgeLineList":[{"bpm":120,
        "judgeLineDisappearEvents":[],"judgeLineMoveEvents":[],"judgeLineRotateEvents":[],
        "speedEvents":[{"startTime":0,"endTime":1024,"value":1}],"notesBelow":[],
        "notesAbove":[
        {"type":1,"time":0,"positionX":0,"holdTime":0,"speed":1,"floorPosition":0},
        {"type":2,"time":16,"positionX":0,"holdTime":0,"speed":1,"floorPosition":0},
        {"type":3,"time":32,"positionX":0,"holdTime":32,"speed":1,"floorPosition":0},
        {"type":4,"time":64,"positionX":0,"holdTime":0,"speed":1,"floorPosition":0}]}]
        }"#,
        ChartExtra::default(),
    )?;
    let strip = RhythmStrip::from_chart(
        &chart,
        RhythmNoteSelection {
            tap: true,
            drag: true,
            hold: true,
            flick: true,
        },
        None,
    );
    ensure!(
        strip.notes().len() == 4,
        "generated chart did not exercise all four note kinds"
    );
    let composite = RhythmComposite::new(WIDTH, GAME_HEIGHT, Some(strip), 2)?;
    ensure!(
        composite.layout.output_height == OUTPUT_HEIGHT,
        "rhythm strip changed the required extended output dimensions"
    );
    let game = MSRenderTarget::new((WIDTH, GAME_HEIGHT), 4);
    let disabled = RhythmComposite::new(WIDTH, GAME_HEIGHT, None, 2)?;
    ensure!(
        disabled.compose(0, &game, None, &mut painter).render_pass == game.output().render_pass,
        "disabled rhythm analysis replaced the original game target"
    );
    let mut camera =
        Camera2D::from_display_rect(Rect::new(0., 0., WIDTH as f32, GAME_HEIGHT as f32));
    camera.render_target = Some(game.input());
    set_camera(&camera);
    clear_background(BLACK);
    let half_width = WIDTH as f32 * 0.5;
    let half_height = GAME_HEIGHT as f32 * 0.5;
    // Use exact saturated channels rather than the named macroquad colors,
    // whose display palette is deliberately softer than pure RGB primaries.
    draw_rectangle(0., 0., half_width, half_height, Color::new(1., 0., 0., 1.));
    draw_rectangle(
        half_width,
        0.,
        half_width,
        half_height,
        Color::new(0., 1., 0., 1.),
    );
    draw_rectangle(
        0.,
        half_height,
        half_width,
        half_height,
        Color::new(0., 0., 1., 1.),
    );
    draw_rectangle(
        half_width,
        half_height,
        half_width,
        half_height,
        Color::new(1., 1., 0., 1.),
    );
    unsafe { get_internal_gl() }.flush();
    let original = read_rgba(game.output(), WIDTH, GAME_HEIGHT);
    // Validate the source independently: an empty/black original must not let
    // the identical-crop comparison succeed without ever drawing the fixture.
    let mut centers = Vec::new();
    for (x, y) in [
        (WIDTH / 4, GAME_HEIGHT / 4),
        (3 * WIDTH / 4, GAME_HEIGHT / 4),
        (WIDTH / 4, 3 * GAME_HEIGHT / 4),
        (3 * WIDTH / 4, 3 * GAME_HEIGHT / 4),
    ] {
        let i = ((y * WIDTH + x) * 4) as usize;
        centers.push([original[i], original[i + 1], original[i + 2]]);
    }
    let expected = vec![[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    ensure!(
        centers == expected,
        "MSAA test source did not contain the four RGB quadrants in top-first order: {centers:?}"
    );

    let mut passes = Vec::new();
    let mut strips = Vec::new();
    for (slot, time) in [(0, 0.), (1, 0.025), (0, 0.18)] {
        let rendered = composite.compose(slot, &game, Some(time), &mut painter);
        passes.push(rendered.render_pass);
        let bytes = read_rgba(rendered, WIDTH, OUTPUT_HEIGHT);
        let differing = bytes[..original.len()]
            .iter()
            .zip(&original)
            .filter(|(a, b)| a != b)
            .count();
        ensure!(
            differing == 0,
            "Metal rhythm compositor changed {differing} game bytes at time {time}, slot {slot}"
        );
        let bottom = &bytes[original.len()..];
        let visible = bottom
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|&v| v > 100))
            .count();
        ensure!(
            visible >= 32,
            "Metal rhythm strip was blank at time {time}: {visible} visible pixels"
        );
        strips.push(bottom.to_vec());
        println!("APPLE_RHYTHM_CROP time={time} slot={slot} size={WIDTH}x{OUTPUT_HEIGHT} game_bytes_changed={differing} visible_strip_pixels={visible}");
    }
    ensure!(
        passes[0] != passes[1] && passes[0] == passes[2],
        "rhythm compositor did not rotate and reuse its two independent frame slots"
    );
    ensure!(
        strips[0] != strips[1] && strips[1] != strips[2],
        "rhythm strip did not advance its note/reference/pulse clock"
    );
    // Result/loading pages expose no chart clock. They must keep the last
    // rendered game time instead of restarting the rhythm strip at beat zero.
    let result_page = composite.compose(1, &game, None, &mut painter);
    let bytes = read_rgba(result_page, WIDTH, OUTPUT_HEIGHT);
    ensure!(
        &bytes[original.len()..] == strips[2].as_slice(),
        "result-page None clock reset the last rendered rhythm time"
    );
    println!("APPLE_RHYTHM_RESULT_CLOCK last_time=0.18 strip_bytes_changed=0");
    Ok(())
}

#[test]
fn apple_rhythm_composite_preserves_game_rectangle() -> Result<()> {
    let result = Arc::new(Mutex::new(None::<Result<()>>));
    let finished = Arc::clone(&result);
    let dropped = Arc::new(AtomicBool::new(false));
    let resources_dropped = Arc::clone(&dropped);
    macroquad::Window::from_config(
        mq::conf::Conf {
            headless: true,
            window_width: WIDTH as i32,
            window_height: OUTPUT_HEIGHT as i32,
            platform: mq::conf::Platform {
                rendering_backend: mq::conf::RenderingBackend::Metal,
                ..Default::default()
            },
            ..Default::default()
        },
        async move {
            // Macroquad catches frame panics. Return the failure explicitly to the
            // Rust test and always request normal event-loop shutdown afterwards.
            let checked = catch_unwind(AssertUnwindSafe(|| verify_composite(resources_dropped)))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "rhythm compositor panicked inside the GPU frame"
                    ))
                });
            *finished.lock().unwrap() = Some(checked);
            macroquad::window::miniquad_context().request_quit();
        },
    );
    let checked = result
        .lock()
        .unwrap()
        .take()
        .context("Metal event loop returned without completing rhythm verification")?;
    checked?;
    ensure!(
        dropped.load(Ordering::Acquire),
        "rhythm GPU resources did not leave their scope before window shutdown"
    );
    println!("APPLE_RHYTHM_COMPOSITE_PASS");
    Ok(())
}

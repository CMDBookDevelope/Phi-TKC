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

const WIDTH: u32 = 640;
const GAME_HEIGHT: u32 = 360;
const OUTPUT_HEIGHT: u32 = 424;

/// The production target can be RGB8. Copy through a real RGBA8 render pass
/// before readback so RGB channels are never reinterpreted as RGBA pixels.
fn read_rgba_raw(target: RenderTarget, width: u32, height: u32) -> Vec<u8> {
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
    bytes
}

fn read_rgba(target: RenderTarget, width: u32, height: u32) -> Vec<u8> {
    let mut bytes = read_rgba_raw(target, width, height);
    // The Metal shader translator preserves OpenGL's bottom-first target
    // rows for the generic crop assertion. Match the production Apple
    // export's flip_y=true before treating the prefix as the game rectangle
    // and the suffix as its appended strip.
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

// Use real PEZ metadata and GameScene's loader, rather than constructing a
// BpmList in the test. A fixed-BPM PGR may change its scrolling speed without
// changing musical tempo: verify that separately from the variable-BPM RPE.
#[cfg(target_os = "macos")]
mod variable_bpm {
    // BPM ROIs and the native MP4 reader both use top-first image coordinates.
    // Metal texture readback is bottom-first, so use the same row-normalizing
    // helper as the composite crop check above.
    use super::read_rgba;
    use anyhow::{ensure, Context, Result};
    use macroquad::{miniquad as mq, prelude::*};
    use prpr::{
        core::{MSRenderTarget, RhythmComposite, RhythmNoteSelection, RhythmStrip},
        scene::GameScene,
        ui::{FontArc, TextPainter},
    };
    use serde_json::{json, Value};
    use std::{
        future::Future,
        io::Write,
        panic::{catch_unwind, AssertUnwindSafe},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    const WIDTH: u32 = 640;
    const GAME_HEIGHT: u32 = 360;
    const STRIP_HEIGHT: u32 = 64;
    const OUTPUT_HEIGHT: u32 = GAME_HEIGHT + STRIP_HEIGHT;
    const BPMS: [u32; 4] = [120, 180, 90, 150];
    const SAMPLE_TIMES: [f32; 3] = [0.125, 2.125, 4.125];
    const COLORS: [&str; 3] = ["#4A70FF", "#FFC07A", "#C690E5"];

    fn palette() -> [String; 3] {
        COLORS.map(str::to_owned)
    }

    fn tempo_json() -> Value {
        json!([
            {"startTime": [0, 0, 1], "bpm": 120},
            {"startTime": [4, 0, 1], "bpm": 180},
            {"startTime": [10, 0, 1], "bpm": 90}
        ])
    }

    fn pgr_json(bpm: u32) -> Value {
        json!({
            "formatVersion": 3, "offset": 0.125,
            "judgeLineList": [{
                "bpm": bpm,
                "judgeLineDisappearEvents": [{"startTime": 0, "endTime": 512, "start": 0, "end": 1}],
                "judgeLineRotateEvents": [{"startTime": 0, "endTime": 512, "start": 0, "end": 90}],
                "judgeLineMoveEvents": [{"startTime": 0, "endTime": 512, "start": 0.25, "end": 0.75, "start2": 0.5, "end2": 0.5}],
                "speedEvents": [{"startTime": 0, "endTime": 512, "value": 1}],
                "notesAbove": [
                    {"type": 1, "time": 32, "positionX": 0, "holdTime": 0, "speed": 1, "floorPosition": 0.5},
                    {"type": 3, "time": 224, "positionX": 0, "holdTime": 96, "speed": 1, "floorPosition": 3.5}
                ], "notesBelow": []
            }]
        })
    }

    fn pgr_speed_json() -> Value {
        let mut chart = pgr_json(150);
        chart["judgeLineList"][0]["speedEvents"] = json!([
            {"startTime": 0, "endTime": 128, "value": 1},
            {"startTime": 128, "endTime": 256, "value": 2},
            {"startTime": 256, "endTime": 512, "value": 0.5}
        ]);
        chart
    }

    fn rpe_json() -> Value {
        let event = |value: f32| {
            json!([{
                "startTime": [0, 0, 1], "endTime": [20, 0, 1],
                "start": value, "end": value, "easingType": 1
            }])
        };
        let note = |kind: u8, start: i32, end: i32| {
            json!({
                "type": kind, "above": 1, "startTime": [start, 0, 1],
                "endTime": [end, 0, 1], "positionX": 0, "yOffset": 0,
                "alpha": 255, "size": 1, "speed": 1, "isFake": 0, "visibleTime": 99999
            })
        };
        json!({
            "META": {"offset": 125}, "BPMList": tempo_json(),
            "judgeLineList": [{
                "Name": "Synthetic variable BPM", "Texture": "line.png", "father": null,
                "bpmfactor": 1, "isCover": 1, "extended": null,
                "eventLayers": [{"alphaEvents": event(255.), "moveXEvents": event(0.),
                    "moveYEvents": event(0.), "rotateEvents": event(0.), "speedEvents": event(1.)}],
                "notes": [note(1, 0, 0), note(2, 3, 5), note(2, 9, 11), note(1, 12, 12)]
            }]
        })
    }

    fn tone_wav() -> Vec<u8> {
        const RATE: u32 = 48_000;
        let bytes = 6 * RATE * 4;
        let mut wav = Vec::with_capacity(bytes as usize + 44);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(bytes + 36).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        for frame in 0..6 * RATE {
            let wave = (std::f64::consts::TAU * 440. * frame as f64 / RATE as f64).sin();
            wav.extend_from_slice(&((wave * 4000.) as i16).to_le_bytes());
            wav.extend_from_slice(&((wave * -3000.) as i16).to_le_bytes());
        }
        wav
    }

    fn fixture(directory: &Path, name: &str, chart: &Value) -> Result<PathBuf> {
        let path = directory.join(format!("{name}.pez"));
        // The 256px synthetic cover also exercises LoadingScene's 80px blur.
        const PNG: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 0, 0, 0, 1, 0,
            8, 6, 0, 0, 0, 92, 114, 168, 102, 0, 0, 2, 93, 73, 68, 65, 84, 120, 218, 237, 212, 49,
            1, 0, 0, 8, 128, 48, 250, 151, 214, 2, 54, 112, 199, 10, 112, 208, 212, 0, 63, 37, 2,
            24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0,
            6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128,
            1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0,
            128, 1, 0, 6, 0, 6, 32, 2, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6,
            0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1,
            0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0,
            128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 6, 32, 2, 24, 0, 96, 0, 128, 1, 0, 6,
            0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1,
            0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0,
            128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 6, 32,
            4, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1,
            0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0,
            128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0,
            96, 0, 128, 1, 0, 6, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0,
            6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128,
            1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0,
            128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0,
            24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0,
            6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128,
            1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 6, 0, 24, 0,
            96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0,
            24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0,
            6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 24, 0, 96, 0, 128, 1, 0, 6, 0, 92, 22, 115, 198,
            29, 227, 54, 13, 76, 114, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path)?);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("info.yml", b"name: Synthetic variable BPM\nlevel: AT Lv.10\ncharter: CI\nchart: chart.json\nmusic: song.wav\nillustration: cover.png\n".to_vec()),
            ("chart.json", serde_json::to_vec(chart)?),
            ("song.wav", tone_wav()),
            ("cover.png", PNG.to_vec()),
        ] {
            archive.start_file(name, options)?;
            archive.write_all(&data)?;
        }
        if name.ends_with("-legacy-gl") {
            archive.start_file("extra.json", options)?;
            archive.write_all(&serde_json::to_vec(&json!({
                "bpm": 120,
                "effects": [{"start": [0, 0, 1], "end": [64, 0, 1],
                    "shader": "/legacy.glsl", "global": true}]
            }))?)?;
            archive.start_file("legacy.glsl", options)?;
            archive.write_all(
                br#"#version 100
precision mediump float;
uniform sampler2D screenTexture;
varying vec2 uv;
void main() { gl_FragColor = texture2D(screenTexture, uv) * vec4(0.7, 1.0, 1.0, 1.0); }
"#,
            )?;
        }
        archive.finish()?;
        Ok(path)
    }

    async fn load_strip(path: &Path) -> Result<RhythmStrip> {
        let mut fs = prpr::fs::fs_from_file(path)?;
        let mut info = prpr::fs::load_info(fs.as_mut()).await?;
        prpr::fs::fix_info(fs.as_mut(), &mut info).await?;
        // No format hint: the same byte-based detection as the native renderer.
        ensure!(info.format.is_none(), "fixture bypassed format detection");
        let (chart, _, _) = GameScene::load_chart(fs.as_mut(), &info).await?;
        ensure!(
            (chart.offset - 0.125).abs() < 1e-6,
            "loader lost chart offset"
        );
        Ok(RhythmStrip::from_chart(
            &chart,
            RhythmNoteSelection {
                tap: false,
                drag: false,
                hold: false,
                flick: false,
            },
            None,
        )
        .with_colors(Some(&palette())))
    }

    #[derive(Clone)]
    struct DigitMask(Vec<bool>);

    fn digit_mask(rgba: &[u8]) -> DigitMask {
        let mut mask = Vec::new();
        // Exclude the ring, BPM caption and BEAT caption. Only the large,
        // neutral-white Saira digits remain, even while the halo is pulsing.
        for y in GAME_HEIGHT + STRIP_HEIGHT * 40 / 100..GAME_HEIGHT + STRIP_HEIGHT * 64 / 100 {
            for x in WIDTH * 22 / 1000..WIDTH * 78 / 1000 {
                let i = ((y * WIDTH + x) * 4) as usize;
                let pixel = &rgba[i..i + 3];
                mask.push(
                    pixel.iter().all(|&channel| channel >= 130)
                        && pixel[0].abs_diff(pixel[1]) <= 24
                        && pixel[1].abs_diff(pixel[2]) <= 24,
                );
            }
        }
        DigitMask(mask)
    }

    fn mask_distance(a: &DigitMask, b: &DigitMask) -> f32 {
        let different = a.0.iter().zip(&b.0).filter(|(x, y)| x != y).count();
        let union = a.0.iter().zip(&b.0).filter(|(x, y)| **x || **y).count();
        different as f32 / union.max(1) as f32
    }

    fn read_bpm(mask: &DigitMask, references: &[DigitMask; 4]) -> Result<u32> {
        ensure!(
            mask.0.iter().filter(|&&v| v).count() >= 12,
            "BPM digits are blank"
        );
        let mut scores: Vec<_> = references
            .iter()
            .enumerate()
            .map(|(index, reference)| (mask_distance(mask, reference), index))
            .collect();
        scores.sort_by(|a, b| a.0.total_cmp(&b.0));
        ensure!(
            scores[0].0 <= 0.30 && scores[1].0 - scores[0].0 >= 0.06,
            "BPM digits do not identify a unique 120/180/90/150 reference: {scores:?}"
        );
        Ok(BPMS[scores[0].1])
    }

    fn guide_spacing(rgba: &[u8], bpm: u32) -> Result<f32> {
        // Stop before the next tempo region; the 90 BPM final region extends
        // to the right edge. Note selection is empty, so no head/hold can be
        // mistaken for a beat guide. Inspect several vertical dash periods.
        let right = if bpm == 90 || bpm == 150 {
            WIDTH - 2
        } else {
            408
        };
        let mut centers = Vec::new();
        let mut begin = None;
        for x in 96..=right {
            let lit = (GAME_HEIGHT + 20..GAME_HEIGHT + 40)
                .filter(|&y| {
                    let i = ((y * WIDTH + x) * 4) as usize;
                    rgba[i..i + 3].iter().all(|&v| v >= 110)
                })
                .count();
            if lit >= 5 {
                begin.get_or_insert(x);
            } else if let Some(start) = begin.take() {
                centers.push((start + x - 1) as f32 * 0.5);
            }
        }
        ensure!(
            centers.len() >= 3,
            "too few actual dashed guides for {bpm}: {centers:?}"
        );
        let differences: Vec<_> = centers.windows(2).map(|pair| pair[1] - pair[0]).collect();
        // Fixed 640px viewport + six default beat columns give these authored
        // hard expectations. Do not derive them from the production BpmList.
        let expected = match bpm {
            120 | 150 => 89.6,
            180 => 59.733334,
            90 => 119.46667,
            _ => unreachable!(),
        };
        ensure!(
            differences.iter().all(|&gap| (gap - expected).abs() <= 2.),
            "wrong {bpm} BPM guide density: {differences:?}, expected {expected}px"
        );
        Ok(differences.iter().sum::<f32>() / differences.len() as f32)
    }

    fn ring_luma(rgba: &[u8]) -> f64 {
        let center = STRIP_HEIGHT as f64 * 0.5;
        let radius = STRIP_HEIGHT as f64 * 0.345;
        let mut sum = 0.;
        let mut count = 0;
        for y in 8..STRIP_HEIGHT - 8 {
            for x in 8..STRIP_HEIGHT - 8 {
                let distance = ((x as f64 - center).powi(2) + (y as f64 - center).powi(2)).sqrt();
                if (distance - radius).abs() < 3. {
                    let i = (((GAME_HEIGHT + y) * WIDTH + x) * 4) as usize;
                    sum += rgba[i..i + 3].iter().map(|&v| v as f64).sum::<f64>() / 3.;
                    count += 1;
                }
            }
        }
        sum / count as f64
    }

    fn reference_fixtures(directory: &Path) -> Result<[PathBuf; 4]> {
        let paths: Vec<_> = BPMS
            .iter()
            .map(|&bpm| fixture(directory, &format!("fixed-{bpm}"), &pgr_json(bpm)))
            .collect::<Result<_>>()?;
        paths
            .try_into()
            .map_err(|_| anyhow::anyhow!("missing reference fixtures"))
    }

    fn draw_test_game(game: &MSRenderTarget) -> Result<Vec<u8>> {
        let mut camera =
            Camera2D::from_display_rect(Rect::new(0., 0., WIDTH as f32, GAME_HEIGHT as f32));
        camera.render_target = Some(game.input());
        set_camera(&camera);
        clear_background(BLACK);
        let half_width = WIDTH as f32 * 0.5;
        let half_height = GAME_HEIGHT as f32 * 0.5;
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
        let pixels = read_rgba(game.output(), WIDTH, GAME_HEIGHT);
        let centers: Vec<_> = [
            (WIDTH / 4, GAME_HEIGHT / 4),
            (3 * WIDTH / 4, GAME_HEIGHT / 4),
            (WIDTH / 4, 3 * GAME_HEIGHT / 4),
            (3 * WIDTH / 4, 3 * GAME_HEIGHT / 4),
        ]
        .into_iter()
        .map(|(x, y)| {
            let i = ((y * WIDTH + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        })
        .collect();
        ensure!(
            centers == [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]],
            "Metal source target did not preserve exact top-first quadrants: {centers:?}"
        );
        Ok(pixels)
    }

    async fn metal_references_and_verify(
        fixtures: &[PathBuf],
        fixed: &[PathBuf; 4],
    ) -> Result<[DigitMask; 4]> {
        let _resources = crate::common::NativeRenderResources;
        macroquad::file::set_pc_assets_folder(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .to_string_lossy(),
        );
        ensure!(
            macroquad::window::miniquad_context()
                .metal_context()
                .is_some(),
            "dynamic BPM requires actual Metal"
        );
        let mut painter = TextPainter::new(FontArc::try_from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/font.ttf"
        )))?);
        let mut references = Vec::new();
        for (index, path) in fixed.iter().enumerate() {
            let strip = load_strip(path).await?;
            let reference_game = MSRenderTarget::new((WIDTH, GAME_HEIGHT), 4);
            let reference_original = draw_test_game(&reference_game)?;
            ensure!(
                strip.bpm_at(4.125) == BPMS[index] as f32,
                "fixed reference tempo is wrong"
            );
            let composite = RhythmComposite::new(WIDTH, GAME_HEIGHT, Some(strip), 1)?;
            let target = composite.compose(0, &reference_game, Some(0.125), &mut painter);
            let pixels = read_rgba(target, WIDTH, OUTPUT_HEIGHT);
            let game = &pixels[..reference_original.len()];
            let changed = game
                .chunks_exact(4)
                .zip(reference_original.chunks_exact(4))
                .filter(|(actual, expected)| actual != expected)
                .count();
            let first: Vec<_> = game
                .chunks_exact(4)
                .zip(reference_original.chunks_exact(4))
                .enumerate()
                .filter(|(_, (actual, expected))| actual != expected)
                .take(8)
                .map(|(pixel, (actual, expected))| {
                    (
                        pixel % WIDTH as usize,
                        pixel / WIDTH as usize,
                        expected,
                        actual,
                    )
                })
                .collect();
            ensure!(
                changed == 0,
                "fixed BPM {bpm} reference changed {changed} game pixels; first={first:?}",
                bpm = BPMS[index]
            );
            let mask = digit_mask(&pixels);
            println!(
                "APPLE_BPM_REFERENCE bpm={} digit_pixels={}",
                BPMS[index],
                mask.0.iter().filter(|&&v| v).count(),
            );
            references.push(mask);
        }
        let references: [DigitMask; 4] = references
            .try_into()
            .map_err(|_| anyhow::anyhow!("missing digit references"))?;
        for (index, reference) in references.iter().enumerate() {
            let lit_pixels = reference.0.iter().filter(|&&v| v).count();
            ensure!(
                lit_pixels >= 12,
                "blank fixed BPM {bpm} reference in the digit ROI: {lit_pixels} pixels",
                bpm = BPMS[index]
            );
            ensure!(
                read_bpm(reference, &references)? == BPMS[index],
                "digit references are not discriminative"
            );
        }
        for (format_index, path) in fixtures.iter().enumerate() {
            let strip = load_strip(path).await?;
            for (time, expected) in [(1.999, 120.), (2., 180.), (3.999, 180.), (4., 90.)] {
                let expected = if format_index == 0 { expected } else { 150. };
                ensure!(
                    strip.bpm_at(time) == expected,
                    "actual loader lost tempo at {time}: {}",
                    path.display()
                );
            }
            // Asset loading awaits Metal frames, so create transient MSAA
            // targets only after the loader has finished polling.
            let game = MSRenderTarget::new((WIDTH, GAME_HEIGHT), 4);
            let original = draw_test_game(&game)?;
            let composite = RhythmComposite::new(WIDTH, GAME_HEIGHT, Some(strip), 1)?;
            for (index, &time) in SAMPLE_TIMES.iter().enumerate() {
                let target = composite.compose(0, &game, Some(time), &mut painter);
                let bytes = read_rgba(target, WIDTH, OUTPUT_HEIGHT);
                let game = &bytes[..original.len()];
                if game != original {
                    let differences: Vec<_> = game
                        .chunks_exact(4)
                        .zip(original.chunks_exact(4))
                        .enumerate()
                        .filter(|(_, (actual, expected))| actual != expected)
                        .take(4)
                        .map(|(pixel, (actual, expected))| (pixel, expected, actual))
                        .collect();
                    let changed = game
                        .chunks_exact(4)
                        .zip(original.chunks_exact(4))
                        .filter(|(actual, expected)| actual != expected)
                        .count();
                    println!(
                        "APPLE_DYNAMIC_GAME_RECT time={time} changed_pixels={changed} first={differences:?}"
                    );
                    anyhow::bail!("dynamic BPM altered the game rectangle at {time}");
                }
                let expected = if format_index == 0 { BPMS[index] } else { 150 };
                ensure!(
                    read_bpm(&digit_mask(&bytes), &references)? == expected,
                    "Metal drew the wrong BPM digits at {time}: {}",
                    path.display()
                );
                let gap = guide_spacing(&bytes, expected)?;
                println!("APPLE_DYNAMIC_BPM_METAL fixture={} chart_time={time} bpm={expected} guide_spacing={gap}", path.display());
            }
            if format_index == 1 {
                let peak = read_rgba(
                    composite.compose(0, &game, Some(0.4), &mut painter),
                    WIDTH,
                    OUTPUT_HEIGHT,
                );
                let next_peak = read_rgba(
                    composite.compose(0, &game, Some(0.8), &mut painter),
                    WIDTH,
                    OUTPUT_HEIGHT,
                );
                let trough = read_rgba(
                    composite.compose(0, &game, Some(0.6), &mut painter),
                    WIDTH,
                    OUTPUT_HEIGHT,
                );
                ensure!(
                    (ring_luma(&peak) - ring_luma(&next_peak)).abs() < 0.5,
                    "150 BPM consecutive .4s beats have different luminous peaks"
                );
                ensure!(
                    ring_luma(&peak) - ring_luma(&trough) >= 8.,
                    "150 BPM metronome failed to flash every .4s while scrolling speed changed"
                );
            }
            let last = read_rgba(
                composite.compose(0, &game, Some(4.125), &mut painter),
                WIDTH,
                OUTPUT_HEIGHT,
            );
            let ending = read_rgba(
                composite.compose(0, &game, None, &mut painter),
                WIDTH,
                OUTPUT_HEIGHT,
            );
            ensure!(
                last[original.len()..] == ending[original.len()..],
                "ending reset the final BPM strip"
            );
        }
        Ok(references)
    }

    fn run_metal(fixtures: Vec<PathBuf>, fixed: [PathBuf; 4]) -> Result<[DigitMask; 4]> {
        let result = Arc::new(Mutex::new(None));
        let completed = Arc::clone(&result);
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
                // Catch panics at the actual async polling boundary: macroquad's
                // frame runner must not turn a failed assertion into a passing test.
                let mut future = Box::pin(metal_references_and_verify(&fixtures, &fixed));
                let checked = std::future::poll_fn(|cx| {
                    match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
                        Ok(poll) => poll,
                        Err(_) => std::task::Poll::Ready(Err(anyhow::anyhow!(
                            "dynamic BPM assertion panicked in Metal frame"
                        ))),
                    }
                })
                .await;
                *completed.lock().unwrap() = Some(checked);
                macroquad::window::miniquad_context().request_quit();
            },
        );
        let checked = result
            .lock()
            .unwrap()
            .take()
            .context("Metal loop did not report dynamic BPM assertions")?;
        checked
    }

    #[test]
    fn apple_rhythm_real_pez_variable_bpm_and_fixed_speed() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let fixtures = vec![
            fixture(directory.path(), "dynamic-rpe", &rpe_json())?,
            fixture(
                directory.path(),
                "fixed-150-pgr-speed-events",
                &pgr_speed_json(),
            )?,
        ];
        let fixed = reference_fixtures(directory.path())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let _runtime = runtime.enter();
        run_metal(fixtures, fixed)?;
        println!("APPLE_DYNAMIC_BPM_REAL_LOADER_METAL_PASS");
        Ok(())
    }

    // Decode the actual native export with Apple's own reader. This exercises
    // apple_render::run, LoadingScene/GameScene's clock and MetalFrames, rather
    // than substituting a test frame loop or a mirrored BPM calculation.
    mod media {
        use super::{
            digit_mask, guide_spacing, read_bpm, ring_luma, DigitMask, BPMS, OUTPUT_HEIGHT, WIDTH,
        };
        use crate::apple_media::Time;
        use anyhow::{ensure, Context, Result};
        use objc::{
            class, msg_send,
            runtime::{Object, NO, YES},
            sel, sel_impl, Encode, Encoding,
        };
        use serde_json::{json, Value};
        use std::{ffi::c_void, io::Write, path::Path, ptr::null_mut};

        type Ref = *mut c_void;
        const FPS: u64 = 30;
        const RATE: u64 = 48_000;

        #[repr(C)]
        struct TimeRange {
            start: Time,
            duration: Time,
        }
        unsafe impl Encode for TimeRange {
            fn encode() -> Encoding {
                unsafe { Encoding::from_str("{CMTimeRange={CMTime=qiIq}{CMTime=qiIq}}") }
            }
        }

        #[cfg_attr(
            any(target_os = "macos", target_os = "ios"),
            link(name = "CoreFoundation", kind = "framework")
        )]
        extern "C" {
            fn CFRelease(value: Ref);
        }
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios"),
            link(name = "CoreMedia", kind = "framework")
        )]
        extern "C" {
            fn CMSampleBufferIsValid(sample: Ref) -> u8;
            fn CMSampleBufferGetNumSamples(sample: Ref) -> i64;
            fn CMSampleBufferGetPresentationTimeStamp(sample: Ref) -> Time;
            fn CMSampleBufferGetImageBuffer(sample: Ref) -> Ref;
            fn CMSampleBufferGetDataBuffer(sample: Ref) -> Ref;
            fn CMTimeGetSeconds(time: Time) -> f64;
            fn CMBlockBufferGetDataLength(block: Ref) -> usize;
            fn CMBlockBufferCopyDataBytes(
                block: Ref,
                offset: usize,
                length: usize,
                output: Ref,
            ) -> i32;
            fn CMFormatDescriptionGetMediaSubType(description: Ref) -> u32;
        }
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios"),
            link(name = "CoreVideo", kind = "framework")
        )]
        extern "C" {
            fn CVPixelBufferLockBaseAddress(pixel: Ref, flags: u64) -> i32;
            fn CVPixelBufferUnlockBaseAddress(pixel: Ref, flags: u64) -> i32;
            fn CVPixelBufferGetBaseAddress(pixel: Ref) -> Ref;
            fn CVPixelBufferGetBytesPerRow(pixel: Ref) -> usize;
            fn CVPixelBufferGetWidth(pixel: Ref) -> usize;
            fn CVPixelBufferGetHeight(pixel: Ref) -> usize;
            fn CVPixelBufferGetPixelFormatType(pixel: Ref) -> u32;
            static kCVPixelBufferPixelFormatTypeKey: Ref;
        }

        struct Obj(*mut Object);
        impl Drop for Obj {
            fn drop(&mut self) {
                unsafe {
                    let _: () = msg_send![self.0, release];
                }
            }
        }
        struct Cf(Ref);
        impl Drop for Cf {
            fn drop(&mut self) {
                unsafe {
                    CFRelease(self.0);
                }
            }
        }

        unsafe fn string(value: &str) -> Obj {
            let object: *mut Object = msg_send![class!(NSString), alloc];
            Obj(msg_send![object, initWithBytes:value.as_ptr() length:value.len() encoding:4usize])
        }
        unsafe fn number(value: i64) -> Obj {
            let object: *mut Object = msg_send![class!(NSNumber), numberWithLongLong:value];
            let _: () = msg_send![object, retain];
            Obj(object)
        }
        unsafe fn dictionary(entries: &[(&str, &Obj)]) -> Obj {
            let keys: Vec<_> = entries.iter().map(|(key, _)| string(key)).collect();
            let key_pointers: Vec<_> = keys.iter().map(|key| key.0).collect();
            let values: Vec<_> = entries.iter().map(|(_, value)| value.0).collect();
            let object: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObjects:values.as_ptr() forKeys:key_pointers.as_ptr() count:values.len()];
            let _: () = msg_send![object, retain];
            Obj(object)
        }
        unsafe fn reader(
            asset: *mut Object,
            track: *mut Object,
            settings: *mut Object,
        ) -> Result<(Obj, Obj)> {
            let allocated: *mut Object = msg_send![class!(AVAssetReader), alloc];
            let mut error: *mut Object = null_mut();
            let object: *mut Object = msg_send![allocated, initWithAsset:asset error:&mut error];
            ensure!(!object.is_null(), "cannot create native media reader");
            let reader = Obj(object);
            let allocated: *mut Object = msg_send![class!(AVAssetReaderTrackOutput), alloc];
            let object: *mut Object =
                msg_send![allocated, initWithTrack:track outputSettings:settings];
            ensure!(!object.is_null(), "cannot create native media output");
            let output = Obj(object);
            let can: objc::runtime::BOOL = msg_send![reader.0, canAddOutput:output.0];
            ensure!(can == YES, "reader rejected the output");
            let _: () = msg_send![reader.0, addOutput:output.0];
            let started: objc::runtime::BOOL = msg_send![reader.0, startReading];
            ensure!(started == YES, "reader did not start");
            Ok((reader, output))
        }
        unsafe fn codec(track: *mut Object) -> Result<u32> {
            let descriptions: *mut Object = msg_send![track, formatDescriptions];
            let count: usize = msg_send![descriptions, count];
            ensure!(count > 0, "track has no codec description");
            let description: Ref = msg_send![descriptions, objectAtIndex:0usize];
            Ok(CMFormatDescriptionGetMediaSubType(description))
        }

        unsafe fn rgba(sample: Ref) -> Result<Vec<u8>> {
            let pixel = CMSampleBufferGetImageBuffer(sample);
            ensure!(!pixel.is_null(), "H.264 sample has no decoded image");
            ensure!(
                CVPixelBufferGetPixelFormatType(pixel) == 0x42475241,
                "decoder did not return BGRA"
            );
            ensure!(
                CVPixelBufferGetWidth(pixel) == WIDTH as usize
                    && CVPixelBufferGetHeight(pixel) == OUTPUT_HEIGHT as usize,
                "native output dimensions do not include the rhythm strip"
            );
            ensure!(
                CVPixelBufferLockBaseAddress(pixel, 1) == 0,
                "cannot read decoded pixels"
            );
            let base = CVPixelBufferGetBaseAddress(pixel) as *const u8;
            let stride = CVPixelBufferGetBytesPerRow(pixel);
            if base.is_null() || stride < WIDTH as usize * 4 {
                let _ = CVPixelBufferUnlockBaseAddress(pixel, 1);
                anyhow::bail!("decoded image has an invalid base address/row stride");
            }
            let mut result = vec![0; (WIDTH * OUTPUT_HEIGHT * 4) as usize];
            for y in 0..OUTPUT_HEIGHT as usize {
                let row = std::slice::from_raw_parts(base.add(y * stride), WIDTH as usize * 4);
                for (source, destination) in row.chunks_exact(4).zip(
                    result[y * WIDTH as usize * 4..(y + 1) * WIDTH as usize * 4]
                        .chunks_exact_mut(4),
                ) {
                    destination.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
                }
            }
            ensure!(
                CVPixelBufferUnlockBaseAddress(pixel, 1) == 0,
                "cannot unlock decoded pixels"
            );
            Ok(result)
        }

        fn ppm(path: &Path, rgba: &[u8]) -> Result<()> {
            let mut file = std::fs::File::create(path)?;
            write!(file, "P6\n{WIDTH} {OUTPUT_HEIGHT}\n255\n")?;
            for pixel in rgba.chunks_exact(4) {
                file.write_all(&pixel[..3])?;
            }
            Ok(())
        }

        unsafe fn video(
            asset: *mut Object,
            track: *mut Object,
            references: &[DigitMask; 4],
            artifacts: Option<&Path>,
            name: &str,
            dynamic: bool,
        ) -> Result<Value> {
            ensure!(
                codec(track)? == u32::from_be_bytes(*b"avc1"),
                "native output is not H.264"
            );
            let format = number(0x42475241);
            let settings: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject:format.0 forKey:kCVPixelBufferPixelFormatTypeKey];
            let (reader, output) = reader(asset, track, settings)?;
            let mut frames = 0;
            let mut stage = if dynamic { 0 } else { 3 };
            let mut stage_frames = [0u64; 4];
            let mut transitions = Vec::new();
            let mut gaps = [0.; 4];
            let mut ring = Vec::new();
            loop {
                let next = objc::rc::autoreleasepool(|| -> Result<Option<(f64, u32, Vec<u8>)>> {
                    let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
                    if sample.is_null() {
                        return Ok(None);
                    }
                    let sample = Cf(sample);
                    ensure!(
                        CMSampleBufferIsValid(sample.0) != 0
                            && CMSampleBufferGetNumSamples(sample.0) == 1,
                        "invalid decoded video sample"
                    );
                    let pts = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample.0));
                    let bytes = rgba(sample.0)?;
                    let bpm = read_bpm(&digit_mask(&bytes), references)?;
                    Ok(Some((pts, bpm, bytes)))
                })?;
                let Some((pts, bpm, bytes)) = next else {
                    break;
                };
                ensure!(
                    (pts - frames as f64 / FPS as f64).abs() < 1e-5,
                    "native video PTS gap/reorder at {frames}: {pts}"
                );
                if bpm != BPMS[stage] {
                    ensure!(
                        dynamic && stage < 2 && bpm == BPMS[stage + 1],
                        "native BPM sequence jumped or moved backward: frame={frames}, bpm={bpm}"
                    );
                    stage += 1;
                    transitions.push(pts);
                }
                stage_frames[stage] += 1;
                if stage_frames[stage] == 4 {
                    gaps[stage] = guide_spacing(&bytes, bpm)?;
                    if let Some(directory) = artifacts {
                        ppm(&directory.join(format!("{name}-{bpm}.ppm")), &bytes)?;
                    }
                }
                ring.push(ring_luma(&bytes));
                frames += 1;
            }
            let state: isize = msg_send![reader.0, status];
            ensure!(state == 2, "native video decoding failed");
            ensure!(
                frames == 8 * FPS,
                "native six-second fixture dropped/duplicated output frames: {frames}"
            );
            let mut pulse_evidence = Value::Null;
            if dynamic {
                ensure!(
                    stage == 2
                        && stage_frames[..3].iter().all(|&count| count >= FPS)
                        && stage_frames[3] == 0,
                    "actual native video did not display all tempo segments: {stage_frames:?}"
                );
                ensure!(transitions.len() == 2, "missing native tempo transitions");
            } else {
                ensure!(
                    stage == 3 && stage_frames[3] == frames && transitions.is_empty(),
                    "scrolling speed incorrectly changed the fixed 150 BPM clock"
                );
                // 150 BPM is exactly twelve 30-FPS frames per musical beat.
                // Check an actual luminous cycle across both speed-event changes,
                // rather than just noticing that two video frames differ.
                let active = &ring[FPS as usize..(5 * FPS) as usize];
                let brightest = active.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let darkest = active.iter().copied().fold(f64::INFINITY, f64::min);
                ensure!(
                    brightest - darkest >= 8.,
                    "decoded fixed-150 metronome did not flash: {brightest}/{darkest}"
                );
                let differences: Vec<_> = active
                    .iter()
                    .zip(&active[12..])
                    .map(|(a, b)| (a - b).abs())
                    .collect();
                let period_error = differences.iter().sum::<f64>() / differences.len() as f64;
                ensure!(
                    period_error <= 3.,
                    "scrolling speed altered the .4s/12-frame musical pulse: {period_error}"
                );
                let mut shorter_periods = Vec::new();
                for lag in [1usize, 2, 3, 4, 6] {
                    let error = active
                        .iter()
                        .zip(&active[lag..])
                        .map(|(a, b)| (a - b).abs())
                        .sum::<f64>()
                        / (active.len() - lag) as f64;
                    ensure!(
                        error > 0.10 * (brightest - darkest),
                        "metronome repeated after only {lag} frames instead of twelve: {error}"
                    );
                    shorter_periods.push(json!({"lag_frames": lag, "mean_absolute_error": error}));
                }
                pulse_evidence = json!({"period_frames": 12, "mean_absolute_error": period_error,
                    "luminous_range": brightest - darkest, "excluded_shorter_periods": shorter_periods});
            }
            // The production loop may quantize each Loading/Game transition
            // by a frame. The authored chart has a genuine +125ms offset:
            // ignoring it exceeds this two-frame tolerance at 30 FPS.
            for (&actual, expected) in transitions.iter().zip([2.125, 4.125]) {
                ensure!((actual - expected).abs() <= 2. / FPS as f64 + 1e-5,
                    "native Scene chart clock changed BPM at wrong video time: {actual}, expected {expected}");
            }
            let range: TimeRange = msg_send![track, timeRange];
            ensure!(
                CMTimeGetSeconds(range.start).abs() < 1e-5
                    && (CMTimeGetSeconds(range.duration) - 8.).abs() < 1e-5,
                "native stored video duration/PTS differs"
            );
            Ok(json!({"codec": "h264", "frames": frames, "fps": FPS,
                "bpm_stage_frames": stage_frames, "bpm_transition_video_seconds": transitions,
                "expected_chart_nodes_seconds": if dynamic { vec![2, 4] } else { vec![] }, "chart_offset_seconds": 0.125,
                "expected_transition_video_seconds": if dynamic { vec![2.125, 4.125] } else { vec![] },
                "fixed_bpm_pulse": pulse_evidence, "guide_spacing_pixels": gaps}))
        }

        unsafe fn audio(asset: *mut Object, track: *mut Object) -> Result<Value> {
            ensure!(
                codec(track)? == u32::from_be_bytes(*b"aac "),
                "native output audio is not AAC"
            );
            let value: *mut Object = msg_send![class!(NSNumber), numberWithBool:NO];
            let _: () = msg_send![value, retain];
            let no = Obj(value);
            let settings = dictionary(&[
                ("AVFormatIDKey", &number(0x6c70636d)),
                ("AVLinearPCMBitDepthKey", &number(16)),
                ("AVLinearPCMIsFloatKey", &no),
                ("AVLinearPCMIsBigEndianKey", &no),
                ("AVLinearPCMIsNonInterleaved", &no),
            ]);
            let (reader, output) = reader(asset, track, settings.0)?;
            let mut frames = 0u64;
            let mut audible = 0usize;
            loop {
                let next = objc::rc::autoreleasepool(|| -> Result<Option<(u64, usize)>> {
                    let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
                    if sample.is_null() {
                        return Ok(None);
                    }
                    let sample = Cf(sample);
                    ensure!(CMSampleBufferIsValid(sample.0) != 0, "invalid AAC sample");
                    let count = CMSampleBufferGetNumSamples(sample.0);
                    ensure!(count >= 0, "negative decoded AAC sample count");
                    if count == 0 {
                        return Ok(Some((0, 0)));
                    }
                    let block = CMSampleBufferGetDataBuffer(sample.0);
                    ensure!(!block.is_null(), "decoded AAC has no PCM");
                    let mut bytes = vec![0; CMBlockBufferGetDataLength(block).min(16 * 1024)];
                    ensure!(
                        CMBlockBufferCopyDataBytes(
                            block,
                            0,
                            bytes.len(),
                            bytes.as_mut_ptr() as Ref
                        ) == 0,
                        "cannot read AAC PCM"
                    );
                    let audible = bytes
                        .chunks_exact(4)
                        .filter(|frame| {
                            let left = i16::from_le_bytes([frame[0], frame[1]]);
                            let right = i16::from_le_bytes([frame[2], frame[3]]);
                            left.unsigned_abs() > 100
                                && right.unsigned_abs() > 100
                                && (left < 0) != (right < 0)
                        })
                        .count();
                    Ok(Some((count as u64, audible)))
                })?;
                let Some((count, nonzero)) = next else {
                    break;
                };
                frames += count;
                audible += nonzero;
            }
            let state: isize = msg_send![reader.0, status];
            ensure!(state == 2, "native AAC decoding failed");
            ensure!(
                frames.abs_diff(8 * RATE) <= 4096,
                "native AAC duration differs: {frames} PCM frames"
            );
            ensure!(
                audible >= RATE as usize,
                "synthetic music was missing/silent or lost stereo polarity: {audible}"
            );
            Ok(
                json!({"codec": "aac", "decoded_pcm_frames": frames, "audible_opposite_polarity_stereo_frames": audible}),
            )
        }

        pub(super) fn verify(
            path: &Path,
            references: &[DigitMask; 4],
            artifacts: Option<&Path>,
            name: &str,
            dynamic: bool,
        ) -> Result<Value> {
            objc::rc::autoreleasepool(|| unsafe {
                let path_string = string(&path.to_string_lossy());
                let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath:path_string.0];
                let asset: *mut Object =
                    msg_send![class!(AVURLAsset), URLAssetWithURL:url options:null_mut::<Object>()];
                ensure!(!asset.is_null(), "cannot open native BPM MP4");
                let _: () = msg_send![asset, retain];
                let asset = Obj(asset);
                let video_name = string("vide");
                let audio_name = string("soun");
                let videos: *mut Object = msg_send![asset.0, tracksWithMediaType:video_name.0];
                let audios: *mut Object = msg_send![asset.0, tracksWithMediaType:audio_name.0];
                let video_count: usize = msg_send![videos, count];
                let audio_count: usize = msg_send![audios, count];
                ensure!(
                    video_count == 1 && audio_count == 1,
                    "native MP4 must contain one H.264 and one AAC track"
                );
                let video = video(
                    asset.0,
                    msg_send![videos, objectAtIndex:0usize],
                    references,
                    artifacts,
                    name,
                    dynamic,
                )
                .context("actual native BPM video assertions")?;
                let audio = audio(asset.0, msg_send![audios, objectAtIndex:0usize])
                    .context("actual native AAC assertions")?;
                Ok(json!({"video": video, "audio": audio}))
            })
        }
    }

    #[test]
    fn apple_rhythm_native_variable_and_fixed_bpm_mp4() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let fixed = reference_fixtures(directory.path())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let _runtime = runtime.enter();
        // Only create glyph references here; the separate real-loader Metal
        // test covers RPE tempo nodes and fixed-PGR scrolling without repetition.
        let references = run_metal(Vec::new(), fixed)?;
        let artifacts = std::env::var_os("PHI_TKC_APPLE_SMOKE_ARTIFACTS").map(PathBuf::from);
        if let Some(path) = &artifacts {
            std::fs::create_dir_all(path)?;
        }
        for (name, chart, dynamic) in [
            ("dynamic-bpm-rpe", rpe_json(), true),
            ("dynamic-bpm-rpe-legacy-gl", rpe_json(), true),
            ("fixed-150-pgr-speed-events", pgr_speed_json(), false),
        ] {
            let path = fixture(directory.path(), name, &chart)?;
            let mut fs = prpr::fs::fs_from_file(&path)?;
            let info = runtime.block_on(prpr::fs::load_info(fs.as_mut()))?;
            let output = artifacts
                .as_deref()
                .unwrap_or(directory.path())
                .join(format!("{name}.mp4"));
            let mut config = crate::render::RenderConfig::default();
            config.resolution = (WIDTH, GAME_HEIGHT);
            config.fps = 30;
            config.target_audio = 48_000;
            config.sample_count = 4;
            config.disable_loading = true;
            config.ending_length = 0.;
            config.video_codec = "h264".to_owned();
            config.bitrate_control = "CBR".to_owned();
            config.bitrate = "8M".to_owned();
            config.volume_sfx = 0.;
            config.particle = false;
            config.rhythm_analysis = true;
            config.rhythm_tap = false;
            config.rhythm_drag = false;
            config.rhythm_hold = false;
            config.rhythm_flick = false;
            config.rhythm_colors = Some(palette());
            crate::apple_render::run(
                crate::apple_render::RenderRequest {
                    params: crate::render::RenderParams { path, info, config },
                    output_path: output.clone(),
                    asset_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"),
                    request_cancel: None,
                },
                |_, _| {},
            )?;
            let evidence =
                media::verify(&output, &references, artifacts.as_deref(), name, dynamic)?;
            if let Some(directory) = &artifacts {
                std::fs::write(
                    directory.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(&evidence)?,
                )?;
            }
            println!("APPLE_DYNAMIC_BPM_NATIVE_MP4_PASS fixture={name} {evidence}");
        }
        Ok(())
    }
}

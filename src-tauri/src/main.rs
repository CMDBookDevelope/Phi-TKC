// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{CommandFactory, Parser};
use app_lib::render::{render_cli, CliArgs};

fn build_conf() -> macroquad::window::Conf {
    // The Vulkan renderer needs a real Vulkan context, and miniquad's headless
    // path is EGL/OpenGL only. Only the Tauri GUI process stays headless; the
    // render/preview subprocesses (and the CLI directly) open a window, which
    // also works under `xvfb-run` on servers.
    let first = std::env::args().skip(1).next();
    let is_preview = first.as_deref() == Some("preview");
    let is_render = first.as_deref() == Some("render");
    macroquad::window::Conf {
        window_title: "Phi-TKC (GUI/CLI)".to_string(),
        window_width: 1920,
        window_height: 1440,
        headless: !(is_preview || is_render || has_cli_intent()),
        platform: macroquad::miniquad::conf::Platform {
            rendering_backend: macroquad::miniquad::conf::RenderingBackend::Vulkan,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Flags that mean "the user wanted the CLI". `--help`/`--version` are included so they
/// are not swallowed by the GUI fallback (previously `Phi-TKC --help` opened the GUI).
const CLI_FLAGS: &[&str] = &[
    "-i",
    "--input",
    "-o",
    "--output",
    "-r",
    "--resolution",
    "-c",
    "--crf",
    "--fps",
    "-d",
    "--dark",
    "-l",
    "--load",
    "--finish",
    "-a",
    "--assets",
    "-h",
    "--help",
    "-V",
    "--version",
];

fn has_cli_intent() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The GUI process spawns itself with these internal subcommands; they are not CLI.
    if matches!(args.first().map(String::as_str), Some("render") | Some("preview")) {
        return false;
    }
    args.iter().any(|arg| {
        CLI_FLAGS
            .iter()
            .any(|flag| arg == flag || arg.strip_prefix(flag).is_some_and(|rest| rest.starts_with('=')))
    })
}

fn main() {
    // Handle help/version before macroquad/miniquad initializes: its startup prints
    // GL_VERSION and the renderer name to stdout, which would corrupt CLI output.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        let _ = CliArgs::command().print_help();
        println!();
        return;
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        print!("{}", CliArgs::command().render_version());
        return;
    }

    macroquad::Window::from_config(build_conf(), async {
        if let Err(err) = run_app().await {
            // A failed CLI render must exit non-zero for scripts; macroquad's attribute
            // macro used to ignore the returned Result and exit with status 0.
            eprintln!("Error: {err:?}");
            std::process::exit(1);
        }
    });
}

async fn run_app() -> Result<(), anyhow::Error> {
    match CliArgs::try_parse() {
        Ok(cli_args) => {
            // 创建独立的 Tokio 运行时，执行渲染
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(render_cli(cli_args))?;
            Ok(())
        }
        Err(e) => {
            if has_cli_intent() {
                // Clap prints usage errors on stderr (exit 2), which is what a CLI user
                // expects. Help/version already returned above.
                e.exit();
            }
            // GUI 模式，启动原有界面
            app_lib::run().await?;
            Ok(())
        }
    }
}

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{CommandFactory, Parser};
use app_lib::render::{render_cli, CliArgs};

fn build_conf() -> macroquad::window::Conf {
    // Everything except the preview window is headless. The Vulkan backend now
    // builds a surface-less context (`VulkanContext::new_headless`), so the CLI
    // and the `render` subprocess never create an X11/Wayland window.
    let is_preview = std::env::args().skip(1).next().as_deref() == Some("preview");
    macroquad::window::Conf {
        window_title: "Phi-TKC (GUI/CLI)".to_string(),
        window_width: 1920,
        window_height: 1440,
        headless: !is_preview,
        platform: macroquad::miniquad::conf::Platform {
            rendering_backend: macroquad::miniquad::conf::RenderingBackend::Vulkan,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Flags that mean "the user wanted the CLI". `--help`/`--version` are included so they
/// are not swallowed by the GUI fallback (previously `Phi-TKC --help` opened the GUI).
fn has_cli_intent() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The GUI process spawns itself with these internal subcommands; they are not CLI.
    if matches!(
        args.first().map(String::as_str),
        Some("render") | Some("preview") | Some("server")
    ) {
        return false;
    }
    // Any `-`/`--` flag means the user wanted the CLI. macOS Finder passes
    // `-psn_...` when opening the .app, which must not be mistaken for a flag.
    args.iter()
        .any(|arg| arg.starts_with('-') && !arg.starts_with("-psn_"))
}

fn main() {
    // Handle help/version before macroquad/miniquad initializes: its startup prints
    // GL_VERSION and the renderer name to stdout, which would corrupt CLI output.
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `phi-tkc server ...` must not initialise miniquad: it is a pure HTTP process
    // that spawns the existing `render` child for each job.
    if args.first().map(String::as_str) == Some("server") {
        // Drop the `server` subcommand token: ServerArgs only describes its own flags.
        let mut argv: Vec<String> = std::env::args().collect();
        if argv.len() > 1 {
            argv.remove(1);
        }
        let server_args = app_lib::server::ServerArgs::parse_from(argv);
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                eprintln!("cannot start the runtime: {err:?}");
                std::process::exit(1);
            }
        };
        if let Err(err) = rt.block_on(app_lib::server::run(server_args)) {
            eprintln!("server error: {err:?}");
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        let _ = CliArgs::command().print_help();
        println!();
        return;
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        print!("{}", CliArgs::command().render_version());
        return;
    }

    // Parse before creating the window: a typo like `--dark` must produce a clap
    // error (exit 2) even on a machine without a display, instead of failing inside
    // miniquad's X11/Wayland init.
    let cli_args = if has_cli_intent() {
        match CliArgs::try_parse() {
            Ok(args) => Some(args),
            Err(e) => e.exit(),
        }
    } else {
        None
    };

    macroquad::Window::from_config(build_conf(), async move {
        let result = match cli_args {
            Some(cli_args) => {
                // A separate runtime: rendering blocks on tokio and must not share
                // the GUI's runtime.
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(err) => {
                        eprintln!("cannot start the runtime: {err:?}");
                        std::process::exit(1);
                    }
                };
                rt.block_on(render_cli(cli_args))
            }
            // GUI mode.
            None => app_lib::run().await,
        };
        if let Err(err) = result {
            // A failed CLI render must exit non-zero for scripts; macroquad's attribute
            // macro used to ignore the returned Result and exit with status 0.
            eprintln!("Error: {err:?}");
            std::process::exit(1);
        }
    });
}

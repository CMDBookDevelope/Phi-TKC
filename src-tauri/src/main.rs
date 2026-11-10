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
    #[cfg(windows)]
    unsafe {
        // Keep the GUI console hidden, but let CLI/server calls use their parent
        // terminal even though the release executable uses the Windows subsystem.
        if has_cli_intent()
            || matches!(std::env::args().nth(1).as_deref(), Some("server") | Some("render") | Some("preview"))
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn AttachConsole(process_id: u32) -> i32;
                fn GetStdHandle(id: u32) -> *mut std::ffi::c_void;
                fn SetStdHandle(id: u32, handle: *mut std::ffi::c_void) -> i32;
                fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
            }
            let handles = [(-10_i32) as u32, (-11_i32) as u32, (-12_i32) as u32]
                .map(|id| (id, GetStdHandle(id)));
            AttachConsole(u32::MAX);
            // Attaching can replace standard handles. Keep redirected files and
            // pipes intact, especially the render child's JSON IPC streams.
            for (id, handle) in handles {
                if matches!(GetFileType(handle), 1 | 3) {
                    SetStdHandle(id, handle);
                }
            }
        }
    }
    app_lib::logger::init();
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
        let server_args = app_lib::server::ServerArgs::try_parse_from(argv)
            .unwrap_or_else(|error| app_lib::logger::clap_error(error));
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                log::error!("cannot start the runtime: {err:?}");
                std::process::exit(1);
            }
        };
        if let Err(err) = rt.block_on(app_lib::server::run(server_args)) {
            log::error!("server error: {err:?}");
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        log::info!("{}", CliArgs::command().render_help());
        return;
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        log::info!("{}", CliArgs::command().render_version().trim_end());
        return;
    }

    // Parse before creating the window: a typo like `--dark` must produce a clap
    // error (exit 2) even on a machine without a display, instead of failing inside
    // miniquad's X11/Wayland init.
    let cli_args = if has_cli_intent() {
        match CliArgs::try_parse() {
            Ok(args) => Some(args),
            Err(e) => app_lib::logger::clap_error(e),
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
                        log::error!("cannot start the runtime: {err:?}");
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
            log::error!("Error: {err:?}");
            std::process::exit(1);
        }
    });
}

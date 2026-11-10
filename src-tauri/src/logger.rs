//! Shared formatting for terminal diagnostics, including early startup failures.
use colored::Colorize;
use log::Level;
use std::{io::Write, sync::Once};

static INIT: Once = Once::new();

pub fn prefix(level: Level) -> String {
    let (face, label) = match level {
        Level::Error => ("(｀皿´＃).", "ERROR!".red()),
        Level::Warn => ("(╬ Ò﹏Ó).", "WARN!".yellow()),
        Level::Info => ("(*・ω・)っ.", "INFO♪".cyan()),
        Level::Debug => ("≽^•⩊•^≼.", "DEBUG:".green()),
        Level::Trace => ("(◎_◎).", "TRACE~".magenta()),
    };
    format!(
        "{}{}{}",
        face.bright_black(),
        label.bold(),
        " ♡ ".bright_magenta()
    )
}

pub fn init() {
    INIT.call_once(|| {
        let mut builder =
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
        builder.filter(Some("symphonia"), log::LevelFilter::Warn);
        builder.target(env_logger::Target::Stdout);
        builder.format(|buf, record| {
            let prefix = prefix(record.level());
            // Errors, panic backtraces and help text can span several lines.
            for line in record.args().to_string().split('\n') {
                writeln!(buf, "{prefix}{line}")?;
            }
            Ok(())
        });
        let _ = builder.try_init();
        std::panic::set_hook(Box::new(|panic| {
            log::error!("{panic}\n{}", std::backtrace::Backtrace::force_capture());
        }));
    });
}

pub fn clap_error(error: clap::Error) -> ! {
    let code = error.exit_code();
    if error.use_stderr() {
        log::error!("{}", error.render());
    } else {
        log::info!("{}", error.render());
    }
    std::process::exit(code);
}

/// Relay child records at their original severity, with one logger prefix.
pub fn relay(context: &str, line: &str, fallback: Level) {
    let (level, message) = match line.split_once(" ♡ ") {
        Some((head, message)) => {
            let level = if head.contains("ERROR!") {
                Level::Error
            } else if head.contains("WARN!") {
                Level::Warn
            } else if head.contains("INFO♪") {
                Level::Info
            } else if head.contains("DEBUG:") {
                Level::Debug
            } else if head.contains("TRACE~") {
                Level::Trace
            } else {
                fallback
            };
            (level, message.trim_start_matches("\x1b[0m"))
        }
        None => (fallback, line),
    };
    log::log!(level, "{context}: {message}");
}

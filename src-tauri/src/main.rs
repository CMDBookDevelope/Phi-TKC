mod render;
use clap::Parser;
use render::{render_cli, CliArgs};

fn build_conf() -> macroquad::window::Conf {
    let is_cli = std::env::args().any(|arg| arg == "--input" || arg == "-i");
    macroquad::window::Conf {
        window_title: "Phi-TKC (GUI/CLI)".to_string(),
        window_width: 1920,
        window_height: 1440,
        headless: is_cli,
        ..Default::default()
    }
}

#[macroquad::main(build_conf)]
async fn main() -> Result<(), anyhow::Error> {
    match CliArgs::try_parse() {
        Ok(cli_args) => {
            // 创建独立的 Tokio 运行时，执行渲染
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(render_cli(cli_args))?;
            Ok(())
        }
        Err(e) => {
            let has_input = std::env::args().any(|arg| arg == "--input" || arg == "-i");
            if has_input {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            // GUI 模式，启动原有界面
            app_lib::run().await?;
            Ok(())
        }
    }
}

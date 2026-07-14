mod render;
use clap::Parser;
use render::{render_cli, CliArgs};

/// 根据是否包含 CLI 参数决定是否 headless
fn build_conf() -> macroquad::window::Conf {
    let is_cli = std::env::args().any(|arg| arg == "--input" || arg == "-i");
    macroquad::window::Conf {
        window_title: "Phi-TK (GUI/CLI)".to_string(),
        window_width: 1280,
        window_height: 720,
        headless: is_cli,          // CLI 模式不显示窗口
        ..Default::default()
    }
}

#[macroquad::main(build_conf)]
async fn main() -> Result<(), anyhow::Error> {
    // 尝试解析 CLI 参数
    match CliArgs::try_parse() {
        Ok(cli_args) => {
            // 成功解析 → 执行 CLI 渲染
            render_cli(cli_args).await?;
            Ok(())
        }
        Err(e) => {
            // 如果用户提供了 --input 但参数错误，报错退出
            let has_input = std::env::args().any(|arg| arg == "--input" || arg == "-i");
            if has_input {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            // 否则启动 GUI
            app_lib::run().await?;
            Ok(())
        }
    }
}

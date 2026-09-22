//! Dr.COM WebUI 启动器。
//!
//! 只负责解析参数、启动 [`drcom::webui::WebUi`] 并打开浏览器。

use drcom::webui::WebUi;

fn main() {
    let (port, open_browser) = parse_args();

    let webui = match WebUi::start(&format!("127.0.0.1:{port}")) {
        Ok(webui) => webui,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    println!("Dr.COM WebUI: {}", webui.url());
    println!("按 Ctrl+C 退出。");

    if open_browser && let Err(e) = webui.open_browser() {
        eprintln!("无法自动打开浏览器: {e}");
    }

    loop {
        std::thread::park();
    }
}

fn parse_args() -> (u16, bool) {
    let mut port = 0u16;
    let mut open_browser = true;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                if let Some(value) = args.next() {
                    port = value.parse().unwrap_or(0);
                }
            }
            "--no-open" => open_browser = false,
            "-h" | "--help" => {
                println!("Usage: drcom-web [--port <PORT>] [--no-open]");
                std::process::exit(0);
            }
            _ => {}
        }
    }

    (port, open_browser)
}

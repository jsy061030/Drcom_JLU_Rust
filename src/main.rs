//! Dr.COM 客户端命令行入口。
//!
//! 该二进制是对 [`drcom`] 库的薄封装：加载配置、创建 [`Client`] 并运行。
//! 运行中按 `q` 键可询问是否注销，再按 `y` 确认注销。

use drcom::{Client, Config};
use std::env;
use std::path::{Path, PathBuf};
use std::process;

fn sample_config() -> &'static str {
    r#"server = "10.100.61.3"
username = "your_username"
password = "your_password"
host_ip = "192.168.1.2"
mac = "AA:BB:CC:DD:EE:FF"

# 以下为可选项，保持默认即可
# host_name = "YOURPCNAME"
# primary_dns = "10.10.10.10"
# dhcp_server = "0.0.0.0"
# bind_ip = "0.0.0.0"
# is_test = true
"#
}

/// 非 Unix 平台下为空操作；Unix 下将 PID 写入 `/var/run/jludrcom.pid`。
#[cfg(unix)]
fn daemon() -> std::io::Result<()> {
    std::fs::write("/var/run/jludrcom.pid", process::id().to_string())
}

#[cfg(not(unix))]
fn daemon() -> std::io::Result<()> {
    Ok(())
}

/// 解析配置文件路径：
/// - 若命令行提供了参数，使用该路径；
/// - 否则使用可执行文件所在目录下的 `drcom.toml`。
fn resolve_config_path(args: &[String]) -> PathBuf {
    if let Some(path) = args.get(1) {
        return Path::new(path).to_path_buf();
    }
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("drcom.toml")))
        .unwrap_or_else(|| PathBuf::from("drcom.toml"))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let config_path = resolve_config_path(&args);

    if !config_path.exists() {
        eprintln!("config file not found: {}", config_path.display());
        eprintln!("sample config:\n{}", sample_config());
        process::exit(1);
    }

    let config = match Config::from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to load config: {e}");
            process::exit(1);
        }
    };

    if !config.is_test
        && let Err(e) = daemon()
    {
        eprintln!("daemon failed: {e}");
    }

    println!(
        "auth svr: {}\nusername: {}\nmac: {}\nbind ip: {}",
        config.server, config.username, config.mac, config.bind_ip
    );
    println!("按 q 键可注销当前会话。");

    let verbose = config.is_test;
    let client = match Client::new(config) {
        Ok(c) => c.with_verbose(verbose),
        Err(e) => {
            eprintln!("failed to create client: {e}");
            process::exit(1);
        }
    };

    match client.run() {
        Ok(()) => println!("已退出。"),
        Err(e) => {
            eprintln!("运行失败: {e}");
            process::exit(1);
        }
    }
}

//! # drcom
//!
//! Dr.COM 校园网认证客户端库。
//!
//! 本库提供 Dr.COM 协议的核心实现，包括：
//! - 配置文件解析
//! - 协议报文构造（Challenge、Login、KeepAlive）
//! - UDP 通信与保活循环
//!
//! ## 使用示例
//!
//! ```no_run
//! use drcom::{Config, Client};
//!
//! fn main() -> std::io::Result<()> {
//!     let config = Config::from_file("drcom.toml")?;
//!     let client = Client::new(config)?;
//!     client.run()?;
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod config;
pub mod ffi;
pub mod protocol;

pub use client::Client;
pub use config::Config;

//! 配置解析模块。
//!
//! 提供 [`Config`] 结构体用于从 TOML 文件加载 Dr.COM 客户端配置，
//! 以及辅助方法用于解析 MAC 地址、IP 地址和十六进制字符串。

use serde::Deserialize;
use std::fs;
use std::io;
use std::path::Path;

/// Dr.COM 客户端配置。
///
/// 认证信息（server / username / password / host_ip / mac）必须提供；
/// 其余协议字段若省略，则使用与原版脚本一致的默认值。
///
/// # 示例
///
/// ```toml
/// server = "10.100.61.3"
/// username = "your_username"
/// password = "your_password"
/// host_ip = "192.168.1.2"
/// mac = "AA:BB:CC:DD:EE:FF"
/// ```
#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: String,
    pub username: String,
    pub password: String,
    pub host_ip: String,
    pub mac: String,
    #[serde(default = "default_host_name")]
    pub host_name: String,
    #[serde(default = "default_primary_dns")]
    pub primary_dns: String,
    #[serde(default = "default_dhcp_server")]
    pub dhcp_server: String,
    #[serde(default = "default_bind_ip")]
    pub bind_ip: String,

    // 协议固定字段，使用 hex 字符串表示，例如 "20"
    #[serde(default = "default_control_check_status")]
    pub control_check_status: String,
    #[serde(default = "default_adapter_num")]
    pub adapter_num: String,
    #[serde(default = "default_ip_dog")]
    pub ip_dog: String,
    #[serde(default = "default_auth_version")]
    pub auth_version: String,
    #[serde(default = "default_keep_alive_version")]
    pub keep_alive_version: String,

    #[serde(default = "default_is_test")]
    pub is_test: bool,
    #[serde(default = "default_unlimited_retry")]
    pub unlimited_retry: bool,
}

fn default_host_name() -> String {
    "YOURPCNAME".into()
}
fn default_primary_dns() -> String {
    "10.10.10.10".into()
}
fn default_dhcp_server() -> String {
    "0.0.0.0".into()
}
fn default_bind_ip() -> String {
    "0.0.0.0".into()
}
fn default_control_check_status() -> String {
    "20".into()
}
fn default_adapter_num() -> String {
    "03".into()
}
fn default_ip_dog() -> String {
    "01".into()
}
fn default_auth_version() -> String {
    "68 00".into()
}
fn default_keep_alive_version() -> String {
    "dc 02".into()
}
fn default_is_test() -> bool {
    true
}
fn default_unlimited_retry() -> bool {
    true
}

impl Config {
    /// 从 TOML 文件加载配置。
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// 将 MAC 字符串解析为 48 位无符号整数。
    ///
    /// 支持 `AA:BB:CC:DD:EE:FF`、`AA-BB-CC-DD-EE-FF` 或 `AABBCCDDEEFF` 三种形式。
    pub fn parse_mac(&self) -> io::Result<u64> {
        let s = self.mac.replace([':', '-'], "");
        u64::from_str_radix(&s, 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid MAC: {e}")))
    }

    /// 将 IPv4 点分字符串解析为 4 字节数组。
    pub fn parse_ip(&self, ip: &str) -> io::Result<[u8; 4]> {
        let mut bytes = [0u8; 4];
        for (i, part) in ip.split('.').enumerate() {
            if i >= 4 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid IPv4"));
            }
            bytes[i] = part.parse::<u8>().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("invalid IPv4: {e}"))
            })?;
        }
        Ok(bytes)
    }

    /// 解析 hex 字符串（可包含空格）为字节串。
    pub fn parse_hex(&self, s: &str) -> io::Result<Vec<u8>> {
        let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if !compact.len().is_multiple_of(2) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "odd hex length"));
        }
        (0..compact.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&compact[i..i + 2], 16))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid hex: {e}")))
    }
}

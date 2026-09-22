//! 保存 WebUI 表单参数到 `~/.drcomconfig`。
//!
//! 密码使用简单 XOR 混淆，key 为 MAC 去掉冒号/横线后的字符串。
//! 注意：这只是混淆，不是强加密。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// `~/.drcomconfig` 中保存的字段。
///
/// 所有字段都是可选的，方便旧版本文件缺字段时继续加载。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlainConfig {
    pub server: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host_ip: Option<String>,
    pub mac: Option<String>,
    pub host_name: Option<String>,
    pub primary_dns: Option<String>,
    pub dhcp_server: Option<String>,
    pub bind_ip: Option<String>,
    pub control_check_status: Option<String>,
    pub adapter_num: Option<String>,
    pub ip_dog: Option<String>,
    pub auth_version: Option<String>,
    pub keep_alive_version: Option<String>,
    pub is_test: Option<bool>,
    pub unlimited_retry: Option<bool>,
}

/// `~/.drcomconfig` 的完整路径。
pub fn config_path() -> io::Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| io::Error::other("cannot determine home directory"))?;
    Ok(home.join(".drcomconfig"))
}

/// 读取配置；文件不存在时返回 `Ok(None)`。
///
/// 密码会用文件里的 `mac` 作为 key 解密。
pub fn load() -> io::Result<Option<PlainConfig>> {
    let path = config_path()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };

    let mut fields: PlainConfig = serde_json::from_str(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {}: {e}", path.display()),
        )
    })?;

    if let Some(encrypted) = fields.password.clone() {
        let key = key_from_mac(fields.mac.as_deref())?;
        let plain = xor_hex_decode(&encrypted, &key)?;
        fields.password = Some(String::from_utf8(plain).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid password encoding: {e}"),
            )
        })?);
    }

    Ok(Some(fields))
}

/// 写入配置；密码会用 `fields.mac` 作为 key 混淆后保存。
pub fn save(fields: &PlainConfig) -> io::Result<()> {
    let mut disk = fields.clone();
    if let Some(password) = fields.password.as_deref()
        && !password.is_empty()
    {
        let key = key_from_mac(fields.mac.as_deref())?;
        disk.password = Some(xor_hex_encode(password.as_bytes(), &key));
    }

    let path = config_path()?;
    let text = serde_json::to_string_pretty(&disk)
        .map_err(|e| io::Error::other(format!("failed to serialize config: {e}")))?;
    fs::write(&path, text)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

fn key_from_mac(mac: Option<&str>) -> io::Result<Vec<u8>> {
    let mac = mac.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "mac is required to encrypt password",
        )
    })?;
    let key: String = mac.chars().filter(|c| *c != ':' && *c != '-').collect();
    if key.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mac is empty"));
    }
    Ok(key.into_bytes())
}

fn xor_hex_encode(input: &[u8], key: &[u8]) -> String {
    use std::fmt::Write;

    let mut hex = String::with_capacity(input.len() * 2);
    for (i, byte) in input.iter().enumerate() {
        let mixed = byte ^ key[i % key.len()];
        let _ = write!(hex, "{mixed:02x}");
    }
    hex
}

fn xor_hex_decode(hex: &str, key: &[u8]) -> io::Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "odd hex length"));
    }

    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid hex: {e}")))?;
        out.push(byte ^ key[(i / 2) % key.len()]);
    }
    Ok(out)
}

impl From<&crate::Config> for PlainConfig {
    fn from(config: &crate::Config) -> Self {
        Self {
            server: Some(config.server.clone()),
            username: Some(config.username.clone()),
            password: Some(config.password.clone()),
            host_ip: Some(config.host_ip.clone()),
            mac: Some(config.mac.clone()),
            host_name: Some(config.host_name.clone()),
            primary_dns: Some(config.primary_dns.clone()),
            dhcp_server: Some(config.dhcp_server.clone()),
            bind_ip: Some(config.bind_ip.clone()),
            control_check_status: Some(config.control_check_status.clone()),
            adapter_num: Some(config.adapter_num.clone()),
            ip_dog: Some(config.ip_dog.clone()),
            auth_version: Some(config.auth_version.clone()),
            keep_alive_version: Some(config.keep_alive_version.clone()),
            is_test: Some(config.is_test),
            unlimited_retry: Some(config.unlimited_retry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_round_trip() {
        let key = b"CC28AA1A1ED1";
        let plain = b"secret-password";
        let hex = xor_hex_encode(plain, key);
        assert_eq!(xor_hex_decode(&hex, key).unwrap(), plain);
    }
}

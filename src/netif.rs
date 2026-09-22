//! 本机网卡枚举。
//!
//! 该模块基于 `network-interface` crate，在 Linux / macOS / Windows 上提供
//! 一致的结果，供 WebUI 或 C ABI 选择 MAC 地址使用。

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use std::io;

/// 一块本机网卡的信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    /// 网卡名称（Windows 下通常是友好名称，Unix 下是接口名）。
    pub name: String,
    /// MAC 地址；部分虚拟网卡可能没有。
    pub mac: Option<String>,
    /// 该网卡上配置的 IPv4 地址，可能为空。
    pub ipv4: Vec<String>,
    /// 是否为 loopback 等不可远程访问的接口。
    pub internal: bool,
}

/// 列出本机所有网卡。
pub fn list_adapters() -> io::Result<Vec<AdapterInfo>> {
    let interfaces = NetworkInterface::show()
        .map_err(|e| io::Error::other(format!("failed to list network interfaces: {e}")))?;

    Ok(interfaces
        .into_iter()
        .map(|itf| AdapterInfo {
            name: itf.name,
            mac: itf.mac_addr,
            ipv4: itf
                .addr
                .iter()
                .filter_map(|addr| match addr {
                    Addr::V4(v4) => Some(v4.ip.to_string()),
                    Addr::V6(_) => None,
                })
                .collect(),
            internal: itf.internal,
        })
        .collect())
}

/// 列出可用于认证的网卡：排除 loopback 等内部接口，且必须有合法的 6 字节 MAC。
pub fn usable_adapters() -> io::Result<Vec<AdapterInfo>> {
    Ok(list_adapters()?
        .into_iter()
        .filter(|a| !a.internal && a.mac.as_deref().is_some_and(is_usable_mac))
        .collect())
}

/// 判断 MAC 是否为可用的 6 字节单播地址。
///
/// `network-interface` 在 Windows 上会把 Teredo 等隧道接口的 8 字节标识
/// 也当作 MAC 返回，这里过滤掉长度不对或全零的地址。
fn is_usable_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split([':', '-']).collect();
    if parts.len() != 6 {
        return false;
    }

    let mut all_zero = true;
    for part in parts {
        if part.len() != 2 || u8::from_str_radix(part, 16).is_err() {
            return false;
        }
        if !part.eq_ignore_ascii_case("00") {
            all_zero = false;
        }
    }
    !all_zero
}

//! Dr.COM 协议实现模块。
//!
//! 提供 Dr.COM 认证协议所需的所有底层函数，包括：
//! - [`md5sum`][]: MD5 哈希计算
//! - [`dump`][]: 整数到十六进制字节的转换
//! - [`ror`][]: 密码混淆算法
//! - [`checksum`][]: 报文校验和
//! - [`mkpkt`][]: 登录报文构造
//! - [`keep_alive_package_builder`][]: 保活报文构造
//! - [`logout`][]: 注销报文构造
//!
//! 通常用户不需要直接调用这些函数，[`Client`](crate::Client) 会处理所有协议细节。

use crate::config::Config;
use md5::{Digest, Md5};
use std::io;

/// 计算 MD5 摘要。
///
/// # 示例
///
/// ```
/// use drcom::protocol::md5sum;
/// let hash = md5sum(b"hello");
/// assert_eq!(hash.len(), 16);
/// ```
pub fn md5sum(data: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 将整数转换为最简偶数长度 hex 字节串。
///
/// 与 Python 脚本中的 `dump()` 对应：
/// - `0` -> `[0x00]`
/// - `255` -> `[0xff]`
/// - `256` -> `[0x01, 0x00]`
pub fn dump(n: u64) -> Vec<u8> {
    if n == 0 {
        return vec![0x00];
    }
    let mut hex = format!("{n:x}");
    if hex.len() % 2 != 0 {
        hex.insert(0, '0');
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Dr.COM 密码混淆算法，对应 Python 脚本中的 `ror()`。
pub fn ror(md5: &[u8], pwd: &[u8]) -> Vec<u8> {
    pwd.iter()
        .enumerate()
        .map(|(i, &p)| {
            let x = md5[i] ^ p;
            // `& 0xFF` 在 u8 上是恒等操作，但保留以体现 Python 原意
            #[allow(clippy::identity_op)]
            {
                ((x << 3) & 0xFF) + (x >> 5)
            }
        })
        .collect()
}

/// Dr.COM 报文校验和，对应 Python 脚本中的 `checksum()`。
///
/// # 重要：刻意复刻 Python 正则的分块行为
///
/// 原脚本使用 `re.findall(b'....', s)` 分块，而 Python 正则中的 `.` **默认不匹配
/// 换行字节 `0x0A`**。因此任何含 `0x0A` 的 4 字节窗口都会匹配失败，扫描位置右移
/// 1 字节后重新对齐，与"固定 4 字节分组"的结果不同。
///
/// 由于本机 DNS（例如 `10.10.10.10` → `0a 0a 0a 0a`）会产生 `0x0A`，该差异会实际
/// 影响登录报文的校验和，故必须保留此行为，才能与原客户端逐字节一致。
/// 若服务器并不校验该字段，则两种实现均可用。
pub fn checksum(data: &[u8]) -> [u8; 4] {
    let mut ret: u32 = 1234;
    let mut pos = 0usize;
    while pos + 4 <= data.len() {
        let window = &data[pos..pos + 4];
        if window.contains(&0x0A) {
            // 匹配失败：右移 1 字节重新尝试对齐
            pos += 1;
            continue;
        }
        let word = u32::from_le_bytes([window[0], window[1], window[2], window[3]]);
        ret ^= word;
        pos += 4;
    }
    // `& 0xffffffff` 在 u32 上是恒等操作，但保留以体现 Python 原意
    #[allow(clippy::identity_op)]
    {
        ret = (1968u32.wrapping_mul(ret)) & 0xffffffff;
    }
    ret.to_le_bytes()
}

/// 构造 keep-alive 报文。
pub fn keep_alive_package_builder(
    number: u8,
    _random: &[u8],
    tail: &[u8],
    pkg_type: u8,
    first: bool,
    host_ip: [u8; 4],
    keep_alive_version: &[u8],
) -> Vec<u8> {
    let mut data = Vec::new();
    data.push(0x07);
    data.push(number);
    data.extend_from_slice(&[0x28, 0x00, 0x0b]);
    data.push(pkg_type);
    if first {
        data.extend_from_slice(&[0x0f, 0x27]);
    } else {
        data.extend_from_slice(keep_alive_version);
    }
    data.extend_from_slice(&[0x2f, 0x12]);
    data.extend_from_slice(&[0x00; 6]);
    data.extend_from_slice(tail);
    data.extend_from_slice(&[0x00; 4]);

    if pkg_type == 3 {
        data.extend_from_slice(&[0x00; 4]); // CRC 置零
        data.extend_from_slice(&host_ip);   // 本机 IP
        data.extend_from_slice(&[0x00; 8]);
    } else {
        data.extend_from_slice(&[0x00; 16]);
    }
    data
}

/// 构造注销报文。
///
/// 注销报文用于通知服务器结束当前会话。命令字为 `0x06`，
/// 头部结构与登录报文一致（`0x06 0x01 0x00 <len+20>`），
/// 其后的 16 字节在登录报文中为密码摘要，注销时置零。
///
/// 返回 `(报文字节, 期望的响应命令字)`。
pub fn logout(usr: &[u8], mac: u64, config: &Config) -> io::Result<Vec<u8>> {
    let control_check_status = config.parse_hex(&config.control_check_status)?;
    let adapter_num = config.parse_hex(&config.adapter_num)?;

    let mut data = Vec::new();
    data.extend_from_slice(&[0x06, 0x01, 0x00]);
    data.push((usr.len() + 20) as u8);

    // 占位摘要（登录报文中为 md5，注销时置零）
    data.extend_from_slice(&[0x00; 16]);

    // 用户名（不足 36 字节右补零）
    let mut usr_padded = usr.to_vec();
    if usr.len() < 36 {
        usr_padded.extend_from_slice(&vec![0u8; 36 - usr.len()]);
    }
    data.extend_from_slice(&usr_padded);

    // 控制位与适配器号
    data.extend_from_slice(&control_check_status);
    data.extend_from_slice(&adapter_num);

    // MAC 地址（不足 6 字节左补零）
    let mac_bytes = dump(mac);
    let mut mac_padded = vec![0u8; 6];
    let start = 6usize.saturating_sub(mac_bytes.len());
    mac_padded[start..].copy_from_slice(&mac_bytes[mac_bytes.len().saturating_sub(6)..]);
    data.extend_from_slice(&mac_padded);

    Ok(data)
}

/// 构造登录报文，对应 Python 脚本中的 `mkpkt()`。
pub fn mkpkt(salt: &[u8; 4], usr: &[u8], pwd: &[u8], mac: u64, config: &Config) -> io::Result<Vec<u8>> {
    let control_check_status = config.parse_hex(&config.control_check_status)?;
    let adapter_num = config.parse_hex(&config.adapter_num)?;
    let ip_dog = config.parse_hex(&config.ip_dog)?;
    let auth_version = config.parse_hex(&config.auth_version)?;
    let host_ip = config.parse_ip(&config.host_ip)?;
    let primary_dns = config.parse_ip(&config.primary_dns)?;
    let dhcp_server = config.parse_ip(&config.dhcp_server)?;
    let host_name = config.host_name.as_bytes();

    let mut data = Vec::new();

    // 头部
    data.extend_from_slice(&[0x03, 0x01, 0x00]);
    data.push((usr.len() + 20) as u8);

    // md51
    let md51 = md5sum(&[&[0x03, 0x01][..], salt, pwd].concat());
    data.extend_from_slice(&md51);

    // 用户名（不足 36 字节右补零，超过则保持原长，与 Python ljust 行为一致）
    let mut usr_padded = usr.to_vec();
    if usr.len() < 36 {
        usr_padded.extend_from_slice(&vec![0u8; 36 - usr.len()]);
    }
    data.extend_from_slice(&usr_padded);

    // 控制检查状态与适配器数量
    data.extend_from_slice(&control_check_status);
    data.extend_from_slice(&adapter_num);

    // mac xor md51[0:6]
    let md51_part = u64::from_be_bytes([
        0, 0,
        md51[0], md51[1], md51[2], md51[3], md51[4], md51[5],
    ]);
    let xor_val = md51_part ^ mac;
    let xor_bytes = dump(xor_val);
    let mut xor_padded = vec![0u8; 6];
    let start = 6usize.saturating_sub(xor_bytes.len());
    xor_padded[start..].copy_from_slice(&xor_bytes[xor_bytes.len().saturating_sub(6)..]);
    data.extend_from_slice(&xor_padded);

    // md52
    let mut md52_input = Vec::new();
    md52_input.push(0x01);
    md52_input.extend_from_slice(pwd);
    md52_input.extend_from_slice(salt);
    md52_input.extend_from_slice(&[0x00; 4]);
    let md52 = md5sum(&md52_input);
    data.extend_from_slice(&md52);

    // IP 数量与本机 IP
    data.push(0x01);
    data.extend_from_slice(&host_ip);
    data.extend_from_slice(&[0x00; 4 * 3]);

    // md53
    let mut md53_input = data.clone();
    md53_input.extend_from_slice(&[0x14, 0x00, 0x07, 0x0b]);
    let md53 = md5sum(&md53_input);
    data.extend_from_slice(&md53[..8]);

    // IPDOG 与分隔符
    data.extend_from_slice(&ip_dog);
    data.extend_from_slice(&[0x00; 4]);

    // 计算机名（不足 32 字节右补零，超过则保持原长）
    let mut name_padded = host_name.to_vec();
    if host_name.len() < 32 {
        name_padded.extend_from_slice(&vec![0u8; 32 - host_name.len()]);
    }
    data.extend_from_slice(&name_padded);

    // DNS 信息
    data.extend_from_slice(&primary_dns);
    data.extend_from_slice(&dhcp_server);
    data.extend_from_slice(&[0x00; 4]); // secondary dns
    data.extend_from_slice(&[0x00; 8]); // delimiter

    // OS 与 DrCOM 魔数字段
    data.extend_from_slice(&[0x94, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x06, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);
    data.extend_from_slice(&[0xf0, 0x23, 0x00, 0x00]);
    data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);
    data.extend_from_slice(b"DrCOM\x00\xcf\x07\x68");
    data.extend_from_slice(&[0x00; 55]);
    data.extend_from_slice(b"3dc79f5212e8170acfa9ec95f1d74916542be7b1");
    data.extend_from_slice(&[0x00; 24]);

    // 认证版本与密码混淆
    data.extend_from_slice(&auth_version);
    data.push(0x00);
    data.push(pwd.len() as u8);
    let ror_data = ror(&md51, pwd);
    data.extend_from_slice(&ror_data);

    data.extend_from_slice(&[0x02, 0x0c]);

    // 校验和
    let mut checksum_input = data.clone();
    checksum_input.extend_from_slice(&[0x01, 0x26, 0x07, 0x11, 0x00, 0x00]);
    checksum_input.extend_from_slice(&dump(mac));
    let chk = checksum(&checksum_input);
    data.extend_from_slice(&chk);

    data.extend_from_slice(&[0x00, 0x00]);
    data.extend_from_slice(&dump(mac));

    // 密码非 16 字节时的特殊填充
    if pwd.len() != 16 {
        data.extend_from_slice(&vec![0x00; pwd.len() / 4]);
    }

    data.extend_from_slice(&[0x60, 0xa2]);
    data.extend_from_slice(&[0x00; 28]);

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望值来自原 Python 脚本（fixed 输入下的输出），用于防止实现漂移。
    fn unhex(s: &str) -> Vec<u8> {
        let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..compact.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&compact[i..i + 2], 16).unwrap())
            .collect()
    }

    fn to_hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn test_config() -> Config {
        toml::from_str(
            r#"
server = "10.100.61.3"
username = "testuser"
password = "testpass123"
host_ip = "192.168.1.100"
mac = "AA:BB:CC:DD:EE:FF"
host_name = "TESTPC"
primary_dns = "10.10.10.10"
dhcp_server = "0.0.0.0"
"#,
        )
        .expect("parse test config")
    }

    #[test]
    fn md5_and_dump() {
        assert_eq!(to_hex(&md5sum(b"hello")), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(to_hex(&dump(0)), "00");
        assert_eq!(to_hex(&dump(255)), "ff");
        assert_eq!(to_hex(&dump(256)), "0100");
    }

    #[test]
    fn checksum_matches_python() {
        assert_eq!(to_hex(&checksum(b"abcdefgh")), "206dc65e");
    }

    #[test]
    fn mkpkt_matches_python() {
        let cfg = test_config();
        let salt: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
        let packet = mkpkt(&salt, b"testuser", b"testpass123", 0xAABBCCDDEEFF, &cfg).unwrap();

        let expected = "0301001ccd5aacb5b263c2e2a9df3e163af1e8d3746573747573657200000000000000000000000000000000000000000000000000000000200367e160685c9c6d805acc4d5b804226e38e74e141f78b01c0a80164000000000000000000000000258d55c6a51b2a7f010000000054455354504300000000000000000000000000000000000000000000000000000a0a0a0a00000000000000000000000000000000940000000600000002000000f0230000020000004472434f4d00cf076800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000336463373966353231326538313730616366613965633935663164373439313635343262653762310000000000000000000000000000000000000000000000006800000bcdf9fe0e16108d8cc46f68020c8001217d0000aabbccddeeff000060a200000000000000000000000000000000000000000000000000000000";

        assert_eq!(to_hex(&packet), to_hex(&unhex(expected)));
    }

    #[test]
    fn keep_alive_builder_matches_python() {
        let cfg = test_config();
        let host_ip = cfg.parse_ip(&cfg.host_ip).unwrap();
        let kav = cfg.parse_hex(&cfg.keep_alive_version).unwrap();

        let ka1 = keep_alive_package_builder(0, &[], &[0u8; 4], 1, true, host_ip, &kav);
        assert_eq!(
            to_hex(&ka1),
            "070028000b010f272f12000000000000000000000000000000000000000000000000000000000000"
        );

        let ka3 = keep_alive_package_builder(5, &[], &[1, 2, 3, 4], 3, false, host_ip, &kav);
        assert_eq!(
            to_hex(&ka3),
            "070528000b03dc022f12000000000000010203040000000000000000c0a801640000000000000000"
        );
    }
}

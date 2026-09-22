//! C ABI 接口（`extern "C"`）。
//!
//! 该模块把 [`Client`] 封装为 C 可调用的 API，编译产物为 `cdylib`（`.dll`/`.so`/`.dylib`）
//! 与 `staticlib`（`.lib`/`.a`）。C 侧仅需包含 `include/drcom.h`。
//!
//! ## 约定
//!
//! - 所有返回 `int` 的函数：`0` 表示成功，`-1` 表示失败。
//! - 失败时可用 [`drcom_last_error`] 获取线程局部的错误信息（UTF-8，以 NUL 结尾）。
//! - 由库返回的字符串（如 `drcom_config_get`）必须用 [`drcom_string_free`] 释放。
//! - 句柄必须用 [`drcom_client_free`] 释放。
//!
//! ## 示例（C）
//!
//! ```c
//! #include "drcom.h"
//!
//! drcom_client *c = drcom_client_from_file("drcom.toml");
//! if (!c) { fprintf(stderr, "%s\n", drcom_last_error()); return 1; }
//! drcom_client_set_verbose(c, 1);
//! if (drcom_client_run(c) != 0) { fprintf(stderr, "%s\n", drcom_last_error()); }
//! drcom_client_free(c);
//! ```

use crate::client::Client;
use crate::config::Config;
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_last_error(msg: &str) {
    let sanitized = msg.replace('\0', " ");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(sanitized).ok();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// 不透明句柄，C 侧仅作为指针使用。
pub struct DrcomClient {
    inner: Client,
}

/// 从 `*const c_char` 安全地转为 `&str`。
///
/// # Safety
///
/// `ptr` 必须指向以 NUL 结尾的有效 UTF-8 字符串，或在提供 `default` 时为 NULL。
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err("null pointer".to_string());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| format!("invalid UTF-8 string: {e}"))
}

/// 将 Rust 字符串转为由调用方释放的 C 字符串。
fn into_c_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// 运行可能 panic 的闭包，panic 时记录错误并返回 `-1`。
fn guard<T>(fallback: T, f: impl FnOnce() -> Result<T, String>) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            set_last_error(&e);
            fallback
        }
        Err(_) => {
            set_last_error("internal panic");
            fallback
        }
    }
}

/// 获取最近一次错误的线程局部信息（NUL 结尾的 UTF-8）。
///
/// 返回的指针由库内部持有，**不要**释放；下一次错误会覆盖它。
/// 若无错误则返回 NULL。
#[unsafe(no_mangle)]
pub extern "C" fn drcom_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| match &*slot.borrow() {
        Some(cs) => cs.as_ptr(),
        None => ptr::null(),
    })
}

/// 释放由本库返回的字符串（如 [`drcom_config_get`] 的返回值）。
///
/// # Safety
///
/// `s` 必须是本库返回且尚未释放的指针，或 NULL。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(s));
    }
}

/// 从 TOML 配置文件创建客户端。
///
/// 成功返回句柄，失败返回 NULL（可用 [`drcom_last_error`] 查看原因）。
///
/// # Safety
///
/// `config_path` 必须是有效的 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_from_file(config_path: *const c_char) -> *mut DrcomClient {
    guard(ptr::null_mut(), || {
        let path = unsafe { cstr_to_str(config_path) }?;
        let config = Config::from_file(path).map_err(|e| format!("failed to load config: {e}"))?;
        let client = Client::new(config).map_err(|e| format!("failed to create client: {e}"))?;
        clear_last_error();
        Ok(Box::into_raw(Box::new(DrcomClient { inner: client })))
    })
}

/// 从 TOML 字符串创建客户端（不经过配置文件）。
///
/// `toml_text` 的内容与 `drcom.toml` 相同，可选字段可省略并使用默认值。
/// 成功返回句柄，失败返回 NULL（可用 [`drcom_last_error`] 查看原因）。
///
/// # Safety
///
/// `toml_text` 必须是有效的 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_new_from_toml(toml_text: *const c_char) -> *mut DrcomClient {
    guard(ptr::null_mut(), || {
        let text = unsafe { cstr_to_str(toml_text) }?;
        let config: Config =
            toml::from_str(text).map_err(|e| format!("failed to parse config: {e}"))?;
        let client = Client::new(config).map_err(|e| format!("failed to create client: {e}"))?;
        clear_last_error();
        Ok(Box::into_raw(Box::new(DrcomClient { inner: client })))
    })
}

/// 设置是否输出详细报文日志。
///
/// # Safety
///
/// `client` 必须是 [`drcom_client_from_file`] 返回且尚未释放的指针。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_set_verbose(client: *mut DrcomClient, verbose: c_int) {
    if client.is_null() {
        set_last_error("null client handle");
        return;
    }
    unsafe {
        (*client).inner.set_verbose(verbose != 0);
    }
}

/// 运行认证与保活流程（阻塞），直到通过 [`drcom_client_stop`] 请求停止。
///
/// 停止后会自动发送注销报文。成功返回 `0`，失败返回 `-1`。
///
/// 由于该函数会阻塞，通常应在独立线程中调用，并在其他线程调用
/// [`drcom_client_stop`]。
///
/// # Safety
///
/// `client` 必须是有效且尚未释放的句柄。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_run(client: *mut DrcomClient) -> c_int {
    if client.is_null() {
        set_last_error("null client handle");
        return -1;
    }
    guard(-1, || {
        let c = unsafe { &*client };
        c.inner
            .run_until_stopped()
            .map_err(|e| format!("run failed: {e}"))?;
        clear_last_error();
        Ok(0)
    })
}

/// 请求停止保活循环（线程安全，可在任意线程调用）。
///
/// # Safety
///
/// `client` 必须是有效且尚未释放的句柄。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_stop(client: *mut DrcomClient) -> c_int {
    if client.is_null() {
        set_last_error("null client handle");
        return -1;
    }
    unsafe {
        (*client).inner.request_stop();
    }
    0
}

/// 立即发送注销报文并退出会话。
///
/// # Safety
///
/// `client` 必须是有效且尚未释放的句柄。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_logout(client: *mut DrcomClient) -> c_int {
    if client.is_null() {
        set_last_error("null client handle");
        return -1;
    }
    guard(-1, || {
        let c = unsafe { &*client };
        c.inner
            .logout()
            .map_err(|e| format!("logout failed: {e}"))?;
        clear_last_error();
        Ok(0)
    })
}

/// 释放客户端句柄。
///
/// # Safety
///
/// `client` 必须是 [`drcom_client_from_file`] 返回且尚未释放的指针，或 NULL。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_client_free(client: *mut DrcomClient) {
    if client.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(client));
    }
}

/// 获取客户端配置中的字段（返回需用 [`drcom_string_free`] 释放的字符串）。
///
/// 支持的 `field`：`server`、`username`、`password`、`host_ip`、`mac`、
/// `host_name`、`primary_dns`、`dhcp_server`、`bind_ip`。
/// 未知字段返回 NULL。
///
/// # Safety
///
/// `client` 必须是有效句柄，`field` 必须是有效的 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_config_get(
    client: *mut DrcomClient,
    field: *const c_char,
) -> *mut c_char {
    if client.is_null() {
        set_last_error("null client handle");
        return ptr::null_mut();
    }
    guard(ptr::null_mut(), || {
        let name = unsafe { cstr_to_str(field) }?;
        let cfg = unsafe { (*client).inner.config() };
        let value = match name {
            "server" => cfg.server.clone(),
            "username" => cfg.username.clone(),
            "password" => cfg.password.clone(),
            "host_ip" => cfg.host_ip.clone(),
            "mac" => cfg.mac.clone(),
            "host_name" => cfg.host_name.clone(),
            "primary_dns" => cfg.primary_dns.clone(),
            "dhcp_server" => cfg.dhcp_server.clone(),
            "bind_ip" => cfg.bind_ip.clone(),
            other => return Err(format!("unknown field: {other}")),
        };
        clear_last_error();
        Ok(into_c_string(value))
    })
}

/// 计算 MD5 摘要，写入调用方提供的 16 字节缓冲区。
///
/// # Safety
///
/// `data` 必须指向 `len` 字节，`out` 必须指向至少 16 字节的可写缓冲区。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_md5sum(data: *const u8, len: usize, out: *mut u8) -> c_int {
    if out.is_null() || (data.is_null() && len != 0) {
        set_last_error("null pointer");
        return -1;
    }
    let input = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let digest = crate::protocol::md5sum(input);
    unsafe {
        ptr::copy_nonoverlapping(digest.as_ptr(), out, digest.len());
    }
    0
}

/// 计算 Dr.COM 报文校验和，写入调用方提供的 4 字节缓冲区。
///
/// # Safety
///
/// `data` 必须指向 `len` 字节，`out` 必须指向至少 4 字节的可写缓冲区。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_checksum(data: *const u8, len: usize, out: *mut u8) -> c_int {
    if out.is_null() || (data.is_null() && len != 0) {
        set_last_error("null pointer");
        return -1;
    }
    let input = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let chk = crate::protocol::checksum(input);
    unsafe {
        ptr::copy_nonoverlapping(chk.as_ptr(), out, chk.len());
    }
    0
}

/// `drcom_adapter_info.name` 的缓冲区长度。
pub const DRCOM_ADAPTER_NAME_LEN: usize = 256;
/// `drcom_adapter_info.mac` 的缓冲区长度。
pub const DRCOM_ADAPTER_MAC_LEN: usize = 32;
/// `drcom_adapter_info.ipv4` 的缓冲区长度。
pub const DRCOM_ADAPTER_IPV4_LEN: usize = 16;

/// 单个网卡信息的 C 结构体。
///
/// 字段均为以 NUL 结尾的 UTF-8 字符串；名称过长时会被截断。
#[repr(C)]
pub struct DrcomAdapterInfo {
    pub name: [c_char; DRCOM_ADAPTER_NAME_LEN],
    pub mac: [c_char; DRCOM_ADAPTER_MAC_LEN],
    pub ipv4: [c_char; DRCOM_ADAPTER_IPV4_LEN],
}

fn write_cstr(dst: &mut [c_char], src: &str) {
    let bytes = src.as_bytes();
    let max = dst.len().saturating_sub(1);
    let n = bytes.len().min(max);
    for (i, &b) in bytes[..n].iter().enumerate() {
        dst[i] = b as c_char;
    }
    dst[n] = 0;
}

/// 列出可用于认证的本机网卡（排除 loopback 和无 MAC 的接口）。
///
/// `out` 是调用方提供的数组，`capacity` 是元素个数。
/// 返回找到的网卡总数（可能大于 `capacity`），失败返回 `-1`。
/// 若 `out` 为 NULL 且 `capacity` 为 0，则只返回数量。
/// 每个字段均以 NUL 结尾；名称过长时会被截断。
///
/// # Safety
///
/// `out` 必须指向至少 `capacity` 个 [`DrcomAdapterInfo`] 的可写内存，
/// 或者为 NULL（此时 `capacity` 必须为 0）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drcom_adapters_list(out: *mut DrcomAdapterInfo, capacity: c_int) -> c_int {
    guard(-1, || {
        if capacity < 0 {
            return Err("negative capacity".to_string());
        }
        if out.is_null() && capacity != 0 {
            return Err("null output with non-zero capacity".to_string());
        }

        let adapters =
            crate::netif::usable_adapters().map_err(|e| format!("failed to list adapters: {e}"))?;
        let total = adapters.len();

        if out.is_null() {
            clear_last_error();
            return Ok(total as c_int);
        }

        let cap = capacity as usize;
        let slots = unsafe { std::slice::from_raw_parts_mut(out, cap) };
        for (slot, adapter) in slots.iter_mut().zip(adapters.iter()) {
            *slot = DrcomAdapterInfo {
                name: [0; DRCOM_ADAPTER_NAME_LEN],
                mac: [0; DRCOM_ADAPTER_MAC_LEN],
                ipv4: [0; DRCOM_ADAPTER_IPV4_LEN],
            };
            write_cstr(&mut slot.name, &adapter.name);
            write_cstr(&mut slot.mac, adapter.mac.as_deref().unwrap_or(""));
            write_cstr(
                &mut slot.ipv4,
                adapter.ipv4.first().map(String::as_str).unwrap_or(""),
            );
        }
        clear_last_error();
        Ok(total as c_int)
    })
}

//! 高级客户端接口。
//!
//! [`Client`] 封装了 UDP socket、Challenge、Login、KeepAlive 的完整流程，
//! 是库使用者推荐使用的入口。

use crate::config::Config;
use crate::protocol::{dump, keep_alive_package_builder, logout, md5sum, mkpkt};
use rand::Rng;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SERVER_PORT: u16 = 61440;
const RECV_BUF_SIZE: usize = 1024;

/// Dr.COM 客户端。
///
/// 管理 UDP socket 与认证状态，提供 [`run`](Client::run) 方法进行完整认证与保活。
///
/// 可用 [`request_stop`](Client::request_stop) 从其他线程请求停止，
/// [`run_until_stopped`](Client::run_until_stopped) 则在停止后自动注销。
pub struct Client {
    config: Config,
    socket: UdpSocket,
    svr_addr: SocketAddr,
    verbose: bool,
    stop_flag: Arc<AtomicBool>,
}

impl Client {
    /// 根据配置创建客户端实例。
    ///
    /// # Errors
    ///
    /// 当 socket 绑定失败或服务器地址解析失败时返回 [`io::Error`]。
    pub fn new(config: Config) -> io::Result<Self> {
        let socket = UdpSocket::bind(format!("{}:{}", config.bind_ip, SERVER_PORT))?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let svr_addr: SocketAddr = format!("{}:{}", config.server, SERVER_PORT)
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad server address: {e}")))?;
        Ok(Self {
            config,
            socket,
            svr_addr,
            verbose: false,
            stop_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 设置是否输出详细的报文日志。
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// 就地设置是否输出详细报文日志。
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    /// 请求停止保活循环（线程安全，可从任意线程调用）。
    pub fn request_stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// 是否已请求停止。
    pub fn is_stop_requested(&self) -> bool {
        self.stop_flag.load(Ordering::SeqCst)
    }

    /// 输出日志（仅在 verbose 模式下）。
    fn log(&self, msg: &str) {
        if self.verbose {
            println!("{msg}");
        }
    }

    /// 将字节串编码为十六进制字符串，用于日志输出。
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 运行完整的认证与保活流程（CLI 行为）。
    ///
    /// 与 [`run_until_stopped`](Client::run_until_stopped) 相同，但会额外启动一个
    /// 键盘监听线程：按 `q` 键询问是否注销，再按 `y` 确认。
    ///
    /// # Errors
    ///
    /// 当网络通信失败或服务器拒绝认证时返回 [`io::Error`]。
    pub fn run(&self) -> io::Result<()> {
        let stop_flag_clone = Arc::clone(&self.stop_flag);
        thread::spawn(move || {
            Self::keyboard_listener(stop_flag_clone);
        });
        self.run_until_stopped()
    }

    /// 运行认证与保活流程，直到收到停止请求。
    ///
    /// 收到停止请求（[`request_stop`](Client::request_stop)）后发送注销报文并返回。
    /// 该方法是阻塞的，适合由宿主程序在独立线程中调用。
    ///
    /// # Errors
    ///
    /// 当网络通信失败或服务器拒绝认证时返回 [`io::Error`]。
    pub fn run_until_stopped(&self) -> io::Result<()> {
        let (salt, package_tail) = self.login()?;
        self.empty_socket_buffer();
        self.keep_alive1(&salt, &package_tail)?;
        self.keep_alive2(&salt, &package_tail, &self.stop_flag)?;

        // 如果收到停止信号，执行注销
        if self.stop_flag.load(Ordering::SeqCst) {
            println!("\n正在注销...");
            self.logout()?;
            println!("注销完成。");
        }

        Ok(())
    }

    /// 执行注销操作。
    ///
    /// 发送注销报文并等待服务器响应。若读取超时，视为服务器未回复，
    /// 但不视为致命错误（会话通常已被服务端释放）。
    pub fn logout(&self) -> io::Result<()> {
        let username = self.config.username.as_bytes();
        let mac = self.config.parse_mac()?;
        let packet = logout(username, mac, &self.config)?;
        self.socket.send_to(&packet, self.svr_addr)?;

        let mut buf = [0u8; RECV_BUF_SIZE];
        match self.socket.recv_from(&mut buf) {
            Ok((len, addr)) if addr == self.svr_addr => {
                if len > 0 {
                    println!("注销响应: {}", Self::hex(&buf[..len]));
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                println!("未收到注销响应（可能已离线）");
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// 键盘监听线程，监听 'q' 键并询问是否注销。
    fn keyboard_listener(stop_flag: Arc<AtomicBool>) {
        use std::io::{self, BufRead};
        let stdin = io::stdin();

        loop {
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => break, // stdin 已关闭（如被重定向），退出监听
                Ok(_) => {}
                Err(_) => continue,
            }

            if line.trim().eq_ignore_ascii_case("q") {
                println!("是否注销？(y/n): ");
                let mut confirm = String::new();
                if stdin.lock().read_line(&mut confirm).is_err() {
                    break;
                }

                if confirm.trim().eq_ignore_ascii_case("y") {
                    stop_flag.store(true, Ordering::SeqCst);
                    break;
                } else {
                    println!("取消注销，继续保活...");
                }
            }
        }
    }

    /// 获取当前配置的只读引用。
    pub fn config(&self) -> &Config {
        &self.config
    }

    fn challenge(&self, ran: u64) -> io::Result<[u8; 4]> {
        loop {
            let ran_bytes = (ran as u16).to_le_bytes();
            let mut packet = vec![0x01, 0x02];
            packet.extend_from_slice(&ran_bytes);
            packet.push(0x09);
            packet.extend_from_slice(&[0x00; 15]);

            self.socket.send_to(&packet, self.svr_addr)?;
            self.log(&format!("[challenge] send {}", Self::hex(&packet)));

            let mut buf = [0u8; RECV_BUF_SIZE];
            match self.socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    let data = &buf[..len];
                    self.log(&format!("[challenge] recv {}", Self::hex(data)));
                    if addr != self.svr_addr {
                        return Err(io::Error::other("wrong server address"));
                    }
                    if data[0] != 2 {
                        return Err(io::Error::other("challenge failed"));
                    }
                    let mut salt = [0u8; 4];
                    salt.copy_from_slice(&data[4..8]);
                    return Ok(salt);
                }
                Err(_) => {
                    self.log("[challenge] timeout, retrying...");
                    continue;
                }
            }
        }
    }

    fn login(&self) -> io::Result<([u8; 4], [u8; 16])> {
        let username = self.config.username.as_bytes();
        let password = self.config.password.as_bytes();
        let mac = self.config.parse_mac()?;

        let ran = Self::now_timestamp() + rand::thread_rng().gen_range(0x0f..=0xff);
        let salt = self.challenge(ran)?;
        self.log(&format!("[salt] {}", Self::hex(&salt)));

        let packet = mkpkt(&salt, username, password, mac, &self.config)?;
        self.log(&format!("[login] send {}", Self::hex(&packet)));
        self.socket.send_to(&packet, self.svr_addr)?;

        let mut buf = [0u8; RECV_BUF_SIZE];
        let (len, addr) = self.socket.recv_from(&mut buf)?;
        let data = &buf[..len];
        self.log(&format!("[login] recv {}", Self::hex(data)));

        if addr != self.svr_addr {
            return Err(io::Error::other("wrong server address"));
        }
        if data.is_empty() || data[0] != 4 {
            return Err(io::Error::other("login failed"));
        }
        self.log("[login] logged in");

        let mut tail = [0u8; 16];
        if data.len() >= 39 {
            tail.copy_from_slice(&data[23..39]);
        }
        Ok((salt, tail))
    }

    fn keep_alive1(&self, salt: &[u8; 4], tail: &[u8; 16]) -> io::Result<()> {
        let password = self.config.password.as_bytes();
        let timestamp = ((Self::now_timestamp() as u16) % 0xFFFF).to_be_bytes();

        let mut data = Vec::new();
        data.push(0xff);
        data.extend_from_slice(&md5sum(&[&[0x03, 0x01][..], salt, password].concat()));
        data.extend_from_slice(&[0x00, 0x00, 0x00]);
        data.extend_from_slice(tail);
        data.extend_from_slice(&timestamp);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        self.log(&format!("[keep_alive1] send {}", Self::hex(&data)));
        self.socket.send_to(&data, self.svr_addr)?;

        let mut buf = [0u8; RECV_BUF_SIZE];
        loop {
            let (len, _) = self.socket.recv_from(&mut buf)?;
            let recv = &buf[..len];
            self.log(&format!("[keep_alive1] recv {}", Self::hex(recv)));
            if !recv.is_empty() && recv[0] == 7 {
                break;
            }
        }
        Ok(())
    }

    fn keep_alive2(&self, salt: &[u8; 4], package_tail: &[u8; 16], stop_flag: &Arc<AtomicBool>) -> io::Result<()> {
        let _password = self.config.password.as_bytes();
        let host_ip = self.config.parse_ip(&self.config.host_ip)?;
        let keep_alive_version = self.config.parse_hex(&self.config.keep_alive_version)?;

        let mut rng = rand::thread_rng();
        let mut ran: u64 = rng.gen_range(0..=0xFFFF) as u64;
        ran += rng.gen_range(1..=10) as u64;

        let mut svr_num: u8 = 0;
        let mut packet = keep_alive_package_builder(svr_num, &dump(ran), &[0x00; 4], 1, true, host_ip, &keep_alive_version);

        loop {
            self.socket.send_to(&packet, self.svr_addr)?;
            let mut buf = [0u8; RECV_BUF_SIZE];
            let (len, _) = self.socket.recv_from(&mut buf)?;
            let data = &buf[..len];

            let expected_a = [0x07, 0x00, 0x28, 0x00];
            let expected_b = vec![0x07, svr_num, 0x28, 0x00];
            if data.starts_with(&expected_a) || data.starts_with(&expected_b) {
                break;
            } else if !data.is_empty() && data[0] == 0x07 && data.len() > 2 && data[2] == 0x10 {
                svr_num = svr_num.wrapping_add(1);
                packet = keep_alive_package_builder(svr_num, &dump(ran), &[0x00; 4], 1, false, host_ip, &keep_alive_version);
            }
        }

        ran += rng.gen_range(1..=10) as u64;
        packet = keep_alive_package_builder(svr_num, &dump(ran), &[0x00; 4], 1, false, host_ip, &keep_alive_version);
        self.socket.send_to(&packet, self.svr_addr)?;

        let mut tail = [0u8; 4];
        loop {
            let mut buf = [0u8; RECV_BUF_SIZE];
            let (len, _) = self.socket.recv_from(&mut buf)?;
            let data = &buf[..len];
            if !data.is_empty() && data[0] == 7 {
                svr_num = svr_num.wrapping_add(1);
                if data.len() >= 20 {
                    tail.copy_from_slice(&data[16..20]);
                }
                break;
            }
        }

        ran += rng.gen_range(1..=10) as u64;
        packet = keep_alive_package_builder(svr_num, &dump(ran), &tail, 3, false, host_ip, &keep_alive_version);
        self.socket.send_to(&packet, self.svr_addr)?;

        loop {
            let mut buf = [0u8; RECV_BUF_SIZE];
            let (len, _) = self.socket.recv_from(&mut buf)?;
            let data = &buf[..len];
            if !data.is_empty() && data[0] == 7 {
                svr_num = svr_num.wrapping_add(1);
                if data.len() >= 20 {
                    tail.copy_from_slice(&data[16..20]);
                }
                break;
            }
        }

        let mut i = svr_num;
        loop {
            // 检查是否收到停止信号
            if stop_flag.load(Ordering::SeqCst) {
                return Ok(());
            }

            let result = (|| -> io::Result<()> {
                ran += rng.gen_range(1..=10) as u64;
                packet = keep_alive_package_builder(i, &dump(ran), &tail, 1, false, host_ip, &keep_alive_version);
                self.socket.send_to(&packet, self.svr_addr)?;

                let mut buf = [0u8; RECV_BUF_SIZE];
                let (len, _) = self.socket.recv_from(&mut buf)?;
                let data = &buf[..len];
                if data.len() >= 20 {
                    tail.copy_from_slice(&data[16..20]);
                }

                ran += rng.gen_range(1..=10) as u64;
                packet = keep_alive_package_builder(i.wrapping_add(1), &dump(ran), &tail, 3, false, host_ip, &keep_alive_version);
                self.socket.send_to(&packet, self.svr_addr)?;

                let (len, _) = self.socket.recv_from(&mut buf)?;
                let data = &buf[..len];
                if data.len() >= 20 {
                    tail.copy_from_slice(&data[16..20]);
                }

                i = ((i as u16 + 2) % 0xFF) as u8;
                // 分片睡眠，以便及时响应停止信号
                for _ in 0..20 {
                    if stop_flag.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                self.keep_alive1(salt, package_tail)?;
                Ok(())
            })();

            if result.is_err() {
                continue;
            }
        }
    }

    fn empty_socket_buffer(&self) {
        let mut buf = [0u8; RECV_BUF_SIZE];
        loop {
            if self.socket.recv_from(&mut buf).is_err() {
                break;
            }
        }
    }

    fn now_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

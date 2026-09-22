//! WebUI 后端。
//!
//! 提供一个小型本地 HTTP 服务，页面和资源全部嵌入在可执行文件里。
//! `drcom-web` 直接使用它，`drcom-tray` 则把它和托盘图标放在同一个进程里。

use crate::store::{self, PlainConfig};
use crate::{Client, Config};
use serde::{Deserialize, Serialize};
use std::io::{self, Cursor};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::{Header, Request, Response, Server};

const INDEX_HTML: &str = include_str!("../web/index.html");

type HttpResponse = Response<Cursor<Vec<u8>>>;

#[derive(Clone)]
struct AppState {
    session: Arc<Mutex<Option<Session>>>,
    last_error: Arc<Mutex<Option<String>>>,
}

struct Session {
    client: Arc<Client>,
    running: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    started_at: Instant,
}

#[derive(Deserialize)]
struct LoginRequest {
    server: String,
    username: String,
    password: String,
    host_ip: String,
    mac: String,
    host_name: Option<String>,
    primary_dns: Option<String>,
    dhcp_server: Option<String>,
    bind_ip: Option<String>,
    control_check_status: Option<String>,
    adapter_num: Option<String>,
    ip_dog: Option<String>,
    auth_version: Option<String>,
    keep_alive_version: Option<String>,
    is_test: Option<bool>,
    unlimited_retry: Option<bool>,
    verbose: Option<bool>,
}

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    stop_requested: bool,
    elapsed_secs: u64,
    error: Option<String>,
}

#[derive(Serialize)]
struct AdapterDto {
    name: String,
    mac: Option<String>,
    ipv4: Vec<String>,
}

#[derive(Serialize)]
struct DefaultsDto {
    host_name: String,
    primary_dns: String,
    dhcp_server: String,
    bind_ip: String,
    control_check_status: String,
    adapter_num: String,
    ip_dog: String,
    auth_version: String,
    keep_alive_version: String,
    is_test: bool,
    unlimited_retry: bool,
}

/// 一个已经启动的 WebUI 服务。
pub struct WebUi {
    url: String,
    server: Arc<Server>,
    state: AppState,
}

impl WebUi {
    /// 在 `addr`（例如 `127.0.0.1:0`）上启动 WebUI 服务。
    ///
    /// 返回后服务已经在后台线程里处理请求。
    pub fn start(addr: &str) -> io::Result<Self> {
        let server = Server::http(addr)
            .map_err(|e| io::Error::other(format!("failed to bind {addr}: {e}")))?;
        let url = format!("http://{}/", server.server_addr());
        let server = Arc::new(server);
        let state = AppState {
            session: Arc::new(Mutex::new(None)),
            last_error: Arc::new(Mutex::new(None)),
        };

        let server_for_thread = Arc::clone(&server);
        let state_for_thread = state.clone();
        thread::spawn(move || {
            for request in server_for_thread.incoming_requests() {
                handle_request(request, &state_for_thread);
            }
        });

        Ok(Self { url, server, state })
    }

    /// WebUI 的访问地址，例如 `http://127.0.0.1:12345/`。
    pub fn url(&self) -> &str {
        &self.url
    }

    /// 用系统默认浏览器打开 WebUI。
    pub fn open_browser(&self) -> io::Result<()> {
        webbrowser::open(&self.url)
            .map_err(|e| io::Error::other(format!("failed to open browser: {e}")))
    }

    /// 停止接受新请求并唤醒后台线程。
    pub fn shutdown(&self) {
        self.server.unblock();
        let _ = &self.state;
    }
}

fn handle_request(mut request: Request, state: &AppState) {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("");

    let response: HttpResponse = match (method.as_str(), path) {
        ("GET", "/") => text_response(INDEX_HTML, 200, "text/html; charset=utf-8"),
        ("GET", "/api/adapters") => handle_adapters(),
        ("GET", "/api/defaults") => handle_defaults(),
        ("GET", "/api/config") => handle_config(),
        ("GET", "/api/status") => handle_status(state),
        ("POST", "/api/login") => handle_login(&mut request, state),
        ("POST", "/api/logout") => handle_logout(state),
        _ => text_response("not found", 404, "text/plain; charset=utf-8"),
    };

    let _ = request.respond(response);
}

fn handle_adapters() -> HttpResponse {
    match crate::netif::usable_adapters() {
        Ok(adapters) => {
            let dtos: Vec<AdapterDto> = adapters
                .into_iter()
                .map(|a| AdapterDto {
                    name: a.name,
                    mac: a.mac,
                    ipv4: a.ipv4,
                })
                .collect();
            json_response(&dtos, 200)
        }
        Err(e) => error_response(500, &format!("无法枚举网卡: {e}")),
    }
}

fn handle_defaults() -> HttpResponse {
    let d = Config::default();
    json_response(
        &DefaultsDto {
            host_name: d.host_name,
            primary_dns: d.primary_dns,
            dhcp_server: d.dhcp_server,
            bind_ip: d.bind_ip,
            control_check_status: d.control_check_status,
            adapter_num: d.adapter_num,
            ip_dog: d.ip_dog,
            auth_version: d.auth_version,
            keep_alive_version: d.keep_alive_version,
            is_test: d.is_test,
            unlimited_retry: d.unlimited_retry,
        },
        200,
    )
}

fn handle_config() -> HttpResponse {
    match store::load() {
        Ok(None) => json_response(&serde_json::json!({}), 200),
        Ok(Some(fields)) => json_response(&fields, 200),
        Err(e) => error_response(500, &format!("读取配置失败: {e}")),
    }
}

fn handle_status(state: &AppState) -> HttpResponse {
    let (running, stop_requested, elapsed_secs) = {
        let guard = state.session.lock().unwrap();
        match guard.as_ref() {
            Some(session) => (
                session.running.load(Ordering::SeqCst),
                session.client.is_stop_requested(),
                session.started_at.elapsed().as_secs(),
            ),
            None => (false, false, 0),
        }
    };

    let error = state.last_error.lock().unwrap().clone();
    json_response(
        &StatusResponse {
            running,
            stop_requested,
            elapsed_secs,
            error,
        },
        200,
    )
}

fn handle_login(request: &mut Request, state: &AppState) -> HttpResponse {
    let mut body = String::new();
    if let Err(e) = request.as_reader().read_to_string(&mut body) {
        return error_response(400, &format!("读取请求失败: {e}"));
    }

    let req: LoginRequest = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => return error_response(400, &format!("JSON 解析失败: {e}")),
    };

    let d = Config::default();
    let config = Config {
        server: req.server.trim().to_string(),
        username: req.username.trim().to_string(),
        password: req.password,
        host_ip: req.host_ip.trim().to_string(),
        mac: req.mac.trim().to_string(),
        host_name: req
            .host_name
            .map(|v| v.trim().to_string())
            .unwrap_or(d.host_name),
        primary_dns: req
            .primary_dns
            .map(|v| v.trim().to_string())
            .unwrap_or(d.primary_dns),
        dhcp_server: req
            .dhcp_server
            .map(|v| v.trim().to_string())
            .unwrap_or(d.dhcp_server),
        bind_ip: req
            .bind_ip
            .map(|v| v.trim().to_string())
            .unwrap_or(d.bind_ip),
        control_check_status: req
            .control_check_status
            .map(|v| v.trim().to_string())
            .unwrap_or(d.control_check_status),
        adapter_num: req
            .adapter_num
            .map(|v| v.trim().to_string())
            .unwrap_or(d.adapter_num),
        ip_dog: req.ip_dog.map(|v| v.trim().to_string()).unwrap_or(d.ip_dog),
        auth_version: req
            .auth_version
            .map(|v| v.trim().to_string())
            .unwrap_or(d.auth_version),
        keep_alive_version: req
            .keep_alive_version
            .map(|v| v.trim().to_string())
            .unwrap_or(d.keep_alive_version),
        is_test: req.is_test.unwrap_or(d.is_test),
        unlimited_retry: req.unlimited_retry.unwrap_or(d.unlimited_retry),
    };

    if let Some(message) = validate(&config) {
        return error_response(400, &message);
    }

    {
        let mut guard = state.session.lock().unwrap();
        if let Some(session) = guard.as_ref()
            && session.running.load(Ordering::SeqCst)
        {
            return error_response(409, "已有会话在运行，请先断开");
        }

        // 旧会话已经结束：先 join 并释放，确保 UDP 61440 被关闭，
        // 否则新 Client::new 会因端口占用失败（os error 10048）。
        if let Some(mut old) = guard.take() {
            if let Some(worker) = old.worker.take() {
                let _ = worker.join();
            }
            drop(old);
        }
    }

    let saved = PlainConfig::from(&config);

    let mut client = match Client::new(config) {
        Ok(c) => c,
        Err(e) => return error_response(500, &format!("创建客户端失败: {e}")),
    };
    client.set_verbose(req.verbose.unwrap_or(false));
    let client = Arc::new(client);
    let running = Arc::new(AtomicBool::new(true));

    *state.last_error.lock().unwrap() = None;

    // 登录成功后保存配置。worker 阻塞在 run_until_stopped 里，
    // 所以用一个轻量轮询线程等 is_logged_in() 变 true。
    {
        let saver_client = Arc::clone(&client);
        let saver_running = Arc::clone(&running);
        let saver_saved = saved.clone();
        thread::spawn(move || {
            while saver_running.load(Ordering::SeqCst) {
                if saver_client.is_logged_in() {
                    if let Err(e) = store::save(&saver_saved) {
                        eprintln!("保存配置失败: {e}");
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
    }

    let state_for_worker = state.clone();
    let client_for_worker = Arc::clone(&client);
    let running_for_worker = Arc::clone(&running);
    let worker = thread::spawn(move || {
        let result = client_for_worker.run_until_stopped();
        *state_for_worker.last_error.lock().unwrap() = result.err().map(|e| e.to_string());
        running_for_worker.store(false, Ordering::SeqCst);
    });

    {
        let mut guard = state.session.lock().unwrap();
        *guard = Some(Session {
            client,
            running,
            worker: Some(worker),
            started_at: Instant::now(),
        });
    }

    json_response(&serde_json::json!({ "ok": true }), 200)
}

fn handle_logout(state: &AppState) -> HttpResponse {
    let guard = state.session.lock().unwrap();
    match guard.as_ref() {
        Some(session) => {
            session.client.request_stop();
            json_response(&serde_json::json!({ "ok": true }), 200)
        }
        None => error_response(409, "没有正在运行的会话"),
    }
}

fn validate(config: &Config) -> Option<String> {
    if config.server.is_empty() {
        return Some("server 不能为空".to_string());
    }
    if config.username.is_empty() {
        return Some("username 不能为空".to_string());
    }
    if config.password.is_empty() {
        return Some("password 不能为空".to_string());
    }
    if config.host_ip.is_empty() {
        return Some("host_ip 不能为空".to_string());
    }
    if config.mac.is_empty() {
        return Some("mac 不能为空".to_string());
    }

    if let Err(e) = config.parse_ip(&config.server) {
        return Some(format!("server 无效: {e}"));
    }
    if let Err(e) = config.parse_ip(&config.host_ip) {
        return Some(format!("host_ip 无效: {e}"));
    }
    if let Err(e) = config.parse_ip(&config.bind_ip) {
        return Some(format!("bind_ip 无效: {e}"));
    }
    if let Err(e) = config.parse_mac() {
        return Some(format!("MAC 无效: {e}"));
    }

    None
}

fn json_response<T: Serialize>(value: &T, status: u16) -> HttpResponse {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    text_response(&body, status, "application/json; charset=utf-8")
}

fn error_response(status: u16, message: &str) -> HttpResponse {
    let body = serde_json::json!({ "error": message }).to_string();
    text_response(&body, status, "application/json; charset=utf-8")
}

fn text_response(body: &str, status: u16, content_type: &str) -> HttpResponse {
    let header = Header::from_bytes("Content-Type", content_type).expect("valid content type");
    Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(header)
}

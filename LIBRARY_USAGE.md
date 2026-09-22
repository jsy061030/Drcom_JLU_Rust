# drcom 库使用指南

本文档介绍如何将 `drcom` 作为库（library）集成到其他 Rust 项目中。

## 添加依赖

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
drcom = { path = "../drcom-client" }
# 或如果发布到 crates.io：
# drcom = "0.1.0"
```

## 基本用法

### 1. 从配置文件创建客户端

```rust
use drcom::{Config, Client};

fn main() -> std::io::Result<()> {
    // 从 TOML 文件加载配置
    let config = Config::from_file("drcom.toml")?;
    
    // 创建客户端
    let client = Client::new(config)?;
    
    // 运行认证和保活循环
    client.run()?;
    
    Ok(())
}
```

### 2. 编程式配置

```rust
use drcom::{Config, Client};

fn main() -> std::io::Result<()> {
    // 手动构造配置
    let config = Config {
        server: "10.100.61.3".to_string(),
        username: "your_username".to_string(),
        password: "your_password".to_string(),
        host_ip: "192.168.1.2".to_string(),
        mac: "AA:BB:CC:DD:EE:FF".to_string(),
        host_name: "YOURPCNAME".to_string(),
        primary_dns: "10.10.10.10".to_string(),
        dhcp_server: "0.0.0.0".to_string(),
        bind_ip: "0.0.0.0".to_string(),
        control_check_status: "20".to_string(),
        adapter_num: "03".to_string(),
        ip_dog: "01".to_string(),
        auth_version: "68 00".to_string(),
        keep_alive_version: "dc 02".to_string(),
        is_test: true,
        unlimited_retry: true,
    };
    
    let client = Client::new(config)?;
    client.run()?;
    
    Ok(())
}
```

### 3. 异步集成

`drcom` 库本身是同步的，但可以在异步运行时中通过 `spawn_blocking` 使用：

```rust
use drcom::{Config, Client};
use tokio::task;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = Config::from_file("drcom.toml")?;
    
    let handle = task::spawn_blocking(move || {
        let client = Client::new(config)?;
        client.run()
    });
    
    handle.await??;
    
    Ok(())
}
```

## 配置结构体

### `Config`

所有字段均为 `pub`，可直接访问和修改：

```rust
pub struct Config {
    pub server: String,
    pub username: String,
    pub password: String,
    pub host_ip: String,
    pub mac: String,
    pub host_name: String,
    pub primary_dns: String,
    pub dhcp_server: String,
    pub bind_ip: String,
    pub control_check_status: String,
    pub adapter_num: String,
    pub ip_dog: String,
    pub auth_version: String,
    pub keep_alive_version: String,
    pub is_test: bool,
    pub unlimited_retry: bool,
}
```

### 辅助方法

```rust
impl Config {
    /// 从 TOML 文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self>;
    
    /// 解析 MAC 地址为 48 位整数
    pub fn parse_mac(&self) -> io::Result<u64>;
    
    /// 解析 IPv4 地址为 4 字节数组
    pub fn parse_ip(&self, ip: &str) -> io::Result<[u8; 4]>;
    
    /// 解析十六进制字符串为字节向量
    pub fn parse_hex(&self, s: &str) -> io::Result<Vec<u8>>;
}
```

## 客户端 API

### `Client`

```rust
pub struct Client { /* ... */ }

impl Client {
    /// 创建客户端实例
    pub fn new(config: Config) -> io::Result<Self>;

    /// 设置是否输出详细报文日志（链式调用）
    pub fn with_verbose(self, verbose: bool) -> Self;

    /// 运行完整的认证和保活流程
    ///
    /// 运行期间在终端按 `q` 询问是否注销，再按 `y` 确认注销；按其他键取消。
    pub fn run(&self) -> io::Result<()>;

    /// 获取配置的只读引用
    pub fn config(&self) -> &Config;
}
```

### 交互式注销

`run()` 会启动一个键盘监听线程：

| 输入 | 行为 |
|------|------|
| `q` | 询问「是否注销？」 |
| 随后 `y` | 发送注销报文（命令字 `0x06`）并退出保活循环 |
| 随后其他 | 取消注销，继续保活 |

注销报文由 [`protocol::logout`](#底层协议-api) 构造。由于原脚本未实现注销，
该报文格式依据通用 Dr.COM 实现推导，若你的服务器不接受，请对照抓包调整。

### 从其他线程停止

`run_until_stopped()` 配合 `request_stop()` 可以在登录阶段取消：

```rust
use drcom::{Client, Config};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let client = Arc::new(Client::new(Config::from_file("drcom.toml")?)?);

    let worker = {
        let client = Arc::clone(&client);
        thread::spawn(move || client.run_until_stopped())
    };

    thread::sleep(Duration::from_secs(1));
    client.request_stop();      // 登录阶段也可取消，最长等一次 3 秒读超时
    worker.join().unwrap()?;
    Ok(())
}
```

登录前取消返回 `Ok(())`，不会发送注销报文；登录后取消会先发送注销报文再返回。

### 详细日志

```rust
use drcom::{Config, Client};

fn main() -> std::io::Result<()> {
    let config = Config::from_file("drcom.toml")?;
    let client = Client::new(config)?.with_verbose(true);
    client.run()?;
    Ok(())
}
```

启用 `verbose` 后会输出 Challenge / Login / KeepAlive 的十六进制报文，便于排查。

## 网卡枚举

`drcom::netif` 模块提供跨平台网卡枚举，可用于 WebUI 选择 MAC：

```rust
use drcom::netif;

fn main() -> std::io::Result<()> {
    for adapter in netif::usable_adapters()? {
        println!("{} {:?} {:?}", adapter.name, adapter.mac, adapter.ipv4);
    }
    Ok(())
}
```

`list_adapters()` 返回所有网卡；`usable_adapters()` 过滤掉 loopback 和无 MAC 的接口。

## 底层协议 API

如果需要更细粒度的控制，可以直接使用 `protocol` 模块：

```rust
use drcom::protocol::{md5sum, dump, ror, checksum, mkpkt, keep_alive_package_builder, logout};
use drcom::Config;

fn main() -> std::io::Result<()> {
    // MD5 哈希
    let hash = md5sum(b"hello");
    
    // 整数转十六进制字节
    let bytes = dump(0x1234); // [0x12, 0x34]
    
    // 密码混淆
    let confused = ror(&hash, b"password");
    
    // 报文校验和
    let chk = checksum(b"some data");
    
    // 构造登录报文
    let config = Config::from_file("drcom.toml")?;
    let salt = [0x01, 0x02, 0x03, 0x04];
    let packet = mkpkt(&salt, b"user", b"pass", 0x123456, &config)?;
    
    // 构造保活报文
    let host_ip = [192, 168, 1, 2];
    let ka_version = [0xdc, 0x02];
    let ka_packet = keep_alive_package_builder(
        0,              // number
        &dump(0x1234),  // random
        &[0x00; 4],     // tail
        1,              // pkg_type
        true,           // first
        host_ip,
        &ka_version,
    );

    // 构造注销报文
    let logout_packet = logout(b"user", 0x123456, &config)?;

    Ok(())
}
```

## 错误处理

所有可能失败的操作都返回 `std::io::Result<T>`：

```rust
use drcom::{Config, Client};
use std::io;

fn authenticate() -> io::Result<()> {
    let config = Config::from_file("drcom.toml")?;
    let client = Client::new(config)?;
    client.run()?;
    Ok(())
}

fn main() {
    match authenticate() {
        Ok(_) => println!("认证成功"),
        Err(e) => eprintln!("认证失败: {}", e),
    }
}
```

## 日志

库本身不包含日志框架，但可以通过以下方式集成：

1. **使用 `log` crate**（推荐）：
   ```rust
   // 在你的应用中初始化日志，例如：
   env_logger::init();
   log::info!("开始认证");
   ```

2. **自定义日志**：
   ```rust
   // 库不会输出日志，你可以在外层包装打印
   println!("认证开始...");
   client.run()?;
   println!("认证成功");
   ```

## 示例项目

完整示例见 `examples/` 目录：

```bash
cargo run --example simple
```

## 注意事项

1. **UDP 端口 61440**：客户端会绑定到本地 61440 端口，确保该端口未被占用
2. **网络权限**：在某些系统上可能需要管理员权限
3. **配置文件安全**：配置中包含明文密码，请妥善保管
4. **KeepAlive 循环**：`client.run()` 会进入无限保活循环，通常在独立线程中运行

## 许可证

AGPL-3.0

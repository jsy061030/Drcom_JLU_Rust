# drcom-client

Dr.COM 校园网认证客户端的 Rust 重写版本，提供**可执行文件**、**Rust 库**和 **C 库**三种形式。

## 功能

- Challenge 认证（获取 salt）
- 登录认证
- 持续保活（keep_alive1 + keep_alive2）
- 交互式注销（`q` → `y`）
- TOML 配置文件支持
- 跨平台（Windows / Linux / macOS）
- **Rust 库接口**（crate `drcom`）：可集成到其他 Rust 项目
- **C ABI 接口**（`cdylib` / `staticlib`）：可被 C/C++/C#/Python 等调用

## 项目结构

本项目同时产出：
- **可执行文件** `drcom-client`：命令行客户端
- **Rust 库** `drcom`（rlib）：可嵌入其他 Rust 应用
- **C 库** `drcom.dll` / `drcom.lib` / `libdrcom.so` / `libdrcom.a`：C ABI，配套头文件 `include/drcom.h`

文档：
- [Rust 库使用指南](LIBRARY_USAGE.md)
- [C 接口使用指南](C_API.md)

## 依赖

- `md-5`: MD5 哈希
- `rand`: 随机数生成
- `serde` + `toml`: 配置文件解析

## 构建

```bash
cargo build --release
```

构建产物（`target/release/`）：

| 文件 | 说明 |
|------|------|
| `drcom-client.exe` / `drcom-client` | 命令行客户端 |
| `drcom.dll` / `libdrcom.so` / `libdrcom.dylib` | C 动态库（cdylib） |
| `drcom.dll.lib` | Windows 动态库导入库 |
| `drcom.lib` / `libdrcom.a` | C 静态库（staticlib） |

> **注意**: 某些 Windows 环境下（如使用 `x86_64-pc-windows-gnullvm` 目标），
> 可能因缺少 MinGW-w64 系统库导致链接失败。建议切换至 `x86_64-pc-windows-msvc`
> 目标并确保安装了 Visual Studio Build Tools。

> **构建时若提示无法替换 `drcom-client.exe`（拒绝访问）**：说明上次运行的客户端
> 进程仍在后台占用该文件，先用任务管理器或 `taskkill /PID <pid> /F` 结束它再构建。

## 使用

1. 创建配置文件 `drcom.toml`：

```toml
server = "10.100.61.3"
username = "your_username"
password = "your_password"
host_ip = "192.168.1.2"
mac = "AA:BB:CC:DD:EE:FF"
```

2. 运行：

```bash
# 使用默认配置文件（程序所在目录下的 drcom.toml）
cargo run --release

# 指定配置文件路径
cargo run --release -- /path/to/config.toml

# 或直接运行编译后的二进制
./target/release/drcom-client /path/to/config.toml
```

> **配置文件查找顺序**：若命令行未指定路径，程序会在**可执行文件所在目录**下查找 `drcom.toml`
> （而非当前工作目录），方便将配置与程序放在一起分发。

### 注销会话

程序运行（保活）期间，在终端中：

- 按 `q` 键 → 程序询问是否注销；
- 再按 `y` 键 → 发送注销报文并退出；
- 输入其他内容 → 取消注销，继续保活。

## 配置说明

### 必填字段

| 字段 | 说明 |
|------|------|
| `server` | 认证服务器 IP |
| `username` | 用户名 |
| `password` | 密码 |
| `host_ip` | 本机 IP |
| `mac` | 本机 MAC（支持 `AA:BB:CC:DD:EE:FF` 或 `AABBCCDDEEFF`） |

### 可选字段

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `host_name` | `YOURPCNAME` | 计算机名 |
| `primary_dns` | `10.10.10.10` | 主 DNS |
| `dhcp_server` | `0.0.0.0` | DHCP 服务器 |
| `bind_ip` | `0.0.0.0` | 绑定 IP |
| `control_check_status` | `20` | 协议字段（hex） |
| `adapter_num` | `03` | 协议字段（hex） |
| `ip_dog` | `01` | 协议字段（hex） |
| `auth_version` | `68 00` | 认证版本（hex） |
| `keep_alive_version` | `dc 02` | 保活版本（hex） |
| `is_test` | `true` | 测试模式 |
| `unlimited_retry` | `true` | 无限重试 |

## 与原版差异

1. **配置方式**: 原版使用硬编码或 Python `exec()` 加载配置；本版本使用 TOML。
2. **文件日志**: 原版在非 Windows 平台写入文件（但有 bug）；本版本仅输出到控制台。
3. **网卡绑定**: 原版在 Unix 下支持 `fcntl` 绑定指定网卡；本版本仅使用 `bind_ip` 参数。
4. **错误处理**: 本版本使用 Rust 标准错误类型，提供清晰的错误信息。

## 实现说明（重要）

### 校验和的"正则怪癖"已刻意保留

原脚本的 `checksum()` 用 `re.findall(b'....', s)` 做 4 字节分块，而 Python 正则中的
`.` **默认不匹配换行字节 `0x0A`**。由于 `primary_dns = 10.10.10.10` 会产生 `0a 0a 0a 0a`，
这会让扫描位置右移重新对齐，结果与"固定 4 字节分组"**不同**。

本实现（Rust 与 C#）为与原客户端逐字节一致，**故意复刻**了这一行为。
若你的服务器并不校验该字段，两种算法都可用；若需改为标准 4 字节分组，请修改
`src/protocol.rs` 的 `checksum()`（C# 对应 `Protocol.Checksum`）。

### 报文一致性

Rust / C# 实现的 `md5sum`、`dump`、`checksum`、`mkpkt`、`keep_alive_package_builder`
输出已与原 Python 脚本逐字节比对通过（相同输入 → 完全相同的报文）。

## 安全提醒

- 配置文件中包含明文密码，请妥善保管。
- 建议将配置文件加入 `.gitignore`。

## License

AGPL-3.0

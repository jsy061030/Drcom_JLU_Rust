# drcom C ABI 跨平台测试

这个目录包含一个最小的 C 程序，用于验证 `drcom` 库导出的 C ABI 是否能正常工作。

## 默认测试内容（不需要真实网络）

- `drcom_md5sum` / `drcom_checksum` 结果校验
- `drcom_client_from_file` 成功/失败路径
- `drcom_client_new_from_toml` 成功/失败路径
- `drcom_config_get` / `drcom_string_free` / `drcom_client_free`
- `drcom_client_set_verbose`
- `drcom_adapters_list`：数量查询、缓冲区不足、NULL 参数错误处理
- 登录阶段取消：在 `127.0.0.2:61440` 挂一个只收不回的 socket，1 秒后调用
  `drcom_client_stop`，验证 `drcom_client_run` 能在一次读超时后返回
- 错误信息（`drcom_last_error`）是否被正确设置

## 可选的真实网络测试

加上 `--run [配置文件路径]` 会额外在一个后台线程调用 `drcom_client_run`，主线程 5 秒后调用 `drcom_client_stop` 触发注销。如果省略路径，默认使用当前目录下的 `drcom.toml`。

## 构建

先编译 Rust 库：

```bash
cd drcom-client
cargo build --release
```

### Windows + MSVC（推荐）

在 “Developer Command Prompt for VS” 里，或先 `call` 对应的 `vcvars64.bat`：

```bat
call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cl /nologo /W4 /utf-8 /I "..\..\include" test_abi.c /Fe:build\test_abi.exe /Fo:build\ ^
   /link /LIBPATH:"..\..\target\release" drcom.dll.lib ws2_32.lib
copy /Y "..\..\target\release\drcom.dll" "build\drcom.dll"
copy /Y "test_config.toml" "build\test_config.toml"
```

### 跨平台 CMake

```bash
cd drcom-client/tests/c_abi
mkdir build && cd build
cmake ..
cmake --build . --config Release
```

如果 `drcom` 库不在默认的 `../../target/release`，可以手动指定：

```bash
cmake .. -DDRCOM_LIB_DIR=/path/to/library
```

## 运行

```bash
# 仅跑无网络测试
./test_abi

# 额外测试真实登录/保活/注销（需要可达的 Dr.COM 服务器）
./test_abi --run /path/to/real/drcom.toml
```

> `--run` 只适合在能连到认证服务器的机器上使用。当前 `Client` 的停止标志只在保活循环里检查；如果服务器不可达，`drcom_client_run()` 会一直卡在 challenge 重试里，`drcom_client_stop()` 无法把它叫醒。

Windows 上可以直接双击运行 `test_abi.exe`，无网络测试不依赖外部配置。

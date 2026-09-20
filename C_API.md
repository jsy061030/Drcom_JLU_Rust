# drcom C 接口使用指南

`drcom` 库除了可作为 Rust crate 使用外，还导出 **C ABI**，可被 C / C++ / C# / Python 等
语言直接调用。编译产物为：

| 平台 | 动态库 | 导入库 | 静态库 |
|------|--------|--------|--------|
| Windows (MSVC) | `drcom.dll` | `drcom.dll.lib` | `drcom.lib` |
| Linux | `libdrcom.so` | — | `libdrcom.a` |
| macOS | `libdrcom.dylib` | — | `libdrcom.a` |

## 构建

```bash
cargo build --release
```

产物位于 `target/release/`。

## 头文件

接口声明在 [`include/drcom.h`](include/drcom.h)，直接 `#include "drcom.h"` 即可。

## API 概览

```c
/* 错误查询与内存管理 */
const char *drcom_last_error(void);
void        drcom_string_free(char *s);

/* 客户端生命周期 */
drcom_client *drcom_client_from_file(const char *config_path);
void          drcom_client_free(drcom_client *client);

/* 运行控制 */
void drcom_client_set_verbose(drcom_client *client, int verbose);
int  drcom_client_run(drcom_client *client);     /* 阻塞，直到 stop */
int  drcom_client_stop(drcom_client *client);    /* 线程安全 */
int  drcom_client_logout(drcom_client *client);

/* 配置读取 */
char *drcom_config_get(drcom_client *client, const char *field);

/* 协议工具 */
int drcom_md5sum(const unsigned char *data, size_t len, unsigned char *out);   /* out >= 16 */
int drcom_checksum(const unsigned char *data, size_t len, unsigned char *out); /* out >= 4  */
```

## 约定

- 返回 `int` 的函数：`0` 成功，`-1` 失败。
- 失败原因通过 `drcom_last_error()` 获取，返回线程局部的 UTF-8 字符串；
  **该指针由库持有，不要释放**，下一次错误会覆盖它。
- 由库返回的 `char *`（目前仅 `drcom_config_get`）必须用 `drcom_string_free()` 释放。
- 句柄必须用 `drcom_client_free()` 释放。

## 完整示例（C）

```c
#include <stdio.h>
#include "drcom.h"

int main(void) {
    drcom_client *c = drcom_client_from_file("drcom.toml");
    if (!c) {
        fprintf(stderr, "create failed: %s\n", drcom_last_error());
        return 1;
    }

    /* 开启报文日志便于排查 */
    drcom_client_set_verbose(c, 1);

    /* drcom_client_run 会阻塞，真实项目建议放到独立线程 */
    if (drcom_client_run(c) != 0) {
        fprintf(stderr, "run failed: %s\n", drcom_last_error());
    }

    drcom_client_free(c);
    return 0;
}
```

### 编译链接

Windows（MSVC）：

```
cl example.c /I include /link drcom.dll.lib ws2_32.lib
```

Linux：

```bash
gcc example.c -Iinclude -Ltarget/release -ldrcom -lpthread -ldl -lm -o example
```

macOS：

```bash
clang example.c -Iinclude -Ltarget/release -ldrcom -o example
```

> 运行时需确保动态库可被找到：Windows 将 `drcom.dll` 放到 exe 同目录；
> Linux 设置 `LD_LIBRARY_PATH`；macOS 设置 `DYLD_LIBRARY_PATH`。

## 从其他线程停止

`drcom_client_run()` 会阻塞直到收到停止请求。典型用法是在一个线程运行，
另一个线程触发停止（例如响应 UI 的"注销"按钮）：

```c
#include <pthread.h>
#include "drcom.h"

static drcom_client *g_client;

void *worker(void *arg) {
    drcom_client_run(g_client);      /* 阻塞保活 */
    return NULL;
}

int main(void) {
    g_client = drcom_client_from_file("drcom.toml");

    pthread_t t;
    pthread_create(&t, NULL, worker, NULL);

    /* ... 等待用户操作 ... */
    drcom_client_stop(g_client);     /* 触发注销并结束 worker */

    pthread_join(t, NULL);
    drcom_client_free(g_client);
    return 0;
}
```

## C# 调用示例

```csharp
using System;
using System.Runtime.InteropServices;

static class Drcom {
    [DllImport("drcom.dll")] public static extern IntPtr drcom_client_from_file(string path);
    [DllImport("drcom.dll")] public static extern int drcom_client_run(IntPtr c);
    [DllImport("drcom.dll")] public static extern int drcom_client_stop(IntPtr c);
    [DllImport("drcom.dll")] public static extern void drcom_client_free(IntPtr c);
    [DllImport("drcom.dll")] public static extern IntPtr drcom_last_error();
}

// 使用：
// var c = Drcom.drcom_client_from_file("drcom.toml");
// var t = Task.Run(() => Drcom.drcom_client_run(c));
// ...
// Drcom.drcom_client_stop(c);
// Drcom.drcom_client_free(c);
```

> 注意：`drcom_client_run` 是阻塞调用，在 C# 中应通过 `Task.Run` 放到线程池执行。

## 线程安全

- `drcom_client_stop()` 是线程安全的，可从任意线程调用。
- 其余函数（`run` / `logout` / `config_get` / `free`）不应在客户端运行期间从其他线程调用。
- 错误信息为线程局部（thread-local），每个线程各自维护。

## 注意事项

1. **注销报文格式**：原 Python 脚本未实现注销，`0x06` 注销报文依据通用 Dr.COM
   实现推导，若服务器不接受请对照抓包调整 `src/protocol.rs` 的 `logout()`。
2. **stdin**：C 库的 `drcom_client_run()` **不**读取 stdin（键盘 `q`/`y` 交互仅存在于
   命令行二进制）。宿主程序通过 `drcom_client_stop()` 控制。
3. **配置文件**：`drcom_client_from_file()` 需要显式传入路径，不做默认目录查找。

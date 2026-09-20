/*
 * drcom.h - Dr.COM 校园网认证客户端 C 接口
 *
 * 本头文件声明了 drcom 库导出的 C ABI 函数。
 * 链接方式：
 *   - 动态库：Windows 链接 drcom.dll（导入库 drcom.dll.lib / drcom.dll.a）
 *             Linux/macOS 链接 libdrcom.so / libdrcom.dylib
 *   - 静态库：Windows 链接 drcom.lib，Linux/macOS 链接 libdrcom.a
 *
 * 另需链接系统 socket 库：Windows 下 ws2_32，Linux/macOS 通常无需额外库。
 *
 * 约定：
 *   - 返回 int 的函数：0 成功，-1 失败；失败原因用 drcom_last_error() 查询。
 *   - 由库返回的 char* 必须用 drcom_string_free() 释放。
 *   - 句柄必须用 drcom_client_free() 释放。
 */

#ifndef DRCOM_H
#define DRCOM_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32)
#  define DRCOM_API __declspec(dllimport)
#else
#  define DRCOM_API
#endif

/* 不透明句柄 */
typedef struct DrcomClient drcom_client;

/*
 * 获取最近一次错误的线程局部描述（UTF-8，NUL 结尾）。
 * 返回的指针由库内部持有，切勿释放；无错误时返回 NULL。
 */
DRCOM_API const char *drcom_last_error(void);

/* 释放由本库返回的字符串。传入 NULL 是安全的。 */
DRCOM_API void drcom_string_free(char *s);

/*
 * 从 TOML 配置文件创建客户端。
 * 成功返回句柄，失败返回 NULL。
 */
DRCOM_API drcom_client *drcom_client_from_file(const char *config_path);

/*
 * 设置是否输出详细报文日志（verbose != 0 表示开启）。
 */
DRCOM_API void drcom_client_set_verbose(drcom_client *client, int verbose);

/*
 * 运行认证与保活流程（阻塞），直到 drcom_client_stop() 被调用。
 * 停止后会自动发送注销报文。
 * 返回 0 成功，-1 失败。
 *
 * 注意：该函数会阻塞，建议在独立线程中调用。
 */
DRCOM_API int drcom_client_run(drcom_client *client);

/*
 * 请求停止保活循环（线程安全，可从任意线程调用）。
 * 返回 0 成功，-1 失败。
 */
DRCOM_API int drcom_client_stop(drcom_client *client);

/*
 * 立即发送注销报文并退出会话。
 * 返回 0 成功，-1 失败。
 */
DRCOM_API int drcom_client_logout(drcom_client *client);

/* 释放客户端句柄。传入 NULL 是安全的。 */
DRCOM_API void drcom_client_free(drcom_client *client);

/*
 * 获取配置字段值，返回的字符串需用 drcom_string_free() 释放。
 * 支持的字段名：
 *   "server" "username" "password" "host_ip" "mac"
 *   "host_name" "primary_dns" "dhcp_server" "bind_ip"
 * 未知字段返回 NULL。
 */
DRCOM_API char *drcom_config_get(drcom_client *client, const char *field);

/*
 * 计算 MD5 摘要，写入 out（至少 16 字节）。
 * 返回 0 成功，-1 失败。
 */
DRCOM_API int drcom_md5sum(const unsigned char *data, size_t len, unsigned char *out);

/*
 * 计算 Dr.COM 报文校验和，写入 out（至少 4 字节）。
 * 返回 0 成功，-1 失败。
 */
DRCOM_API int drcom_checksum(const unsigned char *data, size_t len, unsigned char *out);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* DRCOM_H */

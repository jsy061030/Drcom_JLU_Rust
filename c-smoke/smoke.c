/*
 * smoke.c - drcom C ABI 冒烟测试
 *
 * 验证导出的 C 接口可被 C 正确调用并链接。不涉及网络。
 * 编译示例见 .github/workflows/c-abi.yml 或 C_API.md。
 */

#include <stdio.h>
#include <string.h>

#include "drcom.h"

static void to_hex(const unsigned char *in, int n, char *out)
{
    static const char digits[] = "0123456789abcdef";
    int i;
    for (i = 0; i < n; i++) {
        out[i * 2] = digits[in[i] >> 4];
        out[i * 2 + 1] = digits[in[i] & 0x0F];
    }
    out[n * 2] = '\0';
}

static int check_md5(void)
{
    unsigned char out[16];
    char hex[33];
    const char *expect = "5d41402abc4b2a76b9719d911017c592";

    if (drcom_md5sum((const unsigned char *)"hello", 5, out) != 0) {
        fprintf(stderr, "drcom_md5sum returned non-zero\n");
        return 0;
    }
    to_hex(out, 16, hex);
    if (strcmp(hex, expect) != 0) {
        fprintf(stderr, "md5 mismatch: got %s, want %s\n", hex, expect);
        return 0;
    }
    return 1;
}

static int check_checksum(void)
{
    unsigned char out[4];
    char hex[9];
    const char *expect = "206dc65e";

    if (drcom_checksum((const unsigned char *)"abcdefgh", 8, out) != 0) {
        fprintf(stderr, "drcom_checksum returned non-zero\n");
        return 0;
    }
    to_hex(out, 4, hex);
    if (strcmp(hex, expect) != 0) {
        fprintf(stderr, "checksum mismatch: got %s, want %s\n", hex, expect);
        return 0;
    }
    return 1;
}

int main(void)
{
    if (!check_md5()) {
        return 1;
    }
    if (!check_checksum()) {
        return 1;
    }
    if (drcom_last_error() != NULL) {
        fprintf(stderr, "unexpected last error: %s\n", drcom_last_error());
        return 1;
    }

    printf("C ABI smoke test passed\n");
    return 0;
}

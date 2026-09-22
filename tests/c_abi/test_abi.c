/*
 * Cross-platform C ABI smoke test for the drcom library.
 *
 * By default this test exercises the lifecycle and helper APIs without
 * requiring a real Dr.COM server.  To also test run/stop/logout against a
 * live server, run:
 *
 *     ./test_abi --run /path/to/real/drcom.toml
 */

#include <drcom.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
typedef HANDLE thread_t;
static int thread_create(thread_t *t, LPTHREAD_START_ROUTINE fn, LPVOID arg)
{
    *t = CreateThread(NULL, 0, fn, arg, 0, NULL);
    return *t ? 0 : -1;
}
static int thread_join(thread_t t)
{
    WaitForSingleObject(t, INFINITE);
    CloseHandle(t);
    return 0;
}
static void sleep_seconds(unsigned int s) { Sleep((DWORD)s * 1000); }

typedef SOCKET test_socket_t;
#define TEST_INVALID_SOCKET INVALID_SOCKET
static int socket_init(void)
{
    WSADATA wsa;
    return WSAStartup(MAKEWORD(2, 2), &wsa) == 0 ? 0 : -1;
}
static void socket_cleanup(void) { WSACleanup(); }
static void socket_close(test_socket_t s) { closesocket(s); }
#else
#include <pthread.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
typedef pthread_t thread_t;
static int thread_create(thread_t *t, void *(*fn)(void *), void *arg)
{
    return pthread_create(t, NULL, fn, arg);
}
static int thread_join(thread_t t) { return pthread_join(t, NULL); }
static void sleep_seconds(unsigned int s) { sleep(s); }

typedef int test_socket_t;
#define TEST_INVALID_SOCKET (-1)
static int socket_init(void) { return 0; }
static void socket_cleanup(void) {}
static void socket_close(test_socket_t s) { close(s); }
#endif

static test_socket_t bind_udp(const char *ip, unsigned short port)
{
    test_socket_t s;
    struct sockaddr_in addr;
    int opt = 1;

    s = socket(AF_INET, SOCK_DGRAM, 0);
    if (s == TEST_INVALID_SOCKET) {
        return s;
    }

    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (const char *)&opt, (int)sizeof(opt));

    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);
    if (inet_pton(AF_INET, ip, &addr.sin_addr) != 1) {
        socket_close(s);
        return TEST_INVALID_SOCKET;
    }

    if (bind(s, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        socket_close(s);
        return TEST_INVALID_SOCKET;
    }
    return s;
}

static int g_passed = 0;
static int g_failed = 0;

#define TEST_ASSERT(cond, ...)                                                   \
    do {                                                                         \
        if (!(cond)) {                                                           \
            fprintf(stderr, "FAIL (%s:%d): ", __FILE__, __LINE__);              \
            fprintf(stderr, __VA_ARGS__);                                        \
            fputc('\n', stderr);                                                \
            g_failed++;                                                          \
        } else {                                                                 \
            g_passed++;                                                          \
        }                                                                        \
    } while (0)

struct run_args {
    drcom_client *client;
    volatile int finished;
    int result;
    char error[256];
};

#ifdef _WIN32
static DWORD WINAPI run_thread(LPVOID arg)
#else
static void *run_thread(void *arg)
#endif
{
    struct run_args *a = (struct run_args *)arg;
    a->result = drcom_client_run(a->client);
    {
        const char *err = drcom_last_error();
        snprintf(a->error, sizeof(a->error), "%s", err ? err : "");
    }
    a->finished = 1;
#ifdef _WIN32
    return 0;
#else
    return NULL;
#endif
}

static void test_helpers(void)
{
    const unsigned char data[] = "hello";
    unsigned char md5[16];
    TEST_ASSERT(drcom_md5sum(data, sizeof(data) - 1, md5) == 0, "md5sum failed");

    static const unsigned char expected_md5[16] = {
        0x5d, 0x41, 0x40, 0x2a, 0xbc, 0x4b, 0x2a, 0x76,
        0xb9, 0x71, 0x9d, 0x91, 0x10, 0x17, 0xc5, 0x92,
    };
    TEST_ASSERT(memcmp(md5, expected_md5, 16) == 0, "md5 mismatch");

    const unsigned char data2[] = "1234567890";
    unsigned char chk[4];
    TEST_ASSERT(drcom_checksum(data2, sizeof(data2) - 1, chk) == 0, "checksum failed");

    static const unsigned char expected_chk[4] = {0x20, 0x6d, 0xc6, 0x5e};
    TEST_ASSERT(memcmp(chk, expected_chk, 4) == 0, "checksum mismatch");
}

static void test_lifecycle(const char *config_path)
{
    drcom_client *c = drcom_client_from_file("definitely_missing_file.toml");
    TEST_ASSERT(c == NULL, "missing config should return NULL");
    TEST_ASSERT(drcom_last_error() != NULL, "missing config should set last_error");

    c = drcom_client_from_file(config_path);
    TEST_ASSERT(c != NULL, "valid config should create client (%s)", config_path);
    if (!c) {
        return;
    }

    drcom_client_set_verbose(c, 0);

    char *server = drcom_config_get(c, "server");
    TEST_ASSERT(server != NULL && strcmp(server, "10.100.61.3") == 0,
                "config_get(server) mismatch");
    drcom_string_free(server);

    char *username = drcom_config_get(c, "username");
    TEST_ASSERT(username != NULL && strcmp(username, "test_user") == 0,
                "config_get(username) mismatch");
    drcom_string_free(username);

    char *bad = drcom_config_get(c, "not_a_field");
    TEST_ASSERT(bad == NULL, "unknown config field should return NULL");
    TEST_ASSERT(drcom_last_error() != NULL, "unknown config field should set last_error");

    /* NULL must be safe for string_free. */
    drcom_string_free(NULL);

    drcom_client_free(c);
    drcom_client_free(NULL);
}

static const char *TEST_TOML =
    "server = \"10.100.61.3\"\n"
    "username = \"toml_user\"\n"
    "password = \"toml_pass\"\n"
    "host_ip = \"192.168.1.3\"\n"
    "mac = \"AA:BB:CC:DD:EE:01\"\n";

static void test_new_from_toml(void)
{
    drcom_client *c = drcom_client_new_from_toml(TEST_TOML);
    TEST_ASSERT(c != NULL, "new_from_toml should create client (%s)",
                drcom_last_error() ? drcom_last_error() : "unknown");
    if (c) {
        char *username = drcom_config_get(c, "username");
        TEST_ASSERT(username != NULL && strcmp(username, "toml_user") == 0,
                    "new_from_toml username mismatch");
        drcom_string_free(username);

        char *mac = drcom_config_get(c, "mac");
        TEST_ASSERT(mac != NULL && strcmp(mac, "AA:BB:CC:DD:EE:01") == 0,
                    "new_from_toml mac mismatch");
        drcom_string_free(mac);

        drcom_client_free(c);
    }

    drcom_client *invalid = drcom_client_new_from_toml("this is not toml");
    TEST_ASSERT(invalid == NULL, "invalid TOML should return NULL");
    TEST_ASSERT(drcom_last_error() != NULL, "invalid TOML should set last_error");

    drcom_client *missing = drcom_client_new_from_toml("server = \"1.2.3.4\"\n");
    TEST_ASSERT(missing == NULL, "missing required fields should return NULL");
    TEST_ASSERT(drcom_last_error() != NULL, "missing required fields should set last_error");
}

#define MAX_ADAPTERS 32

static void test_adapters(void)
{
    int count = drcom_adapters_list(NULL, 0);
    TEST_ASSERT(count >= 0, "adapter count should be >= 0 (got %d, err=%s)", count,
                drcom_last_error() ? drcom_last_error() : "none");
    if (count <= 0) {
        return;
    }

    drcom_adapter_info list[MAX_ADAPTERS];
    int capacity = count > MAX_ADAPTERS ? MAX_ADAPTERS : count;
    int got = drcom_adapters_list(list, capacity);
    TEST_ASSERT(got == count, "adapter list count mismatch: got %d, expected %d", got, count);

    for (int i = 0; i < capacity; i++) {
        TEST_ASSERT(list[i].name[0] != '\0', "adapter %d has empty name", i);
        printf("  [%d] name=%s mac=%s ipv4=%s\n", i, list[i].name, list[i].mac,
               list[i].ipv4[0] ? list[i].ipv4 : "(none)");
    }

    /* A too-small buffer still reports the total count. */
    drcom_adapter_info one;
    int total = drcom_adapters_list(&one, 1);
    TEST_ASSERT(total == count, "adapter list with capacity 1 should return total");
    TEST_ASSERT(one.name[0] != '\0', "adapter list with capacity 1 should fill first entry");

    /* NULL output with non-zero capacity is an error. */
    int err = drcom_adapters_list(NULL, 1);
    TEST_ASSERT(err == -1, "null output with capacity 1 should fail");
    TEST_ASSERT(drcom_last_error() != NULL, "null output failure should set last_error");
}

static void test_cancel_login(void)
{
    static const char *toml =
        "server = \"127.0.0.2\"\n"
        "username = \"cancel_user\"\n"
        "password = \"cancel_pass\"\n"
        "host_ip = \"127.0.0.1\"\n"
        "mac = \"AA:BB:CC:DD:EE:02\"\n"
        "bind_ip = \"127.0.0.1\"\n"
        "is_test = true\n";

    drcom_client *c;
    test_socket_t listener;
    struct run_args args;
    thread_t t;
    int waited = 0;

    if (socket_init() != 0) {
        printf("[cancel test] skipped: socket_init failed\n");
        return;
    }

    /*
     * Hold 127.0.0.2:61440 so the client's challenge packet is accepted locally
     * but never answered.  This keeps the test off the outside network while
     * still making the challenge phase block and retry.
     */
    listener = bind_udp("127.0.0.2", 61440);
    if (listener == TEST_INVALID_SOCKET) {
        printf("[cancel test] skipped: could not bind 127.0.0.2:61440\n");
        socket_cleanup();
        return;
    }

    c = drcom_client_new_from_toml(toml);
    if (!c) {
        printf("[cancel test] skipped: %s\n",
               drcom_last_error() ? drcom_last_error() : "unknown error");
        socket_close(listener);
        socket_cleanup();
        return;
    }

    drcom_client_set_verbose(c, 0);

    args.client = c;
    args.finished = 0;
    args.result = 0;
    args.error[0] = '\0';

    if (thread_create(&t, run_thread, &args) != 0) {
        TEST_ASSERT(0, "cancel thread_create failed");
        drcom_client_free(c);
        socket_close(listener);
        socket_cleanup();
        return;
    }

    sleep_seconds(1);
    printf("[cancel test] requesting stop during login...\n");
    TEST_ASSERT(drcom_client_stop(c) == 0, "cancel drcom_client_stop failed");

    while (!args.finished && waited < 10) {
        sleep_seconds(1);
        waited++;
    }

    if (!args.finished) {
        TEST_ASSERT(0, "cancel test: drcom_client_run did not return within 10s");
        socket_close(listener);
        socket_cleanup();
        return;
    }

    TEST_ASSERT(thread_join(t) == 0, "cancel thread_join failed");
    TEST_ASSERT(args.result == 0, "cancel test: drcom_client_run returned %d (%s)", args.result,
                args.error[0] ? args.error : "no error");
    printf("[cancel test] drcom_client_run returned %d after %d s\n", args.result, waited + 1);

    drcom_client_free(c);
    socket_close(listener);
    socket_cleanup();
}

static void test_run_stop(const char *config_path)
{
    drcom_client *c = drcom_client_from_file(config_path);
    if (!c) {
        TEST_ASSERT(0, "run test: failed to load %s (%s)", config_path,
                    drcom_last_error() ? drcom_last_error() : "unknown");
        return;
    }

    const char *verbose_env = getenv("DRCOM_VERBOSE");
    int verbose = verbose_env != NULL && verbose_env[0] != '\0' && verbose_env[0] != '0';
    printf("[run test] verbose=%d\n", verbose);
    drcom_client_set_verbose(c, verbose);

    struct run_args args = {c, 0, 0};
    thread_t t;
    TEST_ASSERT(thread_create(&t, run_thread, &args) == 0, "thread_create failed");

    printf("[run test] letting drcom_client_run() work for 5 seconds...\n");
    sleep_seconds(5);

    printf("[run test] requesting stop...\n");
    TEST_ASSERT(drcom_client_stop(c) == 0, "drcom_client_stop failed");

    TEST_ASSERT(thread_join(t) == 0, "thread_join failed");
    printf("[run test] drcom_client_run() returned %d (%s)\n", args.result,
           args.error[0] ? args.error : "no error");

    drcom_client_free(c);
}

static void print_usage(const char *prog)
{
    printf("Usage: %s [options]\n", prog);
    printf("Options:\n");
    printf("  --run [PATH]   Also test drcom_client_run/stop against a live server.\n");
    printf("                 PATH defaults to 'drcom.toml' in the current directory.\n");
    printf("  -h, --help     Show this help.\n");
}

int main(int argc, char **argv)
{
    const char *test_config = "test_config.toml";
    int do_run = 0;
    const char *run_config = NULL;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--run") == 0) {
            do_run = 1;
            if (i + 1 < argc && argv[i + 1][0] != '-') {
                run_config = argv[++i];
            }
        } else if (strcmp(argv[i], "-h") == 0 || strcmp(argv[i], "--help") == 0) {
            print_usage(argv[0]);
            return 0;
        }
    }

    printf("=== drcom C ABI smoke test ===\n");

    test_helpers();
    test_lifecycle(test_config);
    test_new_from_toml();
    test_adapters();
    test_cancel_login();

    if (do_run) {
        if (!run_config) {
            run_config = "drcom.toml";
        }
        printf("\n=== live run/stop test (%s) ===\n", run_config);
        test_run_stop(run_config);
    }

    printf("\nResults: %d passed, %d failed\n", g_passed, g_failed);
    return g_failed > 0 ? 1 : 0;
}

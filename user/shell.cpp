#include <cstdint>
#include <cstddef>

#define SYS_READ      0
#define SYS_WRITE     1
#define SYS_OPEN      2
#define SYS_CLOSE     3
#define SYS_SEEK      4
#define SYS_STAT      5
#define SYS_MKDIR     6
#define SYS_UNLINK    7
#define SYS_GETDENTS  8
#define SYS_MOUNT     10
#define SYS_UMOUNT    11
#define SYS_EXIT      21
#define SYS_GETPID    23
#define SYS_PRINT     100
#define SYS_READLINE  101
#define SYS_SYSINFO   200

static inline int64_t syscall(uint64_t nr, uint64_t a1 = 0, uint64_t a2 = 0,
                               uint64_t a3 = 0, uint64_t a4 = 0) {
    int64_t result;
    __asm__ volatile (
        "int $0x80"
        : "=a"(result)
        : "a"(nr), "D"(a1), "S"(a2), "d"(a3), "r"(a4)
        : "rcx", "r11", "memory"
    );
    return result;
}

static size_t strlen(const char* str) {
    size_t len = 0;
    while (str[len]) len++;
    return len;
}

static void print(const char* str) {
    syscall(SYS_PRINT, reinterpret_cast<uint64_t>(str), strlen(str));
}

static void println(const char* str) {
    print(str);
    syscall(SYS_PRINT, reinterpret_cast<uint64_t>("\n"), 1);
}

static int strcmp(const char* a, const char* b) {
    while (*a && *b && *a == *b) { a++; b++; }
    return static_cast<int>(*a) - static_cast<int>(*b);
}

static int strncmp(const char* a, const char* b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (a[i] != b[i]) return static_cast<int>(a[i]) - static_cast<int>(b[i]);
        if (a[i] == 0) return 0;
    }
    return 0;
}

static char* strcpy(char* dst, const char* src) {
    char* d = dst;
    while ((*d++ = *src++));
    return dst;
}

static char* strcat(char* dst, const char* src) {
    char* d = dst + strlen(dst);
    while ((*d++ = *src++));
    return dst;
}

static void memset(void* ptr, int value, size_t size) {
    uint8_t* p = static_cast<uint8_t*>(ptr);
    for (size_t i = 0; i < size; i++) {
        p[i] = static_cast<uint8_t>(value);
    }
}

static char g_cwd[512] = "/";
static char g_input_line[1024];
static bool g_running = true;

struct TokenList {
    const char* tokens[16];
    int count;
};

static TokenList tokenize(char* line) {
    TokenList result;
    result.count = 0;
    memset(&result.tokens, 0, sizeof(result.tokens));

    while (*line == ' ' || *line == '\t') line++;

    while (*line && result.count < 16) {
        result.tokens[result.count++] = line;

        while (*line && *line != ' ' && *line != '\t') line++;

        if (*line) {
            *line++ = '\0';
            while (*line == ' ' || *line == '\t') line++;
        }
    }

    return result;
}

static void cmd_ls(const TokenList& args) {
    const char* path = (args.count >= 2) ? args.tokens[1] : g_cwd;

    int64_t fd = syscall(SYS_OPEN, reinterpret_cast<uint64_t>(path), 0);
    if (fd < 0) {
        print("ls: cannot open '");
        print(path);
        println("'");
        return;
    }

    char buffer[4096];
    int64_t bytes = syscall(SYS_GETDENTS, static_cast<uint64_t>(fd),
                            reinterpret_cast<uint64_t>(buffer), sizeof(buffer));

    if (bytes > 0) {
        println("  (directory listing not yet implemented)");
    } else if (bytes == 0) {
        println("  (empty directory)");
    } else {
        println("ls: error reading directory");
    }

    syscall(SYS_CLOSE, static_cast<uint64_t>(fd));
}

static void cmd_cat(const TokenList& args) {
    if (args.count < 2) {
        println("cat: missing file operand");
        return;
    }

    const char* path = args.tokens[1];

    int64_t fd = syscall(SYS_OPEN, reinterpret_cast<uint64_t>(path), 0);
    if (fd < 0) {
        print("cat: ");
        print(path);
        println(": No such file");
        return;
    }

    char buffer[512];
    while (true) {
        int64_t bytes = syscall(SYS_READ, static_cast<uint64_t>(fd),
                                reinterpret_cast<uint64_t>(buffer), sizeof(buffer));
        if (bytes <= 0) break;
        syscall(SYS_PRINT, reinterpret_cast<uint64_t>(buffer), static_cast<uint64_t>(bytes));
    }

    syscall(SYS_CLOSE, static_cast<uint64_t>(fd));
}

static void cmd_cd(const TokenList& args) {
    if (args.count < 2) {
        strcpy(g_cwd, "/");
        return;
    }

    const char* path = args.tokens[1];

    if (path[0] == '/') {
        if (strlen(path) < sizeof(g_cwd)) {
            strcpy(g_cwd, path);
        }
    } else {
        if (strlen(g_cwd) + strlen(path) + 2 < sizeof(g_cwd)) {
            if (strcmp(g_cwd, "/") != 0) {
                strcat(g_cwd, "/");
            }
            strcat(g_cwd, path);
        }
    }
}

static void cmd_pwd() {
    println(g_cwd);
}

static void cmd_mkdir(const TokenList& args) {
    if (args.count < 2) {
        println("mkdir: missing operand");
        return;
    }

    int64_t result = syscall(SYS_MKDIR,
        reinterpret_cast<uint64_t>(args.tokens[1]), 0755);

    if (result < 0) {
        print("mkdir: cannot create '");
        print(args.tokens[1]);
        println("'");
    }
}

static void cmd_mount(const TokenList& args) {
    if (args.count < 4) {
        println("mount: usage: mount <device> <path> <fstype>");
        return;
    }

    int64_t result = syscall(SYS_MOUNT,
        reinterpret_cast<uint64_t>(args.tokens[1]),
        reinterpret_cast<uint64_t>(args.tokens[2]),
        reinterpret_cast<uint64_t>(args.tokens[3]));

    if (result < 0) {
        println("mount: failed");
    } else {
        print("mount: ");
        print(args.tokens[1]);
        print(" mounted at ");
        println(args.tokens[2]);
    }
}

static void cmd_sysinfo() {
    println("DeltaOS v0.1.0");
    println("Kernel: Rust #![no_std] x86_64");
    println("Filesystem: NTFS (delta_ntfs)");
    println("Scheduler: Round-Robin with priorities");
    println("Memory: PMM (bitmap) + VMM (4-level paging)");
    println("Drivers: AHCI, NVMe, PS/2 Keyboard (C)");
    println("Shell: C++17 freestanding");

    int64_t pid = syscall(SYS_GETPID);
    print("Current PID: ");
    char pid_buf[16] = {0};
    int i = 14;
    if (pid == 0) {
        pid_buf[0] = '0';
    } else {
        int64_t p = pid;
        while (p > 0 && i >= 0) {
            pid_buf[i--] = '0' + (p % 10);
            p /= 10;
        }
        int shift = i + 1;
        for (int j = 0; j < 14 - shift; j++) {
            pid_buf[j] = pid_buf[j + shift];
            pid_buf[j + shift + 1] = 0;
        }
    }
    println(pid_buf);
}

static void cmd_help() {
    println("DeltaOS Shell — Available commands:");
    println("  ls [path]        - List directory contents");
    println("  cat <file>       - Display file contents");
    println("  cd <path>        - Change directory");
    println("  pwd              - Print working directory");
    println("  mkdir <dir>      - Create directory");
    println("  mount <dev> <path> <fstype> - Mount filesystem");
    println("  sysinfo          - Show system information");
    println("  help             - Show this help");
    println("  exit             - Exit shell");
}

static void cmd_exit() {
    println("Goodbye!");
    g_running = false;
}

static void cmd_unknown(const char* cmd) {
    print("shell: unknown command: ");
    println(cmd);
}

static void show_prompt() {
    print("deltaos:");
    print(g_cwd);
    print("$ ");
}

static void read_line(char* buffer, size_t max_len) {
    int64_t bytes = syscall(SYS_READLINE,
        reinterpret_cast<uint64_t>(buffer), max_len);

    if (bytes < 0) {
        buffer[0] = '\0';
        return;
    }

    size_t len = strlen(buffer);
    if (len > 0 && buffer[len - 1] == '\n') {
        buffer[len - 1] = '\0';
    }
}

static void execute_command(const TokenList& args) {
    if (args.count == 0) return;

    const char* cmd = args.tokens[0];

    if (strcmp(cmd, "ls") == 0)         cmd_ls(args);
    else if (strcmp(cmd, "cat") == 0)   cmd_cat(args);
    else if (strcmp(cmd, "cd") == 0)    cmd_cd(args);
    else if (strcmp(cmd, "pwd") == 0)   cmd_pwd();
    else if (strcmp(cmd, "mkdir") == 0) cmd_mkdir(args);
    else if (strcmp(cmd, "mount") == 0) cmd_mount(args);
    else if (strcmp(cmd, "sysinfo") == 0) cmd_sysinfo();
    else if (strcmp(cmd, "help") == 0)  cmd_help();
    else if (strcmp(cmd, "exit") == 0)  cmd_exit();
    else                                cmd_unknown(cmd);
}

extern "C" int shell_main() {
    println("");
    println("DeltaOS Shell v0.1.0");
    println("Type 'help' for available commands.");
    println("");

    while (g_running) {
        show_prompt();
        read_line(g_input_line, sizeof(g_input_line));

        TokenList args = tokenize(g_input_line);
        execute_command(args);
    }

    return 0;
}

#include <cstdint>
#include <cstddef>

#define SYS_READ      0
#define SYS_WRITE     1
#define SYS_OPEN      2
#define SYS_CLOSE     3
#define SYS_MKDIR     6
#define SYS_MOUNT     10
#define SYS_EXIT      21
#define SYS_GETPID    23
#define SYS_PRINT     100
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

static void exit(int code) {
    syscall(SYS_EXIT, static_cast<uint64_t>(code));
    __builtin_unreachable();
}

static uint64_t getpid() {
    return syscall(SYS_GETPID);
}

extern "C" int init_main() {
    println("==========================================");
    println("  DeltaOS Init v0.1.0");
    println("  First userspace process");
    println("==========================================");
    println("");

    print("[init] PID: ");
    char pid_str[16] = {0};
    uint64_t pid = getpid();
    {
        int i = 14;
        if (pid == 0) {
            pid_str[0] = '0';
        } else {
            while (pid > 0 && i >= 0) {
                pid_str[i--] = '0' + (pid % 10);
                pid /= 10;
            }
            int shift = i + 1;
            for (int j = 0; j < 14 - shift; j++) {
                pid_str[j] = pid_str[j + shift];
                pid_str[j + shift] = 0;
            }
        }
    }
    println(pid_str);

    println("[init] Mounting NTFS root filesystem...");

    int64_t mount_result = syscall(SYS_MOUNT,
        reinterpret_cast<uint64_t>("/dev/sda1"),
        reinterpret_cast<uint64_t>("/"),
        reinterpret_cast<uint64_t>("ntfs"));

    if (mount_result < 0) {
        println("[init] WARNING: NTFS mount failed, using ramfs");
        syscall(SYS_MOUNT,
            reinterpret_cast<uint64_t>("none"),
            reinterpret_cast<uint64_t>("/"),
            reinterpret_cast<uint64_t>("ramfs"));
    } else {
        println("[init] NTFS mounted at /");
    }

    println("[init] Creating base directories...");

    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/dev"), 0755);
    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/tmp"), 0777);
    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/proc"), 0555);
    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/mnt"), 0755);
    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/etc"), 0755);
    syscall(SYS_MKDIR, reinterpret_cast<uint64_t>("/home"), 0755);

    println("[init] Base directories created");

    syscall(SYS_MOUNT,
        reinterpret_cast<uint64_t>("none"),
        reinterpret_cast<uint64_t>("/dev"),
        reinterpret_cast<uint64_t>("devfs"));

    syscall(SYS_MOUNT,
        reinterpret_cast<uint64_t>("none"),
        reinterpret_cast<uint64_t>("/proc"),
        reinterpret_cast<uint64_t>("procfs"));

    println("[init] Virtual filesystems mounted");

    println("[init] Starting shell...");

    extern int shell_main();
    shell_main();

    println("[init] Shell exited, restarting...");

    while (true) {
        shell_main();
        println("[init] Shell crashed, respawning...");
    }

    return 0;
}

extern "C" void _start() {
    int result = init_main();
    exit(result);
}

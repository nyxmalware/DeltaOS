#include <cstdint>
#include <cstddef>
#include <new>

#define SYS_MMAP    30
#define SYS_MUNMAP  31

static inline int64_t syscall(uint64_t nr, uint64_t a1 = 0, uint64_t a2 = 0,
                               uint64_t a3 = 0) {
    int64_t result;
    __asm__ volatile (
        "int $0x80"
        : "=a"(result)
        : "a"(nr), "D"(a1), "S"(a2), "d"(a3)
        : "rcx", "r11", "memory"
    );
    return result;
}

namespace {
    uintptr_t g_heap_start = 0x500000;
    uintptr_t g_heap_current = 0x500000;
    uintptr_t g_heap_end = 0x800000;
}

void* operator new(size_t size) {
    if (size == 0) size = 1;

    size = (size + 15) & ~static_cast<size_t>(15);

    if (g_heap_current + size > g_heap_end) {
        int64_t result = syscall(SYS_MMAP, g_heap_end, size, 0x03);
        if (result >= 0) {
            g_heap_end += size;
        } else {
            return nullptr;
        }
    }

    void* ptr = reinterpret_cast<void*>(g_heap_current);
    g_heap_current += size;

    for (size_t i = 0; i < size; i++) {
        static_cast<uint8_t*>(ptr)[i] = 0;
    }

    return ptr;
}

void* operator new[](size_t size) {
    return ::operator new(size);
}

void* operator new(size_t size, std::align_val_t align) {
    if (size == 0) size = 1;

    size_t alignment = static_cast<size_t>(align);

    uintptr_t aligned = (g_heap_current + alignment - 1) & ~(alignment - 1);
    size_t padding = aligned - g_heap_current;
    size_t total = padding + size;

    total = (total + 15) & ~static_cast<size_t>(15);

    if (g_heap_current + total > g_heap_end) {
        int64_t result = syscall(SYS_MMAP, g_heap_end, total, 0x03);
        if (result >= 0) {
            g_heap_end += total;
        } else {
            return nullptr;
        }
    }

    void* ptr = reinterpret_cast<void*>(aligned);
    g_heap_current += total;

    for (size_t i = 0; i < size; i++) {
        static_cast<uint8_t*>(ptr)[i] = 0;
    }

    return ptr;
}

void* operator new[](size_t size, std::align_val_t align) {
    return ::operator new(size, align);
}

void operator delete(void* ptr) noexcept {
    (void)ptr;
}

void operator delete(void* ptr, size_t size) noexcept {
    (void)ptr;
    (void)size;
}

void operator delete[](void* ptr) noexcept {
    (void)ptr;
}

void operator delete[](void* ptr, size_t size) noexcept {
    (void)ptr;
    (void)size;
}

void operator delete(void* ptr, std::align_val_t align) noexcept {
    (void)ptr;
    (void)align;
}

void operator delete[](void* ptr, std::align_val_t align) noexcept {
    (void)ptr;
    (void)align;
}

void operator delete(void* ptr, size_t size, std::align_val_t align) noexcept {
    (void)ptr;
    (void)size;
    (void)align;
}

void operator delete[](void* ptr, size_t size, std::align_val_t align) noexcept {
    (void)ptr;
    (void)size;
    (void)align;
}

void* operator new(size_t size, const std::nothrow_t&) noexcept {
    return ::operator new(size);
}

void* operator new[](size_t size, const std::nothrow_t&) noexcept {
    return ::operator new[](size);
}

void* operator new(size_t size, std::align_val_t align, const std::nothrow_t&) noexcept {
    return ::operator new(size, align);
}

void* operator new[](size_t size, std::align_val_t align, const std::nothrow_t&) noexcept {
    return ::operator new[](size, align);
}

extern "C" {

void* memcpy(void* dest, const void* src, size_t n) {
    uint8_t* d = static_cast<uint8_t*>(dest);
    const uint8_t* s = static_cast<const uint8_t*>(src);
    for (size_t i = 0; i < n; i++) {
        d[i] = s[i];
    }
    return dest;
}

void* memmove(void* dest, const void* src, size_t n) {
    uint8_t* d = static_cast<uint8_t*>(dest);
    const uint8_t* s = static_cast<const uint8_t*>(src);

    if (d < s) {
        for (size_t i = 0; i < n; i++) {
            d[i] = s[i];
        }
    } else if (d > s) {
        for (size_t i = n; i > 0; i--) {
            d[i - 1] = s[i - 1];
        }
    }
    return dest;
}

void* memset(void* s, int c, size_t n) {
    uint8_t* p = static_cast<uint8_t*>(s);
    for (size_t i = 0; i < n; i++) {
        p[i] = static_cast<uint8_t>(c);
    }
    return s;
}

int memcmp(const void* s1, const void* s2, size_t n) {
    const uint8_t* a = static_cast<const uint8_t*>(s1);
    const uint8_t* b = static_cast<const uint8_t*>(s2);
    for (size_t i = 0; i < n; i++) {
        if (a[i] != b[i]) {
            return static_cast<int>(a[i]) - static_cast<int>(b[i]);
        }
    }
    return 0;
}

size_t strlen(const char* s) {
    size_t len = 0;
    while (s[len]) len++;
    return len;
}

}

#include <stdint.h>

#define PS2_DATA_PORT       0x60
#define PS2_STATUS_PORT     0x64
#define PS2_COMMAND_PORT    0x64

#define PS2_CMD_READ_CONFIG    0x20
#define PS2_CMD_WRITE_CONFIG   0x60
#define PS2_CMD_DISABLE_PORT2  0xA7
#define PS2_CMD_ENABLE_PORT2   0xA8
#define PS2_CMD_TEST_CTRL      0xAA
#define PS2_CMD_TEST_PORT1     0xAB
#define PS2_CMD_DISABLE_PORT1  0xAD
#define PS2_CMD_ENABLE_PORT1   0xAE
#define PS2_CMD_SEND_PORT2     0xD4

#define KB_CMD_SET_LED         0xED
#define KB_CMD_ECHO            0xEE
#define KB_CMD_SET_SCANCODE    0xF0
#define KB_CMD_GET_ID          0xF2
#define KB_CMD_SET_RATE        0xF3
#define KB_CMD_ENABLE          0xF4
#define KB_CMD_DISABLE         0xF5
#define KB_CMD_RESET           0xFF

#define KB_ACK                 0xFA
#define KB_RESEND              0xFE
#define KB_ERROR               0xFC

#define SCANCODE_ESCAPE        0x01
#define SCANCODE_LSHIFT        0x2A
#define SCANCODE_RSHIFT        0x36
#define SCANCODE_CAPS_LOCK     0x3A
#define SCANCODE_CTRL          0x1D
#define SCANCODE_ALT           0x38
#define SCANCODE_EXTENDED      0xE0
#define SCANCODE_RELEASED      0x80

#define KB_BUFFER_SIZE         256

static inline void outb(uint16_t port, uint8_t val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}

static inline uint8_t inb(uint16_t port) {
    uint8_t ret;
    __asm__ volatile ("inb %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

static volatile uint8_t g_shift_pressed = 0;
static volatile uint8_t g_ctrl_pressed = 0;
static volatile uint8_t g_alt_pressed = 0;
static volatile uint8_t g_caps_lock = 0;
static volatile uint8_t g_extended_key = 0;

static volatile char    g_key_buffer[KB_BUFFER_SIZE];
static volatile uint32_t g_key_buffer_head = 0;
static volatile uint32_t g_key_buffer_tail = 0;

static const char scancode_to_ascii[128] = {
    0, 27, '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
    '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0, 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
    0, '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0,
    '*', 0, ' ',
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, '-', 0, 0, 0, '+', 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0
};

static const char scancode_to_ascii_shift[128] = {
    0, 27, '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\b',
    '\t', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n',
    0, 'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"', '~',
    0, '|', 'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?', 0,
    '*', 0, ' ',
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, '-', 0, 0, 0, '+', 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0
};

static void ps2_wait_input(void) {
    int timeout = 100000;
    while (timeout-- > 0) {
        uint8_t status = inb(PS2_STATUS_PORT);
        if (!(status & 0x02)) {
            return;
        }
    }
}

static void ps2_wait_output(void) {
    int timeout = 100000;
    while (timeout-- > 0) {
        uint8_t status = inb(PS2_STATUS_PORT);
        if (status & 0x01) {
            return;
        }
    }
}

static int kb_send_command(uint8_t cmd) {
    int retries = 3;
    while (retries-- > 0) {
        ps2_wait_input();
        outb(PS2_DATA_PORT, cmd);

        ps2_wait_output();
        uint8_t response = inb(PS2_DATA_PORT);
        if (response == KB_ACK) {
            return 0;
        }
    }
    return -1;
}

static void kb_buffer_put(char ch) {
    uint32_t next_head = (g_key_buffer_head + 1) % KB_BUFFER_SIZE;
    if (next_head != g_key_buffer_tail) {
        g_key_buffer[g_key_buffer_head] = ch;
        g_key_buffer_head = next_head;
    }
}

static void kb_handle_scancode(uint8_t scancode) {
    if (scancode == SCANCODE_EXTENDED) {
        g_extended_key = 1;
        return;
    }

    uint8_t released = (scancode & SCANCODE_RELEASED) != 0;
    uint8_t key = scancode & ~SCANCODE_RELEASED;

    if (key == SCANCODE_LSHIFT || key == SCANCODE_RSHIFT) {
        g_shift_pressed = !released;
        g_extended_key = 0;
        return;
    }

    if (key == SCANCODE_CTRL) {
        g_ctrl_pressed = !released;
        g_extended_key = 0;
        return;
    }

    if (key == SCANCODE_ALT) {
        g_alt_pressed = !released;
        g_extended_key = 0;
        return;
    }

    if (key == SCANCODE_CAPS_LOCK && !released) {
        g_caps_lock = !g_caps_lock;
        g_extended_key = 0;
        return;
    }

    if (released) {
        g_extended_key = 0;
        return;
    }

    char ch = 0;

    if (key < 128) {
        if (g_shift_pressed) {
            ch = scancode_to_ascii_shift[key];
            if (g_caps_lock && ch >= 'A' && ch <= 'Z') {
                ch += 32;
            }
        } else {
            ch = scancode_to_ascii[key];
            if (g_caps_lock && ch >= 'a' && ch <= 'z') {
                ch -= 32;
            }
        }
    }

    if (g_ctrl_pressed && ch >= 'a' && ch <= 'z') {
        ch = ch - 'a' + 1;
    }

    if (ch != 0) {
        kb_buffer_put(ch);
    }

    g_extended_key = 0;
}

int keyboard_init(void) {
    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_DISABLE_PORT1);
    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_DISABLE_PORT2);

    while (inb(PS2_STATUS_PORT) & 0x01) {
        inb(PS2_DATA_PORT);
    }

    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_TEST_CTRL);
    ps2_wait_output();
    uint8_t test_result = inb(PS2_DATA_PORT);
    if (test_result != 0x55) {
        return -1;
    }

    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_ENABLE_PORT1);

    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_READ_CONFIG);
    ps2_wait_output();
    uint8_t config = inb(PS2_DATA_PORT);

    config |= 0x01;
    config &= ~0x10;
    config &= ~0x20;

    ps2_wait_input();
    outb(PS2_COMMAND_PORT, PS2_CMD_WRITE_CONFIG);
    ps2_wait_input();
    outb(PS2_DATA_PORT, config);

    if (kb_send_command(KB_CMD_RESET) != 0) {
        return -1;
    }

    ps2_wait_output();
    uint8_t bat_result = inb(PS2_DATA_PORT);
    if (bat_result != 0xAA) {
        return -1;
    }

    kb_send_command(KB_CMD_SET_SCANCODE);
    kb_send_command(0x01);

    kb_send_command(KB_CMD_ENABLE);

    kb_send_command(KB_CMD_SET_RATE);
    kb_send_command(0x00);

    g_key_buffer_head = 0;
    g_key_buffer_tail = 0;
    g_shift_pressed = 0;
    g_ctrl_pressed = 0;
    g_alt_pressed = 0;
    g_caps_lock = 0;
    g_extended_key = 0;

    return 0;
}

uint8_t keyboard_read_scancode(void) {
    uint8_t scancode = inb(PS2_DATA_PORT);
    kb_handle_scancode(scancode);
    return scancode;
}

char keyboard_get_char(void) {
    if (g_key_buffer_head == g_key_buffer_tail) {
        return 0;
    }

    char ch = g_key_buffer[g_key_buffer_tail];
    g_key_buffer_tail = (g_key_buffer_tail + 1) % KB_BUFFER_SIZE;
    return ch;
}

int keyboard_has_data(void) {
    return g_key_buffer_head != g_key_buffer_tail;
}

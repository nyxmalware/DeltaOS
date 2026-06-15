#include <stdint.h>

#define PCI_VENDOR_INTEL       0x8086
#define PCI_CLASS_MASS_STORAGE 0x01
#define PCI_SUBCLASS_AHCI      0x06

#define AHCI_GHC              0x04
#define AHCI_IS               0x08
#define AHCI_PI               0x0C
#define AHCI_VS               0x10
#define AHCI_CCC_CTL          0x14
#define AHCI_CAP              0x00

#define AHCI_PORT_CLB         0x00
#define AHCI_PORT_CLBU        0x04
#define AHCI_PORT_FB          0x08
#define AHCI_PORT_FBU         0x0C
#define AHCI_PORT_IS          0x10
#define AHCI_PORT_IE          0x14
#define AHCI_PORT_CMD         0x18
#define AHCI_PORT_TFD         0x20
#define AHCI_PORT_SIG         0x24
#define AHCI_PORT_SSTS        0x28
#define AHCI_PORT_SCTL        0x2C
#define AHCI_PORT_SERR        0x30
#define AHCI_PORT_SACT        0x34
#define AHCI_PORT_CI          0x38
#define AHCI_PORT_SNTF        0x3C

#define AHCI_PORT_SIZE        0x80
#define AHCI_CMD_SLOT_COUNT   32
#define AHCI_CMD_TABLE_SIZE   256
#define AHCI_PRDT_ENTRIES     8
#define AHCI_SECTOR_SIZE      512

#define AHCI_GHC_AE           (1 << 31)
#define AHCI_GHC_MRSM         (1 << 2)
#define AHCI_GHC_IE           (1 << 1)
#define AHCI_GHC_HR           (1 << 0)

#define AHCI_PORT_CMD_ST      (1 << 0)
#define AHCI_PORT_CMD_SUD     (1 << 1)
#define AHCI_PORT_CMD_POD     (1 << 2)
#define AHCI_PORT_CMD_CLO     (1 << 3)
#define AHCI_PORT_CMD_FRE     (1 << 4)
#define AHCI_PORT_CMD_FR      (1 << 14)
#define AHCI_PORT_CMD_CR      (1 << 15)

#define AHCI_SIG_SATA         0x00000101
#define AHCI_SIG_SATAPI       0xEB140101
#define AHCI_SIG_SEMB         0xC33C0101
#define AHCI_SIG_PM           0x96690101

typedef enum {
    AHCI_DEV_NULL = 0,
    AHCI_DEV_SATA,
    AHCI_DEV_SATAPI,
    AHCI_DEV_SEMB,
    AHCI_DEV_PM
} ahci_device_type_t;

typedef struct __attribute__((packed)) {
    uint8_t  cfl:5;
    uint8_t  a:1;
    uint8_t  w:1;
    uint8_t  p:1;
    uint8_t  rsv0:8;
    uint16_t prdtl;
    uint32_t prdbc;
    uint32_t ctba;
    uint32_t ctbau;
    uint32_t rsv1[4];
} ahci_cmd_header_t;

typedef struct __attribute__((packed)) {
    uint32_t dba;
    uint32_t dbau;
    uint32_t rsv0;
    uint32_t dbc:22;
    uint32_t rsv1:9;
    uint32_t i:1;
} ahci_prdt_entry_t;

typedef struct __attribute__((packed)) {
    uint8_t  cfis[64];
    uint8_t  acmd[16];
    uint8_t  rsv[48];
    ahci_prdt_entry_t prdt[AHCI_PRDT_ENTRIES];
} ahci_cmd_table_t;

typedef struct __attribute__((packed)) {
    uint8_t  fis_type;
    uint8_t  pm_port:4;
    uint8_t  rsv0:4;
    uint8_t  rsv1;
    uint8_t  command;
    uint8_t  features0;
    uint8_t  lba0;
    uint8_t  lba1;
    uint8_t  lba2;
    uint8_t  device;
    uint8_t  lba3;
    uint8_t  lba4;
    uint8_t  lba5;
    uint8_t  features1;
    uint8_t  count0;
    uint8_t  count1;
    uint8_t  rsv2;
    uint8_t  control;
    uint32_t rsv3;
} ahci_fis_t;

typedef struct {
    int              port_num;
    volatile void   *base;
    ahci_device_type_t dev_type;
    ahci_cmd_header_t *cmd_list;
    ahci_cmd_table_t  *cmd_table;
    void             *fis_recv;
    int              cmd_slot;
} ahci_port_t;

typedef struct {
    volatile void   *base;
    uint32_t         cap;
    uint32_t         cap2;
    uint32_t         version;
    uint32_t         port_count;
    ahci_port_t      ports[32];
} ahci_controller_t;

static ahci_controller_t g_ahci = {0};

static inline void outb(uint16_t port, uint8_t val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}

static inline uint8_t inb(uint16_t port) {
    uint8_t ret;
    __asm__ volatile ("inb %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

static inline void outl(uint16_t port, uint32_t val) {
    __asm__ volatile ("outl %0, %1" : : "a"(val), "Nd"(port));
}

static inline uint32_t inl(uint16_t port) {
    uint32_t ret;
    __asm__ volatile ("inl %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}

static inline uint64_t read64(volatile void *addr) {
    return *(volatile uint64_t *)addr;
}

static inline void write64(volatile void *addr, uint64_t val) {
    *(volatile uint64_t *)addr = val;
}

static inline uint32_t read32(volatile void *addr) {
    return *(volatile uint32_t *)addr;
}

static inline void write32(volatile void *addr, uint32_t val) {
    *(volatile uint32_t *)addr = val;
}

static ahci_device_type_t ahci_get_device_type(volatile void *port_base) {
    uint32_t ssts = read32((void *)((uintptr_t)port_base + AHCI_PORT_SSTS));
    uint32_t sig = read32((void *)((uintptr_t)port_base + AHCI_PORT_SIG));

    uint8_t det = ssts & 0x0F;
    uint8_t ipm = (ssts >> 8) & 0x0F;

    if (det != 0x03 || ipm != 0x01) {
        return AHCI_DEV_NULL;
    }

    switch (sig) {
        case AHCI_SIG_SATA:   return AHCI_DEV_SATA;
        case AHCI_SIG_SATAPI: return AHCI_DEV_SATAPI;
        case AHCI_SIG_SEMB:   return AHCI_DEV_SEMB;
        case AHCI_SIG_PM:     return AHCI_DEV_PM;
        default:              return AHCI_DEV_NULL;
    }
}

static int ahci_stop_cmd_engine(volatile void *port_base) {
    uint32_t cmd = read32((void *)((uintptr_t)port_base + AHCI_PORT_CMD));

    write32((void *)((uintptr_t)port_base + AHCI_PORT_CMD), cmd & ~AHCI_PORT_CMD_ST);
    write32((void *)((uintptr_t)port_base + AHCI_PORT_CMD), cmd & ~AHCI_PORT_CMD_FRE);

    int timeout = 500000;
    while (timeout-- > 0) {
        cmd = read32((void *)((uintptr_t)port_base + AHCI_PORT_CMD));
        if (!(cmd & AHCI_PORT_CMD_FR) && !(cmd & AHCI_PORT_CMD_CR)) {
            return 0;
        }
    }

    return -1;
}

static int ahci_start_cmd_engine(volatile void *port_base) {
    int timeout = 500000;
    while (timeout-- > 0) {
        uint32_t cmd = read32((void *)((uintptr_t)port_base + AHCI_PORT_CMD));
        if (!(cmd & AHCI_PORT_CMD_CR) && !(cmd & AHCI_PORT_CMD_FR)) {
            break;
        }
    }

    uint32_t cmd = read32((void *)((uintptr_t)port_base + AHCI_PORT_CMD));
    write32((void *)((uintptr_t)port_base + AHCI_PORT_CMD), cmd | AHCI_PORT_CMD_FRE);

    cmd = read32((void *)((uintptr_t)port_base + AHCI_PORT_CMD));
    write32((void *)((uintptr_t)port_base + AHCI_PORT_CMD), cmd | AHCI_PORT_CMD_ST);

    return 0;
}

static int ahci_init_port(ahci_port_t *port, int port_num, volatile void *base) {
    port->port_num = port_num;
    port->base = base;
    port->dev_type = ahci_get_device_type(base);

    if (port->dev_type == AHCI_DEV_NULL) {
        return 0;
    }

    ahci_stop_cmd_engine(base);

    uintptr_t cmd_list_addr = 0x400000 + port_num * 0x2000;
    for (int i = 0; i < 1024; i++) {
        ((volatile uint8_t *)cmd_list_addr)[i] = 0;
    }
    port->cmd_list = (ahci_cmd_header_t *)cmd_list_addr;

    write32((void *)((uintptr_t)base + AHCI_PORT_CLB), (uint32_t)cmd_list_addr);
    write32((void *)((uintptr_t)base + AHCI_PORT_CLBU), 0);

    uintptr_t fis_addr = cmd_list_addr + 1024;
    for (int i = 0; i < 256; i++) {
        ((volatile uint8_t *)fis_addr)[i] = 0;
    }
    port->fis_recv = (void *)fis_addr;

    write32((void *)((uintptr_t)base + AHCI_PORT_FB), (uint32_t)fis_addr);
    write32((void *)((uintptr_t)base + AHCI_PORT_FBU), 0);

    uintptr_t cmd_table_addr = cmd_list_addr + 4096;
    for (int i = 0; i < AHCI_CMD_SLOT_COUNT; i++) {
        port->cmd_list[i].prdtl = AHCI_PRDT_ENTRIES;
        port->cmd_list[i].ctba = (uint32_t)(cmd_table_addr + i * AHCI_CMD_TABLE_SIZE);
        port->cmd_list[i].ctbau = 0;
    }
    port->cmd_table = (ahci_cmd_table_t *)cmd_table_addr;

    ahci_start_cmd_engine(base);

    return 0;
}

int ahci_init(void) {
    g_ahci.base = (volatile void *)0xFEC00000;

    g_ahci.cap = read32((void *)((uintptr_t)g_ahci.base + AHCI_CAP));
    g_ahci.version = read32((void *)((uintptr_t)g_ahci.base + AHCI_VS));

    uint32_t ghc = read32((void *)((uintptr_t)g_ahci.base + AHCI_GHC));
    write32((void *)((uintptr_t)g_ahci.base + AHCI_GHC), ghc | AHCI_GHC_AE);

    uint32_t pi = read32((void *)((uintptr_t)g_ahci.base + AHCI_PI));

    g_ahci.port_count = 0;
    for (int i = 0; i < 32; i++) {
        if (pi & (1 << i)) {
            volatile void *port_base = (void *)((uintptr_t)g_ahci.base + 0x100 + i * AHCI_PORT_SIZE);
            ahci_init_port(&g_ahci.ports[i], i, port_base);
            if (g_ahci.ports[i].dev_type != AHCI_DEV_NULL) {
                g_ahci.port_count++;
            }
        }
    }

    return g_ahci.port_count > 0 ? 0 : -1;
}

int nvme_detect(void) {
    for (uint16_t bus = 0; bus < 256; bus++) {
        for (uint8_t dev = 0; dev < 32; dev++) {
            for (uint8_t func = 0; func < 8; func++) {
                uint32_t addr = (1 << 31) | (bus << 16) | (dev << 11) | (func << 8);
                outl(0xCF8, addr);
                uint32_t vendor = inl(0xCFC);

                if (vendor == 0xFFFFFFFF || vendor == 0x00000000) {
                    continue;
                }

                outl(0xCF8, addr | 0x09);
                uint8_t class_code = inb(0xCFC + 1);
                uint8_t subclass = inb(0xCFC);

                if (class_code == 0x01 && subclass == 0x08) {
                    return 0;
                }
            }
        }
    }

    return -1;
}

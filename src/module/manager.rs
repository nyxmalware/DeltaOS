use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::KernelResult;
use crate::KernelError;

static NEXT_MODULE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Loaded,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Driver,
    FileSystem,
    Network,
    Extension,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub id: u64,
    pub name: String,
    pub version: (u16, u16, u16),
    pub module_type: ModuleType,
    pub state: ModuleState,
    pub dependencies: Vec<String>,
    pub size: usize,
    pub init_addr: usize,
    pub cleanup_addr: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleSymbol {
    pub name: String,
    pub addr: usize,
    pub symbol_type: SymbolType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Function,
    Data,
    Bss,
    Unknown,
}

pub struct SymbolTable {
    symbols: BTreeMap<String, usize>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            symbols: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, addr: usize) {
        self.symbols.insert(name.to_string(), addr);
    }

    pub fn lookup(&self, name: &str) -> Option<usize> {
        self.symbols.get(name).copied()
    }

    pub fn all_symbols(&self) -> Vec<ModuleSymbol> {
        self.symbols.iter().map(|(name, &addr)| {
            ModuleSymbol {
                name: name.clone(),
                addr,
                symbol_type: SymbolType::Unknown,
            }
        }).collect()
    }
}

pub struct ModuleManager {
    modules: BTreeMap<u64, ModuleInfo>,
    symbol_table: SymbolTable,
    ramdisk_addr: usize,
    ramdisk_size: usize,
}

impl ModuleManager {
    pub fn new() -> Self {
        let mut mgr = ModuleManager {
            modules: BTreeMap::new(),
            symbol_table: SymbolTable::new(),
            ramdisk_addr: 0,
            ramdisk_size: 0,
        };

        mgr.register_kernel_symbols();

        mgr
    }

    fn register_kernel_symbols(&mut self) {
        self.symbol_table.register("kprint", 0);
        self.symbol_table.register("inb", crate::inb as *const () as usize);
        self.symbol_table.register("outb", crate::outb as *const () as usize);
        self.symbol_table.register("inl", crate::inl as *const () as usize);
        self.symbol_table.register("outl", crate::outl as *const () as usize);

        self.symbol_table.register("kmalloc", 0);
        self.symbol_table.register("kfree", 0);

        self.symbol_table.register("schedule", 0);
        self.symbol_table.register("yield", 0);

        self.symbol_table.register("vfs_open", 0);
        self.symbol_table.register("vfs_read", 0);
        self.symbol_table.register("vfs_write", 0);
        self.symbol_table.register("vfs_close", 0);
    }

    pub fn set_ramdisk(&mut self, addr: usize, size: usize) {
        self.ramdisk_addr = addr;
        self.ramdisk_size = size;
    }

    pub fn load_module(&mut self, name: &str) -> KernelResult<u64> {
        for (_, info) in &self.modules {
            if info.name == name {
                return Err(KernelError::PageAlreadyMapped);
            }
        }

        let id = NEXT_MODULE_ID.fetch_add(1, Ordering::SeqCst);

        let info = ModuleInfo {
            id,
            name: name.to_string(),
            version: (0, 1, 0),
            module_type: ModuleType::Driver,
            state: ModuleState::Loaded,
            dependencies: Vec::new(),
            size: 0,
            init_addr: 0,
            cleanup_addr: 0,
        };

        self.modules.insert(id, info);
        Ok(id)
    }

    pub fn init_module(&mut self, module_id: u64) -> KernelResult<()> {
        let info = self.modules.get(&module_id)
            .ok_or(KernelError::ModuleNotFound)?
            .clone();

        if info.state != ModuleState::Loaded {
            return Err(KernelError::InvalidArgument);
        }

        for dep_name in &info.dependencies {
            let found = self.modules.values().any(|m| m.name == *dep_name && m.state == ModuleState::Running);
            if !found {
                return Err(KernelError::ModuleNotFound);
            }
        }

        if info.init_addr != 0 {
            let init_fn: extern "C" fn() -> i32 = unsafe {
                core::mem::transmute(info.init_addr)
            };
            let result = init_fn();
            if result != 0 {
                self.modules.get_mut(&module_id)
                    .ok_or(KernelError::ModuleNotFound)?
                    .state = ModuleState::Error;
                return Err(KernelError::GeneralFault);
            }
        }

        self.modules.get_mut(&module_id)
            .ok_or(KernelError::ModuleNotFound)?
            .state = ModuleState::Running;
        Ok(())
    }

    pub fn unload_module(&mut self, module_id: u64) -> KernelResult<()> {
        let info = self.modules.get_mut(&module_id)
            .ok_or(KernelError::ModuleNotFound)?;

        if info.state != ModuleState::Running && info.state != ModuleState::Stopped {
            return Err(KernelError::InvalidArgument);
        }

        if info.cleanup_addr != 0 {
            let cleanup_fn: extern "C" fn() = unsafe {
                core::mem::transmute(info.cleanup_addr)
            };
            cleanup_fn();
        }

        self.modules.remove(&module_id);
        Ok(())
    }

    pub fn get_module_info(&self, module_id: u64) -> Option<&ModuleInfo> {
        self.modules.get(&module_id)
    }

    pub fn list_modules(&self) -> Vec<&ModuleInfo> {
        self.modules.values().collect()
    }

    pub fn resolve_symbol(&self, name: &str) -> Option<usize> {
        self.symbol_table.lookup(name)
    }

    pub fn register_symbol(&mut self, name: &str, addr: usize) {
        self.symbol_table.register(name, addr);
    }
}

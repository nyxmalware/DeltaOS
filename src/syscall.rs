use crate::{KernelResult, KernelError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNumber {
    Read = 0,
    Write = 1,
    Open = 2,
    Close = 3,
    Seek = 4,
    Stat = 5,
    Mkdir = 6,
    Unlink = 7,
    Getdents = 8,
    Mount = 10,
    Umount = 11,
    Clone = 20,
    Exit = 21,
    Waitpid = 22,
    Getpid = 23,
    Yield = 24,
    Mmap = 30,
    Munmap = 31,
    Print = 100,
    Readline = 101,
    Sysinfo = 200,
}

impl From<u64> for SyscallNumber {
    fn from(v: u64) -> Self {
        match v {
            0 => SyscallNumber::Read,
            1 => SyscallNumber::Write,
            2 => SyscallNumber::Open,
            3 => SyscallNumber::Close,
            4 => SyscallNumber::Seek,
            5 => SyscallNumber::Stat,
            6 => SyscallNumber::Mkdir,
            7 => SyscallNumber::Unlink,
            8 => SyscallNumber::Getdents,
            10 => SyscallNumber::Mount,
            11 => SyscallNumber::Umount,
            20 => SyscallNumber::Clone,
            21 => SyscallNumber::Exit,
            22 => SyscallNumber::Waitpid,
            23 => SyscallNumber::Getpid,
            24 => SyscallNumber::Yield,
            30 => SyscallNumber::Mmap,
            31 => SyscallNumber::Munmap,
            100 => SyscallNumber::Print,
            101 => SyscallNumber::Readline,
            200 => SyscallNumber::Sysinfo,
            _ => SyscallNumber::Read,
        }
    }
}

pub type SyscallResult = i64;

pub fn init() {}

#[no_mangle]
pub extern "C" fn rust_syscall_handler(
    nr: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    _arg4: u64,
) -> SyscallResult {
    let syscall = SyscallNumber::from(nr);

    let result = match syscall {
        SyscallNumber::Write => sys_write(arg1, arg2 as *const u8, arg3 as usize),
        SyscallNumber::Print => sys_print(arg1 as *const u8, arg2 as usize),
        SyscallNumber::Getpid => sys_getpid(),
        SyscallNumber::Yield => sys_yield(),
        SyscallNumber::Close => sys_close(arg1),
        SyscallNumber::Munmap => sys_munmap(arg1, arg2 as usize),
        SyscallNumber::Exit => sys_exit(arg1 as i32),
        _ => Err(KernelError::IoError),
    };

    match result {
        Ok(val) => val as SyscallResult,
        Err(e) => isize::from(e) as SyscallResult,
    }
}

fn sys_write(fd: u64, buffer: *const u8, count: usize) -> KernelResult<u64> {
    if fd == 1 || fd == 2 {
        let slice = unsafe { core::slice::from_raw_parts(buffer, count) };
        for &byte in slice {
            crate::vga_print_char(byte);
        }
        return Ok(count as u64);
    }
    Err(KernelError::IoError)
}

fn sys_close(_fd: u64) -> KernelResult<u64> {
    Ok(0)
}

fn sys_getpid() -> KernelResult<u64> {
    Ok(1)
}

fn sys_yield() -> KernelResult<u64> {
    Ok(0)
}

fn sys_munmap(_addr: u64, _size: usize) -> KernelResult<u64> {
    Ok(0)
}

fn sys_exit(_code: i32) -> KernelResult<u64> {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

fn sys_print(buffer: *const u8, count: usize) -> KernelResult<u64> {
    let slice = unsafe { core::slice::from_raw_parts(buffer, count) };
    for &byte in slice {
        crate::vga_print_char(byte);
    }
    Ok(count as u64)
}

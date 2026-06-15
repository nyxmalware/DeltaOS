use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::fmt;

use crate::KernelResult;
use crate::KernelError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystemType {
    NTFS,
    FAT32,
    RamFs,
    ProcFs,
    DevFs,
    Unknown,
}

impl fmt::Display for FileSystemType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FileSystemType::NTFS => write!(f, "ntfs"),
            FileSystemType::FAT32 => write!(f, "fat32"),
            FileSystemType::RamFs => write!(f, "ramfs"),
            FileSystemType::ProcFs => write!(f, "procfs"),
            FileSystemType::DevFs => write!(f, "devfs"),
            FileSystemType::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    Device,
    Pipe,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct FilePermissions {
    pub owner_read: bool,
    pub owner_write: bool,
    pub owner_execute: bool,
    pub group_read: bool,
    pub group_write: bool,
    pub group_execute: bool,
    pub others_read: bool,
    pub others_write: bool,
    pub others_execute: bool,
}

impl Default for FilePermissions {
    fn default() -> Self {
        FilePermissions {
            owner_read: true,
            owner_write: true,
            owner_execute: false,
            group_read: true,
            group_write: false,
            group_execute: false,
            others_read: true,
            others_write: false,
            others_execute: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub inode: u64,
    pub file_type: FileType,
    pub size: u64,
    pub permissions: FilePermissions,
    pub created: u64,
    pub modified: u64,
    pub accessed: u64,
    pub nlinks: u64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: FileType,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct OpenFlags: u32 {
        const READ_ONLY = 0x01;
        const WRITE_ONLY = 0x02;
        const READ_WRITE = 0x03;
        const CREATE = 0x10;
        const TRUNCATE = 0x20;
        const APPEND = 0x40;
        const EXCLUSIVE = 0x80;
    }
}

pub type FileOffset = i64;

pub trait Inode: Send + Sync {
    fn metadata(&self) -> KernelResult<FileMetadata>;
    fn read(&self, offset: FileOffset, buffer: &mut [u8]) -> KernelResult<usize>;
    fn write(&mut self, offset: FileOffset, data: &[u8]) -> KernelResult<usize>;
    fn readdir(&self) -> KernelResult<Vec<DirEntry>>;
    fn lookup(&self, name: &str) -> KernelResult<Box<dyn Inode>>;
    fn create(&mut self, name: &str, file_type: FileType) -> KernelResult<Box<dyn Inode>>;
    fn unlink(&mut self, name: &str) -> KernelResult<()>;
    fn truncate(&mut self, size: u64) -> KernelResult<()>;
    fn clone_inode(&self) -> Box<dyn Inode>;
}

pub trait FileSystem: Send + Sync {
    fn fs_type(&self) -> FileSystemType;
    fn label(&self) -> &str;
    fn root_inode(&self) -> Box<dyn Inode>;
    fn get_inode(&self, ino: u64) -> KernelResult<Box<dyn Inode>>;
    fn sync(&self) -> KernelResult<()>;
    fn statfs(&self) -> KernelResult<FileSystemStats>;
}

#[derive(Debug, Clone)]
pub struct FileSystemStats {
    pub total_size: u64,
    pub free_space: u64,
    pub total_inodes: u64,
    pub free_inodes: u64,
    pub block_size: u64,
    pub max_name_length: u32,
}

struct MountPoint {
    path: String,
    fs: Box<dyn FileSystem>,
    flags: MountFlags,
}

#[derive(Debug, Clone, Copy)]
pub struct MountFlags {
    pub read_only: bool,
    pub no_exec: bool,
    pub no_suid: bool,
}

impl Default for MountFlags {
    fn default() -> Self {
        MountFlags {
            read_only: false,
            no_exec: false,
            no_suid: false,
        }
    }
}

pub struct FileDescriptor {
    inode: Box<dyn Inode>,
    flags: OpenFlags,
    offset: FileOffset,
}

impl FileDescriptor {
    pub fn new(inode: Box<dyn Inode>, flags: OpenFlags) -> Self {
        FileDescriptor {
            inode,
            flags,
            offset: 0,
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> KernelResult<usize> {
        if !self.flags.contains(OpenFlags::READ_ONLY) && !self.flags.contains(OpenFlags::READ_WRITE) {
            return Err(KernelError::AccessDenied);
        }
        let bytes_read = self.inode.read(self.offset, buffer)?;
        self.offset += bytes_read as FileOffset;
        Ok(bytes_read)
    }

    pub fn write(&mut self, data: &[u8]) -> KernelResult<usize> {
        if !self.flags.contains(OpenFlags::WRITE_ONLY) && !self.flags.contains(OpenFlags::READ_WRITE) {
            return Err(KernelError::AccessDenied);
        }
        let bytes_written = self.inode.write(self.offset, data)?;
        self.offset += bytes_written as FileOffset;
        Ok(bytes_written)
    }

    pub fn seek(&mut self, offset: FileOffset) -> KernelResult<FileOffset> {
        self.offset = offset;
        Ok(self.offset)
    }

    pub fn position(&self) -> FileOffset {
        self.offset
    }

    pub fn metadata(&self) -> KernelResult<FileMetadata> {
        self.inode.metadata()
    }
}

pub struct Vfs {
    mount_points: BTreeMap<String, MountPoint>,
    next_fd: u64,
    open_files: BTreeMap<u64, FileDescriptor>,
}

impl Vfs {
    pub fn new() -> Self {
        Vfs {
            mount_points: BTreeMap::new(),
            next_fd: 3,
            open_files: BTreeMap::new(),
        }
    }

    pub fn mount(&mut self, path: &str, fs: Box<dyn FileSystem>, flags: MountFlags) -> KernelResult<()> {
        if self.mount_points.contains_key(path) {
            return Err(KernelError::PageAlreadyMapped);
        }

        self.mount_points.insert(path.to_string(), MountPoint {
            path: path.to_string(),
            fs,
            flags,
        });

        Ok(())
    }

    pub fn unmount(&mut self, path: &str) -> KernelResult<()> {
        self.mount_points.remove(path)
            .ok_or(KernelError::FileSystemNotFound)?;
        Ok(())
    }

    pub fn open(&mut self, path: &str, flags: OpenFlags) -> KernelResult<u64> {
        let (mount_path, relative_path) = self.resolve_mount_point(path)?;

        let mount = self.mount_points.get(&mount_path)
            .ok_or(KernelError::FileSystemNotFound)?;

        let inode = self.traverse_path(mount.fs.root_inode(), &relative_path)?;

        let fd = self.next_fd;
        self.next_fd += 1;

        let file_desc = FileDescriptor::new(inode, flags);
        self.open_files.insert(fd, file_desc);

        Ok(fd)
    }

    pub fn close(&mut self, fd: u64) -> KernelResult<()> {
        self.open_files.remove(&fd)
            .ok_or(KernelError::InvalidArgument)?;
        Ok(())
    }

    pub fn read(&mut self, fd: u64, buffer: &mut [u8]) -> KernelResult<usize> {
        let file = self.open_files.get_mut(&fd)
            .ok_or(KernelError::InvalidArgument)?;
        file.read(buffer)
    }

    pub fn write(&mut self, fd: u64, data: &[u8]) -> KernelResult<usize> {
        let file = self.open_files.get_mut(&fd)
            .ok_or(KernelError::InvalidArgument)?;
        file.write(data)
    }

    pub fn readdir(&self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let (mount_path, relative_path) = self.resolve_mount_point(path)?;
        let mount = self.mount_points.get(&mount_path)
            .ok_or(KernelError::FileSystemNotFound)?;

        let inode = self.traverse_path(mount.fs.root_inode(), &relative_path)?;
        inode.readdir()
    }

    fn resolve_mount_point(&self, path: &str) -> KernelResult<(String, String)> {
        let mut best_mount = String::new();
        let mut best_len = 0;

        for mount_path in self.mount_points.keys() {
            if path.starts_with(mount_path) && mount_path.len() > best_len {
                best_mount = mount_path.clone();
                best_len = mount_path.len();
            }
        }

        if best_mount.is_empty() {
            if self.mount_points.contains_key("/") {
                return Ok(("/".to_string(), path.to_string()));
            }
            return Err(KernelError::FileSystemNotFound);
        }

        let relative = &path[best_len..];
        Ok((best_mount, relative.to_string()))
    }

    fn traverse_path(&self, root: Box<dyn Inode>, path: &str) -> KernelResult<Box<dyn Inode>> {
        if path.is_empty() || path == "/" {
            return Ok(root);
        }

        let components: Vec<&str> = path.split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current = root;
        for component in components {
            current = current.lookup(component)?;
        }

        Ok(current)
    }

    pub fn statfs(&self, path: &str) -> KernelResult<FileSystemStats> {
        let (mount_path, _) = self.resolve_mount_point(path)?;
        let mount = self.mount_points.get(&mount_path)
            .ok_or(KernelError::FileSystemNotFound)?;
        mount.fs.statfs()
    }

    pub fn list_mounts(&self) -> Vec<(String, FileSystemType)> {
        self.mount_points.values()
            .map(|m| (m.path.clone(), m.fs.fs_type()))
            .collect()
    }
}

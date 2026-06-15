use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use super::vfs::{
    FileSystem, FileSystemType, FileSystemStats, Inode, FileType, FileMetadata,
    FilePermissions, DirEntry, FileOffset,
};
use crate::{KernelResult, KernelError};

const NTFS_SIGNATURE: &[u8; 8] = b"NTFS    ";
const MFT_NUMBER_MFT: u64 = 0;
const MFT_NUMBER_MFTMIRR: u64 = 1;
const MFT_NUMBER_LOGFILE: u64 = 2;
const MFT_NUMBER_VOLUME: u64 = 3;
const MFT_NUMBER_ROOT: u64 = 5;
const MFT_RECORD_SIZE: usize = 1024;
const MFT_RECORD_SIGNATURE: u32 = 0x454C4946;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NtfsAttributeType {
    StandardInformation = 0x10,
    AttributeList = 0x20,
    FileName = 0x30,
    ObjectId = 0x40,
    SecurityDescriptor = 0x50,
    VolumeName = 0x60,
    VolumeInformation = 0x70,
    Data = 0x80,
    IndexRoot = 0x90,
    IndexAllocation = 0xA0,
    Bitmap = 0xB0,
    ReparsePoint = 0xC0,
    EaInformation = 0xD0,
    Ea = 0xE0,
    LoggedUtilityStream = 0x100,
    End = 0xFFFFFFFF,
}

impl From<u32> for NtfsAttributeType {
    fn from(v: u32) -> Self {
        match v {
            0x10 => NtfsAttributeType::StandardInformation,
            0x20 => NtfsAttributeType::AttributeList,
            0x30 => NtfsAttributeType::FileName,
            0x40 => NtfsAttributeType::ObjectId,
            0x50 => NtfsAttributeType::SecurityDescriptor,
            0x60 => NtfsAttributeType::VolumeName,
            0x70 => NtfsAttributeType::VolumeInformation,
            0x80 => NtfsAttributeType::Data,
            0x90 => NtfsAttributeType::IndexRoot,
            0xA0 => NtfsAttributeType::IndexAllocation,
            0xB0 => NtfsAttributeType::Bitmap,
            0xC0 => NtfsAttributeType::ReparsePoint,
            0xD0 => NtfsAttributeType::EaInformation,
            0xE0 => NtfsAttributeType::Ea,
            0x100 => NtfsAttributeType::LoggedUtilityStream,
            0xFFFFFFFF => NtfsAttributeType::End,
            _ => NtfsAttributeType::End,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MftRecordFlags;

impl MftRecordFlags {
    pub const IN_USE: u16 = 0x0001;
    pub const IS_DIRECTORY: u16 = 0x0002;
    pub const IS_EXTENSION: u16 = 0x0004;
    pub const HAS_INDEX: u16 = 0x0008;
}

#[derive(Debug, Clone, Copy)]
pub struct AttributeFlags;

impl AttributeFlags {
    pub const COMPRESSED: u16 = 0x0001;
    pub const ENCRYPTED: u16 = 0x4000;
    pub const SPARSE: u16 = 0x8000;
}

#[derive(Debug, Clone)]
pub struct NtfsBpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub total_sectors: u64,
    pub mft_start_cluster: u64,
    pub mftmirr_start_cluster: u64,
    pub mft_record_size: i8,
    pub index_record_size: i8,
    pub volume_serial: u64,
    pub cluster_size: u32,
}

impl NtfsBpb {
    pub fn parse(boot_sector: &[u8]) -> KernelResult<Self> {
        if boot_sector.len() < 512 {
            return Err(KernelError::IoError);
        }

        if &boot_sector[3..11] != NTFS_SIGNATURE {
            return Err(KernelError::FileSystemNotFound);
        }

        let bytes_per_sector = u16::from_le_bytes([boot_sector[11], boot_sector[12]]);
        let sectors_per_cluster = boot_sector[13];

        let total_sectors = u64::from_le_bytes([
            boot_sector[40], boot_sector[41], boot_sector[42], boot_sector[43],
            boot_sector[44], boot_sector[45], boot_sector[46], boot_sector[47],
        ]);

        let mft_start_cluster = u64::from_le_bytes([
            boot_sector[48], boot_sector[49], boot_sector[50], boot_sector[51],
            boot_sector[52], boot_sector[53], boot_sector[54], boot_sector[55],
        ]);

        let mftmirr_start_cluster = u64::from_le_bytes([
            boot_sector[56], boot_sector[57], boot_sector[58], boot_sector[59],
            boot_sector[60], boot_sector[61], boot_sector[62], boot_sector[63],
        ]);

        let mft_record_size_raw = boot_sector[64] as i8;
        let index_record_size_raw = boot_sector[68] as i8;

        let volume_serial = u64::from_le_bytes([
            boot_sector[72], boot_sector[73], boot_sector[74], boot_sector[75],
            boot_sector[76], boot_sector[77], boot_sector[78], boot_sector[79],
        ]);

        let mft_record_size = if mft_record_size_raw > 0 {
            mft_record_size_raw as i32 * bytes_per_sector as i32
        } else {
            1i32 << (-mft_record_size_raw as u32) as i32
        };

        let cluster_size = bytes_per_sector as u32 * sectors_per_cluster as u32;

        Ok(NtfsBpb {
            bytes_per_sector,
            sectors_per_cluster,
            total_sectors,
            mft_start_cluster,
            mftmirr_start_cluster,
            mft_record_size: mft_record_size_raw,
            index_record_size: index_record_size_raw,
            volume_serial,
            cluster_size,
        })
    }

    pub fn mft_record_size_bytes(&self) -> usize {
        if self.mft_record_size > 0 {
            self.mft_record_size as usize * self.bytes_per_sector as usize
        } else {
            1usize << (-self.mft_record_size as u32)
        }
    }
}

#[derive(Debug, Clone)]
pub struct MftRecord {
    pub record_number: u32,
    pub flags: u16,
    pub sequence_number: u16,
    pub attributes: Vec<NtfsAttribute>,
    pub bytes_in_use: u32,
}

#[derive(Debug, Clone)]
pub struct NtfsAttribute {
    pub attr_type: NtfsAttributeType,
    pub total_size: u32,
    pub non_resident: bool,
    pub name: Option<String>,
    pub resident_data: Option<Vec<u8>>,
    pub run_list: Vec<DataRun>,
    pub data_size: u64,
    pub initialized_size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct DataRun {
    pub length: u64,
    pub offset: i64,
}

#[derive(Debug)]
pub struct NtfsJournal {
    entries: Vec<JournalEntry>,
    max_entries: usize,
    current_lsn: u64,
}

#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub lsn: u64,
    pub operation: JournalOperation,
    pub file_name: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum JournalOperation {
    FileCreate,
    FileDelete,
    FileWrite,
    FileRename,
    AttributeChange,
    DirectoryCreate,
    Checkpoint,
}

impl NtfsJournal {
    pub fn new() -> Self {
        NtfsJournal {
            entries: Vec::new(),
            max_entries: 1024,
            current_lsn: 0,
        }
    }

    pub fn log(&mut self, operation: JournalOperation, file_name: &str, data: &[u8]) {
        let entry = JournalEntry {
            lsn: self.current_lsn,
            operation,
            file_name: String::from(file_name),
            data: data.to_vec(),
            timestamp: 0,
        };
        self.current_lsn += 1;

        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    pub fn get_entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    pub fn checkpoint(&mut self) {
        self.log(JournalOperation::Checkpoint, "", &[]);
    }
}

pub struct NtfsInode {
    record_number: u64,
    metadata: FileMetadata,
    attributes: Vec<NtfsAttribute>,
    volume: *const NtfsVolume,
}

unsafe impl Send for NtfsInode {}
unsafe impl Sync for NtfsInode {}

impl NtfsInode {
    fn from_mft_record(record: &MftRecord, volume: *const NtfsVolume) -> Self {
        let is_dir = (record.flags & MftRecordFlags::IS_DIRECTORY) != 0;

        let mut file_name = String::from("unknown");
        let mut size: u64 = 0;
        let mut created: u64 = 0;
        let mut modified: u64 = 0;

        for attr in &record.attributes {
            match attr.attr_type {
                NtfsAttributeType::FileName => {
                    if let Some(ref data) = attr.resident_data {
                        file_name = Self::parse_file_name(data);
                    }
                }
                NtfsAttributeType::StandardInformation => {
                    if let Some(ref data) = attr.resident_data {
                        created = Self::parse_timestamp(data, 0);
                        modified = Self::parse_timestamp(data, 8);
                    }
                }
                NtfsAttributeType::Data => {
                    size = attr.data_size;
                }
                _ => {}
            }
        }

        let metadata = FileMetadata {
            inode: record.record_number as u64,
            file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            size,
            permissions: FilePermissions::default(),
            created,
            modified,
            accessed: modified,
            nlinks: 1,
            uid: 0,
            gid: 0,
        };

        NtfsInode {
            record_number: record.record_number as u64,
            metadata,
            attributes: record.attributes.clone(),
            volume,
        }
    }

    fn parse_file_name(data: &[u8]) -> String {
        if data.len() < 66 {
            return String::from("unknown");
        }
        let name_len = data[64] as usize;
        if data.len() < 66 + name_len * 2 {
            return String::from("unknown");
        }

        let mut name = String::new();
        for i in 0..name_len {
            let offset = 66 + i * 2;
            let ch = u16::from_le_bytes([data[offset], data[offset + 1]]);
            if let Some(c) = char::from_u32(ch as u32) {
                name.push(c);
            }
        }
        name
    }

    fn parse_timestamp(data: &[u8], offset: usize) -> u64 {
        if data.len() < offset + 8 {
            return 0;
        }
        let raw = u64::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);
        let seconds = raw / 10_000_000;
        if seconds > 11644473600 {
            seconds - 11644473600
        } else {
            0
        }
    }

    fn get_data_attribute(&self) -> Option<&NtfsAttribute> {
        self.attributes.iter().find(|a| {
            a.attr_type == NtfsAttributeType::Data && a.name.is_none()
        })
    }

    fn get_index_root_attribute(&self) -> Option<&NtfsAttribute> {
        self.attributes.iter().find(|a| {
            a.attr_type == NtfsAttributeType::IndexRoot
        })
    }

    fn read_from_runs(&self, runs: &[DataRun], offset: FileOffset, buffer: &mut [u8]) -> KernelResult<usize> {
        let volume = unsafe { &*self.volume };
        let cluster_size = volume.bpb.cluster_size as u64;

        let mut current_cluster: i64 = 0;
        let mut bytes_read = 0;
        let mut file_offset = offset as u64;

        for run in runs {
            current_cluster += run.offset;
            let run_start_byte = current_cluster as u64 * cluster_size;
            let run_end_byte = run_start_byte + run.length as u64 * cluster_size;

            if file_offset >= run_end_byte {
                continue;
            }

            let read_start = if file_offset > run_start_byte {
                file_offset - run_start_byte
            } else {
                0
            };

            let read_len = ((run.length as u64 * cluster_size) - read_start)
                .min(buffer.len() as u64 - bytes_read as u64);

            let dst_start = bytes_read as usize;
            let dst_end = dst_start + read_len as usize;
            if dst_end <= buffer.len() {
                for i in dst_start..dst_end {
                    buffer[i] = 0;
                }
                bytes_read += read_len as usize;
                file_offset += read_len;
            }

            if bytes_read >= buffer.len() {
                break;
            }
        }

        Ok(bytes_read)
    }
}

impl Inode for NtfsInode {
    fn metadata(&self) -> KernelResult<FileMetadata> {
        Ok(self.metadata.clone())
    }

    fn read(&self, offset: FileOffset, buffer: &mut [u8]) -> KernelResult<usize> {
        if let Some(attr) = self.get_data_attribute() {
            if let Some(ref data) = attr.resident_data {
                let start = offset as usize;
                if start >= data.len() {
                    return Ok(0);
                }
                let end = (start + buffer.len()).min(data.len());
                let len = end - start;
                buffer[..len].copy_from_slice(&data[start..end]);
                Ok(len)
            } else {
                self.read_from_runs(&attr.run_list, offset, buffer)
            }
        } else {
            Err(KernelError::FileNotFound)
        }
    }

    fn write(&mut self, offset: FileOffset, data: &[u8]) -> KernelResult<usize> {
        Err(KernelError::IoError)
    }

    fn readdir(&self) -> KernelResult<Vec<DirEntry>> {
        Ok(Vec::new())
    }

    fn lookup(&self, name: &str) -> KernelResult<Box<dyn Inode>> {
        Err(KernelError::FileNotFound)
    }

    fn create(&mut self, name: &str, file_type: FileType) -> KernelResult<Box<dyn Inode>> {
        Err(KernelError::IoError)
    }

    fn unlink(&mut self, name: &str) -> KernelResult<()> {
        Err(KernelError::IoError)
    }

    fn truncate(&mut self, size: u64) -> KernelResult<()> {
        Err(KernelError::IoError)
    }

    fn clone_inode(&self) -> Box<dyn Inode> {
        Box::new(NtfsInode {
            record_number: self.record_number,
            metadata: self.metadata.clone(),
            attributes: self.attributes.clone(),
            volume: self.volume,
        })
    }
}

pub struct NtfsVolume {
    bpb: NtfsBpb,
    mft_cache: BTreeMap<u64, MftRecord>,
    journal: NtfsJournal,
    disk_id: u32,
}

impl NtfsVolume {
    pub fn new(boot_sector: &[u8], disk_id: u32) -> KernelResult<Self> {
        let bpb = NtfsBpb::parse(boot_sector)?;

        Ok(NtfsVolume {
            bpb,
            mft_cache: BTreeMap::new(),
            journal: NtfsJournal::new(),
            disk_id,
        })
    }

    pub fn parse_mft_record(&self, data: &[u8]) -> KernelResult<MftRecord> {
        if data.len() < MFT_RECORD_SIZE {
            return Err(KernelError::IoError);
        }

        let signature = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if signature != MFT_RECORD_SIGNATURE {
            return Err(KernelError::IoError);
        }

        let sequence_number = u16::from_le_bytes([data[0x10], data[0x11]]);
        let attrs_offset = u16::from_le_bytes([data[0x14], data[0x15]]) as usize;
        let flags = u16::from_le_bytes([data[0x16], data[0x17]]);
        let bytes_in_use = u32::from_le_bytes([data[0x18], data[0x19], data[0x1A], data[0x1B]]);

        let mut attributes = Vec::new();
        let mut offset = attrs_offset;

        while offset + 4 < data.len() {
            let attr_type = u32::from_le_bytes([
                data[offset], data[offset + 1], data[offset + 2], data[offset + 3]
            ]);

            if attr_type == 0xFFFFFFFF {
                break;
            }

            if offset + 24 > data.len() {
                break;
            }

            let total_size = u32::from_le_bytes([
                data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]
            ]) as usize;

            if total_size == 0 || offset + total_size > data.len() {
                break;
            }

            let non_resident = data[offset + 8] != 0;
            let name_length = data[offset + 9] as usize;
            let name_offset = u16::from_le_bytes([data[offset + 10], data[offset + 11]]) as usize;

            let name = if name_length > 0 && name_offset + name_length * 2 <= total_size {
                let mut attr_name = String::new();
                for i in 0..name_length {
                    let ch_off = offset + name_offset + i * 2;
                    if ch_off + 1 < data.len() {
                        let ch = u16::from_le_bytes([data[ch_off], data[ch_off + 1]]);
                        if let Some(c) = char::from_u32(ch as u32) {
                            attr_name.push(c);
                        }
                    }
                }
                Some(attr_name)
            } else {
                None
            };

            let mut attr = NtfsAttribute {
                attr_type: NtfsAttributeType::from(attr_type),
                total_size: total_size as u32,
                non_resident,
                name,
                resident_data: None,
                run_list: Vec::new(),
                data_size: 0,
                initialized_size: 0,
            };

            if non_resident {
                if offset + 0x40 <= data.len() {
                    attr.data_size = u64::from_le_bytes([
                        data[offset + 0x30], data[offset + 0x31], data[offset + 0x32], data[offset + 0x33],
                        data[offset + 0x34], data[offset + 0x35], data[offset + 0x36], data[offset + 0x37],
                    ]);
                    attr.initialized_size = u64::from_le_bytes([
                        data[offset + 0x38], data[offset + 0x39], data[offset + 0x3A], data[offset + 0x3B],
                        data[offset + 0x3C], data[offset + 0x3D], data[offset + 0x3E], data[offset + 0x3F],
                    ]);

                    let run_offset = u16::from_le_bytes([data[offset + 0x20], data[offset + 0x21]]) as usize;
                    attr.run_list = self.parse_run_list(&data[offset + run_offset..offset + total_size]);
                }
            } else {
                if offset + 0x16 <= data.len() {
                    let content_size = u32::from_le_bytes([
                        data[offset + 0x10], data[offset + 0x11], data[offset + 0x12], data[offset + 0x13]
                    ]) as usize;
                    let content_offset = u16::from_le_bytes([data[offset + 0x14], data[offset + 0x15]]) as usize;

                    attr.data_size = content_size as u64;
                    attr.initialized_size = content_size as u64;

                    if offset + content_offset + content_size <= data.len() {
                        let mut content = vec![0u8; content_size];
                        content.copy_from_slice(&data[offset + content_offset..offset + content_offset + content_size]);
                        attr.resident_data = Some(content);
                    }
                }
            }

            attributes.push(attr);
            offset += total_size;
        }

        Ok(MftRecord {
            record_number: 0,
            flags,
            sequence_number,
            attributes,
            bytes_in_use,
        })
    }

    pub fn parse_run_list(&self, data: &[u8]) -> Vec<DataRun> {
        let mut runs = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let header = data[offset];
            if header == 0 {
                break;
            }
            offset += 1;

            let size_len = (header & 0x0F) as usize;
            let offset_len = ((header >> 4) & 0x0F) as usize;

            if size_len == 0 || offset + size_len > data.len() {
                break;
            }

            let mut length: u64 = 0;
            for i in 0..size_len {
                length |= (data[offset + i] as u64) << (i * 8);
            }
            offset += size_len;

            if offset_len == 0 {
                runs.push(DataRun { length, offset: 0 });
                continue;
            }

            if offset + offset_len > data.len() {
                break;
            }

            let mut run_offset: i64 = 0;
            for i in 0..offset_len {
                run_offset |= (data[offset + i] as i64) << (i * 8);
            }
            if data[offset + offset_len - 1] & 0x80 != 0 {
                run_offset |= !0i64 << (offset_len * 8);
            }
            offset += offset_len;

            runs.push(DataRun { length, offset: run_offset });
        }

        runs
    }

    pub fn get_mft_record(&mut self, record_number: u64) -> KernelResult<MftRecord> {
        if let Some(record) = self.mft_cache.get(&record_number) {
            return Ok(record.clone());
        }

        let record_size = self.bpb.mft_record_size_bytes() as u64;
        let mft_byte_offset = self.bpb.mft_start_cluster * self.bpb.cluster_size as u64;
        let record_offset = mft_byte_offset + record_number * record_size;

        let mut buffer = vec![0u8; record_size as usize];

        if buffer.iter().all(|&b| b == 0) {
            if record_number == MFT_NUMBER_ROOT {
                return Ok(self.create_root_directory_record());
            }
            return Err(KernelError::FileNotFound);
        }

        let mut record = self.parse_mft_record(&buffer)?;
        record.record_number = record_number as u32;

        self.mft_cache.insert(record_number, record.clone());

        Ok(record)
    }

    fn create_root_directory_record(&self) -> MftRecord {
        MftRecord {
            record_number: MFT_NUMBER_ROOT as u32,
            flags: MftRecordFlags::IN_USE | MftRecordFlags::IS_DIRECTORY,
            sequence_number: 1,
            attributes: Vec::new(),
            bytes_in_use: MFT_RECORD_SIZE as u32,
        }
    }
}

impl FileSystem for NtfsVolume {
    fn fs_type(&self) -> FileSystemType {
        FileSystemType::NTFS
    }

    fn label(&self) -> &str {
        "NTFS"
    }

    fn root_inode(&self) -> Box<dyn Inode> {
        let record = MftRecord {
            record_number: MFT_NUMBER_ROOT as u32,
            flags: MftRecordFlags::IN_USE | MftRecordFlags::IS_DIRECTORY,
            sequence_number: 1,
            attributes: Vec::new(),
            bytes_in_use: MFT_RECORD_SIZE as u32,
        };
        Box::new(NtfsInode::from_mft_record(&record, self as *const NtfsVolume))
    }

    fn get_inode(&self, ino: u64) -> KernelResult<Box<dyn Inode>> {
        Err(KernelError::FileNotFound)
    }

    fn sync(&self) -> KernelResult<()> {
        Ok(())
    }

    fn statfs(&self) -> KernelResult<FileSystemStats> {
        Ok(FileSystemStats {
            total_size: self.bpb.total_sectors * self.bpb.bytes_per_sector as u64,
            free_space: 0,
            total_inodes: 0,
            free_inodes: 0,
            block_size: self.bpb.cluster_size as u64,
            max_name_length: 255,
        })
    }
}

pub fn mount_ntfs(vfs: &mut crate::fs::vfs::Vfs, disk_id: u32) -> KernelResult<()> {
    let mut boot_sector = vec![0u8; 512];

    boot_sector[3..11].copy_from_slice(NTFS_SIGNATURE);
    boot_sector[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot_sector[13] = 8;

    let volume = NtfsVolume::new(&boot_sector, disk_id)?;

    vfs.mount("/", Box::new(volume), crate::fs::vfs::MountFlags::default())?;

    Ok(())
}

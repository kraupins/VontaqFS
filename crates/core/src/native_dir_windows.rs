//! Windows counterpart of native_dir's Unix descriptor-relative filesystem.
//! All descendants are opened from a held HANDLE using NtCreateFile's
//! OBJECT_ATTRIBUTES.RootDirectory, FILE_OPEN_REPARSE_POINT and a strict
//! reparse/regular-file check. Path-based APIs are never used after root open.
//! Windows local NTFS/ReFS only: unsupported filesystem semantics fail closed.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, OsStr},
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Prefix},
    ptr,
};
use uuid::Uuid;

type Handle = *mut c_void;
type NtStatus = i32;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_SHARE_ALL: u32 = 0x01 | 0x02 | 0x04;
const FILE_READ_DATA: u32 = 0x0001;
const FILE_WRITE_DATA: u32 = 0x0002;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_TRAVERSE: u32 = 0x0020;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
const FILE_APPEND_DATA: u32 = 0x0004;
const DELETE: u32 = 0x00010000;
const SYNCHRONIZE: u32 = 0x00100000;
const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const FILE_DIRECTORY_FILE: u32 = 0x00000001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x00000040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x00000020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x00200000;
const FILE_WRITE_THROUGH: u32 = 0x00000002;
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
const OPEN_EXISTING: u32 = 3;
const FILE_RENAME_INFORMATION: u32 = 10;
const FILE_LINK_INFORMATION: u32 = 11;
const FILE_DISPOSITION_INFORMATION: u32 = 13;
const FILE_ID_INFO_CLASS: i32 = 18;
const STATUS_OBJECT_NAME_COLLISION: NtStatus = 0xC0000035u32 as i32;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: Handle,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}
#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}
#[repr(C)]
struct FileIdInfo {
    volume_serial_number: u64,
    file_id: [u8; 16],
}
#[repr(C)]
struct FileRenameInfo {
    replace_if_exists: u8,
    root_directory: Handle,
    file_name_length: u32,
    file_name: [u16; 1],
}
#[repr(C)]
struct FileDispositionInfo {
    delete_file: u8,
}
#[repr(C)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time: [u32; 2],
    last_access_time: [u32; 2],
    last_write_time: [u32; 2],
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut Handle,
        desired_access: u32,
        object_attributes: *mut ObjectAttributes,
        io_status: *mut IoStatusBlock,
        allocation_size: *mut i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *mut c_void,
        ea_length: u32,
    ) -> NtStatus;
    fn NtSetInformationFile(
        handle: Handle,
        io_status: *mut IoStatusBlock,
        info: *mut c_void,
        length: u32,
        file_information_class: u32,
    ) -> NtStatus;
    fn RtlNtStatusToDosError(status: NtStatus) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn GetFileInformationByHandleEx(
        handle: Handle,
        class: i32,
        info: *mut c_void,
        size: u32,
    ) -> i32;
    fn GetFileInformationByHandle(handle: Handle, info: *mut ByHandleFileInformation) -> i32;
    fn FlushFileBuffers(handle: Handle) -> i32;
}
fn invalid() -> StorageError {
    StorageError::PathInvalid("unsafe Windows native destination component".into())
}
fn io_status(status: NtStatus) -> StorageError {
    let err = unsafe { RtlNtStatusToDosError(status) };
    StorageError::Io(io::Error::from_raw_os_error(err as i32))
}
fn check_status(status: NtStatus) -> StorageResult<()> {
    if status < 0 {
        Err(io_status(status))
    } else {
        Ok(())
    }
}
fn owned(handle: Handle) -> StorageResult<File> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: the Windows native creation functions transfer a new HANDLE.
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}
fn raw(file: &File) -> Handle {
    file.as_raw_handle().cast()
}
fn flush(file: &File) -> StorageResult<()> {
    // Some directory filesystems do not implement FlushFileBuffers on a
    // directory HANDLE. Unsupported durability must not be reported as success.
    if unsafe { FlushFileBuffers(raw(file)) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn validate(name: &OsStr) -> StorageResult<Vec<u16>> {
    let units: Vec<u16> = name.encode_wide().collect();
    if units.is_empty()
        || units.len() > 255
        || units.iter().any(|c| {
            *c == 0 || *c < 32 || *c == b':' as u16 || *c == b'/' as u16 || *c == b'\\' as u16
        })
    {
        return Err(invalid());
    }
    let value = String::from_utf16(&units).map_err(|_| invalid())?;
    if value == "." || value == ".." || value.ends_with('.') || value.ends_with(' ') {
        return Err(invalid());
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return Err(invalid());
    }
    Ok(units)
}
fn components(path: &Path) -> StorageResult<Vec<Vec<u16>>> {
    let mut out = Vec::new();
    for part in path.components() {
        let Component::Normal(name) = part else {
            return Err(invalid());
        };
        out.push(validate(name)?);
    }
    if out.is_empty() {
        return Err(invalid());
    }
    Ok(out)
}
fn nt_open(
    parent: &File,
    name: &[u16],
    directory: bool,
    create: bool,
    write: bool,
) -> StorageResult<File> {
    let access = FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | DELETE
        | (if directory { FILE_TRAVERSE } else { 0 })
        | (if write {
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_ATTRIBUTES
        } else {
            FILE_READ_DATA
        });
    nt_open_with_access(parent, name, directory, create, access)
}

// Issuing a grant only needs to verify a selected directory's path and file ID.
// Do not demand DELETE or LIST_DIRECTORY on every ancestor: a valid selected
// user folder can sit beneath a protected parent that denies those privileges.
// This still opens each component without following reparse points, and still
// compares the held root's identity against the selected path.
fn open_traversal_directory(parent: &File, name: &[u16]) -> StorageResult<File> {
    nt_open_with_access(
        parent,
        name,
        true,
        false,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_TRAVERSE,
    )
}

fn nt_open_with_access(
    parent: &File,
    name: &[u16],
    directory: bool,
    create: bool,
    access: u32,
) -> StorageResult<File> {
    let mut wide = name.to_vec();
    let mut unicode = UnicodeString {
        length: (wide.len() * 2) as u16,
        maximum_length: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let mut attrs = ObjectAttributes {
        length: mem::size_of::<ObjectAttributes>() as u32,
        root_directory: raw(parent),
        object_name: &mut unicode,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE | FILE_WRITE_THROUGH
        };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attrs,
            &mut iosb,
            ptr::null_mut(),
            0,
            FILE_SHARE_ALL,
            if create { FILE_CREATE } else { FILE_OPEN },
            options,
            ptr::null_mut(),
            0,
        )
    };
    check_status(status)?;
    let file = owned(handle)?;
    let info = file_info(&file)?;
    if info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
    {
        return Err(invalid());
    }
    Ok(file)
}
fn file_info(file: &File) -> StorageResult<ByHandleFileInformation> {
    let mut info = mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    if unsafe { GetFileInformationByHandle(raw(file), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { info.assume_init() })
}
fn id_info(file: &File) -> StorageResult<FileIdInfo> {
    let mut info = mem::MaybeUninit::<FileIdInfo>::uninit();
    if unsafe {
        GetFileInformationByHandleEx(
            raw(file),
            FILE_ID_INFO_CLASS,
            info.as_mut_ptr().cast(),
            mem::size_of::<FileIdInfo>() as u32,
        )
    } == 0
    {
        return Err(StorageError::StorageUnavailable(format!(
            "Windows file identity unavailable: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { info.assume_init() })
}
fn same_id(a: &File, b: &File) -> StorageResult<bool> {
    let x = id_info(a)?;
    let y = id_info(b)?;
    Ok(x.volume_serial_number == y.volume_serial_number && x.file_id == y.file_id)
}
fn rename_handle(
    source: &File,
    parent: &File,
    name: &[u16],
    replace: bool,
    link: bool,
) -> StorageResult<()> {
    let head = mem::offset_of!(FileRenameInfo, file_name);
    let bytes = head + name.len() * 2;
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    let info = buffer.as_mut_ptr().cast::<FileRenameInfo>();
    unsafe {
        (*info).replace_if_exists = u8::from(replace);
        (*info).root_directory = raw(parent);
        (*info).file_name_length = (name.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            (info.cast::<u8>()).add(head).cast::<u16>(),
            name.len(),
        );
    }
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let result = unsafe {
        NtSetInformationFile(
            raw(source),
            &mut iosb,
            buffer.as_mut_ptr().cast(),
            bytes as u32,
            if link {
                FILE_LINK_INFORMATION
            } else {
                FILE_RENAME_INFORMATION
            },
        )
    };
    if result == STATUS_OBJECT_NAME_COLLISION {
        return Err(StorageError::Conflict(
            "native target appeared before publication".into(),
        ));
    }
    check_status(result)
}
fn delete_handle(file: &File) -> StorageResult<()> {
    let mut info = FileDispositionInfo { delete_file: 1 };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    check_status(unsafe {
        NtSetInformationFile(
            raw(file),
            &mut iosb,
            (&mut info as *mut FileDispositionInfo).cast(),
            mem::size_of::<FileDispositionInfo>() as u32,
            FILE_DISPOSITION_INFORMATION,
        )
    })
}
fn dir_sync(dir: &File) -> StorageResult<()> {
    // NTFS journals atomic namespace changes. Directory FlushFileBuffers is
    // not implemented on many Windows builds. Read/traverse directory handles
    // can also return ERROR_ACCESS_DENIED (5) even though the file was synced
    // before publication. This is not an OS power-loss durability certification.
    match flush(dir) {
        Ok(()) => Ok(()),
        Err(StorageError::Io(e)) if matches!(e.raw_os_error(), Some(1 | 5 | 50 | 87)) => Ok(()),
        Err(e) => Err(e),
    }
}
fn stable_prune(name: &[u16]) -> Vec<u16> {
    let mut data = Vec::with_capacity(name.len() * 2);
    for ch in name {
        data.extend_from_slice(&ch.to_le_bytes());
    }
    format!(".vontaqfs-prune-{:x}", Sha256::digest(&data))
        .encode_utf16()
        .collect()
}
fn leaf(name: &str) -> Vec<u16> {
    name.encode_utf16().collect()
}
fn open_existing(parent: &File, name: &[u16]) -> StorageResult<Option<File>> {
    match nt_open(parent, name, false, false, true) {
        Ok(f) => Ok(Some(f)),
        Err(StorageError::Io(err)) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn open_directory(parent: &File, name: &[u16], create: bool) -> StorageResult<File> {
    if !create {
        return nt_open(parent, name, true, false, false);
    }
    match nt_open(parent, name, true, true, true) {
        Ok(f) => Ok(f),
        Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {
            nt_open(parent, name, true, false, true)
        }
        Err(e) => Err(e),
    }
}

pub(crate) struct NativeDirectory {
    root: File,
    // Path is used only to verify that the selected directory still names
    // the held root HANDLE. File operations remain handle-relative.
    root_path: std::path::PathBuf,
}
impl NativeDirectory {
    pub(crate) fn open(path: &Path) -> StorageResult<Self> {
        if !path.is_absolute() {
            return Err(invalid());
        }
        let mut parts = path.components();
        let drive = match parts.next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
                _ => {
                    return Err(StorageError::StorageUnavailable(
                        "UNC/remote native destinations are unsupported".into(),
                    ))
                }
            },
            _ => return Err(invalid()),
        };
        if !matches!(parts.next(), Some(Component::RootDir)) {
            return Err(invalid());
        }
        let root_path: Vec<u16> = format!("\\\\?\\{}:\\", drive as char)
            .encode_utf16()
            .chain([0])
            .collect();
        let handle = unsafe {
            CreateFileW(
                root_path.as_ptr(),
                FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_TRAVERSE | FILE_READ_DATA,
                FILE_SHARE_ALL,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        let mut dir = owned(handle)?;
        if file_info(&dir)?.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(invalid());
        }
        for part in parts {
            let Component::Normal(name) = part else {
                return Err(invalid());
            };
            dir = open_traversal_directory(&dir, &validate(name)?)?;
        }
        // Reject filesystem types without durable handles and file IDs.
        id_info(&dir)?;
        Ok(Self {
            root: dir,
            root_path: path.to_path_buf(),
        })
    }
    pub(crate) fn identity(&self) -> StorageResult<String> {
        let info = id_info(&self.root)?;
        let id: String = info.file_id.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("windows:{}:{id}", info.volume_serial_number))
    }
    pub(crate) fn ensure_root_identity(&self, observed: &std::fs::Metadata) -> StorageResult<()> {
        // The preflight MetadataExt::volume_serial_number/file_index accessors
        // are unstable on stable Rust. Compare the stable preflight attributes,
        // then independently re-open the selected path without following a
        // reparse point and compare the *exact* held Windows file IDs.
        let held = self.root.metadata()?;
        if observed.creation_time() != held.creation_time()
            || observed.file_attributes() != held.file_attributes()
        {
            return Err(StorageError::PathInvalid(
                "native destination root changed since preflight".into(),
            ));
        }
        let current_path = Self::open(&self.root_path)?;
        if !same_id(&current_path.root, &self.root)? {
            return Err(StorageError::PathInvalid(
                "native destination root was replaced".into(),
            ));
        }
        Ok(())
    }
    fn parent(&self, relative: &Path, create: bool) -> StorageResult<(File, Vec<u16>)> {
        let names = components(relative)?;
        let mut dir = self.root.try_clone()?;
        for name in &names[..names.len() - 1] {
            dir = open_directory(&dir, name, create)?;
        }
        Ok((dir, names.last().ok_or_else(invalid)?.clone()))
    }
    pub(crate) fn exists(&self, path: &Path) -> StorageResult<bool> {
        let (parent, name) = match self.parent(path, false) {
            Ok(v) => v,
            Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        Ok(open_existing(&parent, &name)?.is_some())
    }
    pub(crate) fn read(&self, path: &Path) -> StorageResult<File> {
        let (p, name) = self.parent(path, false)?;
        nt_open(&p, &name, false, false, false)
    }
    pub(crate) fn read_if_exists(&self, path: &Path) -> StorageResult<Option<File>> {
        let (parent, name) = match self.parent(path, false) {
            Ok(v) => v,
            Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        open_existing(&parent, &name)
    }
    pub(crate) fn temporary(
        &self,
        path: &Path,
        create_parents: bool,
    ) -> StorageResult<NativeTemporary> {
        let (dir, target) = self.parent(path, create_parents)?;
        let _ = open_existing(&dir, &target)?;
        for _ in 0..16 {
            let name = leaf(&format!(".vontaqfs-{}.tmp", Uuid::new_v4()));
            match nt_open(&dir, &name, false, true, true) {
                Ok(file) => {
                    return Ok(NativeTemporary {
                        dir,
                        target,
                        file,
                        published: false,
                    })
                }
                Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(StorageError::StorageUnavailable(
            "cannot create unique Windows temporary file".into(),
        ))
    }
    pub(crate) fn write_atomic(&self, path: &Path, contents: &[u8]) -> StorageResult<()> {
        let mut temp = self.temporary(path, true)?;
        temp.file.write_all(contents)?;
        temp.publish()
    }
    pub(crate) fn unlink(&self, path: &Path) -> StorageResult<()> {
        let (dir, name) = self.parent(path, false)?;
        if let Some(file) = open_existing(&dir, &name)? {
            delete_handle(&file)?;
            drop(file);
            dir_sync(&dir)?;
        }
        Ok(())
    }
    pub(crate) fn rename(&self, from: &Path, to: &Path) -> StorageResult<()> {
        let (dir, name) = self.parent(from, false)?;
        let (dest, target) = self.parent(to, false)?;
        let file = open_existing(&dir, &name)?
            .ok_or_else(|| StorageError::NotFound("native source".into()))?;
        // Journals may be replaced only with regular files. Reject reparse and directories.
        let _ = open_existing(&dest, &target)?;
        rename_handle(&file, &dest, &target, true, false)?;
        dir_sync(&dest)?;
        dir_sync(&dir)
    }
    pub(crate) fn recover_prune_for(&self, path: &Path) -> StorageResult<bool> {
        let (parent, target) = match self.parent(path, false) {
            Ok(v) => v,
            Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let folder = stable_prune(&target);
        let quarantine = match open_directory(&parent, &folder, false) {
            Ok(f) => f,
            Err(StorageError::Io(e)) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let marker_name = leaf("target");
        let item_name = leaf("item");
        let mut marker = nt_open(&quarantine, &marker_name, false, false, false)?;
        let mut expected = Vec::new();
        (&mut marker).take(512).read_to_end(&mut expected)?;
        let wanted: Vec<u8> = target.iter().flat_map(|c| c.to_le_bytes()).collect();
        if expected != wanted {
            return Err(StorageError::StorageUnavailable(
                "invalid prune quarantine marker".into(),
            ));
        }
        let mut restored = false;
        if let Some(item) = open_existing(&quarantine, &item_name)? {
            if let Some(user) = open_existing(&parent, &target)? {
                if !same_id(&user, &item)? {
                    return Err(StorageError::StorageUnavailable(
                        "user destination conflicts with prune recovery".into(),
                    ));
                }
            } else {
                rename_handle(&item, &parent, &target, false, true)?;
            }
            dir_sync(&parent)?;
            delete_handle(&item)?;
            drop(item);
            restored = true;
        }
        delete_handle(&marker)?;
        drop(marker);
        // Quarantine must be empty; remove by held HANDLE only.
        delete_handle(&quarantine)?;
        drop(quarantine);
        dir_sync(&parent)?;
        Ok(restored)
    }
    pub(crate) fn prune_tracked(&self, path: &Path, expected_sha256: &str) -> StorageResult<bool> {
        if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let Some(mut candidate) = self.read_if_exists(path)? else {
            return Ok(false);
        };
        let mut before = Sha256::new();
        io::copy(&mut candidate, &mut HashWriter(&mut before))?;
        if format!("{:x}", before.finalize()) != expected_sha256.to_ascii_lowercase() {
            return Ok(false);
        }
        let initial_id = id_info(&candidate)?;
        let (parent, name) = self.parent(path, false)?;
        let folder = stable_prune(&name);
        let quarantine = open_directory(&parent, &folder, true)?;
        let marker_name = leaf("target");
        let mut marker = nt_open(&quarantine, &marker_name, false, true, true)?;
        for c in &name {
            marker.write_all(&c.to_le_bytes())?;
        }
        marker.sync_all()?;
        dir_sync(&quarantine)?;
        dir_sync(&parent)?;
        // Move exactly the opened candidate HANDLE, not a subsequently swapped name.
        // A held candidate handle is renamed; if another process substituted
        // the old name, that file remains untouched and is never pruned.
        rename_handle(&candidate, &quarantine, &leaf("item"), false, false)?;
        dir_sync(&parent)?;
        dir_sync(&quarantine)?;
        let mut moved = nt_open(&quarantine, &leaf("item"), false, false, false)?;
        let moved_id = id_info(&moved)?;
        let mut verified = moved_id.volume_serial_number == initial_id.volume_serial_number
            && moved_id.file_id == initial_id.file_id;
        if verified {
            let mut hash = Sha256::new();
            io::copy(&mut moved, &mut HashWriter(&mut hash))?;
            verified = format!("{:x}", hash.finalize()) == expected_sha256.to_ascii_lowercase();
        }
        drop(moved);
        drop(candidate);
        // Windows cannot remove a nonempty quarantine while our original
        // marker HANDLE still keeps its entry alive (delete-pending).
        drop(marker);
        if !verified {
            self.recover_prune_for(path)?;
            return Ok(false);
        }
        self.unlink(&path_for_quarantine(path, &folder, "item")?)?;
        self.unlink(&path_for_quarantine(path, &folder, "target")?)?;
        // Remove the directory through a held handle.
        let handle = open_directory(&parent, &folder, false)?;
        delete_handle(&handle)?;
        drop(handle);
        dir_sync(&parent)?;
        Ok(true)
    }
}
fn path_for_quarantine(
    path: &Path,
    folder: &[u16],
    item: &str,
) -> StorageResult<std::path::PathBuf> {
    let folder = String::from_utf16(folder).map_err(|_| invalid())?;
    Ok(path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(folder)
        .join(item))
}
struct HashWriter<'a>(&'a mut Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.update(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) struct NativeTemporary {
    dir: File,
    target: Vec<u16>,
    file: File,
    published: bool,
}
impl NativeTemporary {
    pub(crate) fn writer(&self) -> StorageResult<File> {
        Ok(self.file.try_clone()?)
    }
    pub(crate) fn sync(&self) -> StorageResult<()> {
        self.file.sync_all()?;
        Ok(())
    }
    pub(crate) fn publish(mut self) -> StorageResult<()> {
        self.file.sync_all()?;
        // NtSetInformationFile rename is an atomic same-directory rename.
        // Existing targets may be replaced only after secure regular-file open.
        let _ = open_existing(&self.dir, &self.target)?;
        rename_handle(&self.file, &self.dir, &self.target, true, false)?;
        self.published = true;
        dir_sync(&self.dir)
    }
    pub(crate) fn publish_new(mut self) -> StorageResult<()> {
        self.file.sync_all()?;
        // FileLinkInformation with ReplaceIfExists=FALSE is an atomic no-clobber
        // publication; never create then rename over an independently created file.
        rename_handle(&self.file, &self.dir, &self.target, false, true)?;
        dir_sync(&self.dir)?;
        self.published = true;
        // Unlink temp name only after final entry is durable.
        delete_handle(&self.file)?;
        dir_sync(&self.dir)
    }
}
impl Drop for NativeTemporary {
    fn drop(&mut self) {
        if !self.published {
            let _ = delete_handle(&self.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    fn sandbox() -> PathBuf {
        let p = std::env::temp_dir().join(format!("vontaq-win-handle-{}", Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn write_read_replace() {
        let root = sandbox();
        let dir = NativeDirectory::open(&root).unwrap();
        dir.write_atomic(Path::new("nested/file.txt"), b"one")
            .unwrap();
        assert_eq!(fs::read(root.join("nested/file.txt")).unwrap(), b"one");
        dir.write_atomic(Path::new("nested/file.txt"), b"two")
            .unwrap();
        assert_eq!(fs::read(root.join("nested/file.txt")).unwrap(), b"two");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reparse_descendant_rejected() {
        let root = sandbox();
        let dir = NativeDirectory::open(&root).unwrap();
        let outside = sandbox();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("junction"))
            .arg(&outside)
            .status();
        if status.is_ok_and(|s| s.success()) {
            assert!(dir
                .write_atomic(Path::new("junction/escape.txt"), b"no")
                .is_err());
            assert!(!outside.join("escape.txt").exists());
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
    #[test]
    fn concurrent_create_does_not_overwrite() {
        let root = sandbox();
        let dir = NativeDirectory::open(&root).unwrap();
        let mut tmp = dir.temporary(Path::new("file.txt"), false).unwrap();
        tmp.file.write_all(b"new").unwrap();
        fs::write(root.join("file.txt"), b"user").unwrap();
        assert!(tmp.publish_new().is_err());
        assert_eq!(fs::read(root.join("file.txt")).unwrap(), b"user");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tracked_prune_does_not_remove_changed_user_file() {
        let root = sandbox();
        let dir = NativeDirectory::open(&root).unwrap();
        dir.write_atomic(Path::new("file.txt"), b"export").unwrap();
        let original = format!("{:x}", Sha256::digest(b"export"));
        fs::write(root.join("file.txt"), b"user-edit").unwrap();
        assert!(!dir.prune_tracked(Path::new("file.txt"), &original).unwrap());
        assert_eq!(fs::read(root.join("file.txt")).unwrap(), b"user-edit");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancelled_temporary_does_not_publish() {
        let root = sandbox();
        let dir = NativeDirectory::open(&root).unwrap();
        let temp = dir.temporary(Path::new("missing.txt"), false).unwrap();
        std::io::Write::write_all(&mut temp.writer().unwrap(), b"incomplete").unwrap();
        drop(temp);
        assert!(!root.join("missing.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn replaced_root_identity_rejected() {
        let root = sandbox();
        let observed = fs::metadata(&root).unwrap();
        // Pin the original directory HANDLE first: creation timestamps of
        // two rapidly created Windows directories may be identical. The
        // held file ID must still detect a path replacement reliably.
        let dir = NativeDirectory::open(&root).unwrap();
        let moved = root.with_extension("moved");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(dir.ensure_root_identity(&observed).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(moved);
    }
}

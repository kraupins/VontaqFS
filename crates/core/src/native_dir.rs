//! Handle-relative writes inside an explicitly approved native destination.
//!
//! A path preflight is not enough: a directory component can be replaced by a
//! symlink between validation and use. On Unix every descendant is resolved via
//! openat/mkdirat with O_NOFOLLOW from an owned directory descriptor. On Windows descendants use NtCreateFile with an owned RootDirectory handle;
//! unsupported platforms fail closed.
use crate::{StorageError, StorageResult};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

#[cfg(unix)]
mod imp {
    use super::*;
    use std::{
        ffi::CString,
        io,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::Component,
    };
    use uuid::Uuid;

    fn invalid() -> StorageError {
        StorageError::PathInvalid("unsafe native destination component".into())
    }
    fn component(name: &std::ffi::OsStr) -> StorageResult<CString> {
        CString::new(name.as_bytes()).map_err(|_| invalid())
    }
    fn opened(fd: libc::c_int) -> StorageResult<File> {
        if fd < 0 {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: open/openat returned this newly owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn open_dir(parent: &File, name: &CString) -> StorageResult<File> {
        opened(unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })
    }
    fn split(relative: &Path) -> StorageResult<Vec<CString>> {
        let mut names = Vec::new();
        for part in relative.components() {
            let Component::Normal(name) = part else {
                return Err(invalid());
            };
            names.push(component(name)?);
        }
        if names.is_empty() {
            return Err(invalid());
        }
        Ok(names)
    }
    fn stat(parent: &File, name: &CString) -> StorageResult<Option<libc::stat>> {
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        let status = unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                info.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if status == 0 {
            // SAFETY: fstatat wrote the complete structure on success.
            let info = unsafe { info.assume_init() };
            if info.st_mode & libc::S_IFMT != libc::S_IFREG {
                return Err(invalid());
            }
            return Ok(Some(info));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.into())
        }
    }
    fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }
    /// Stable per-target quarantine name, so a new process can find an
    /// interrupted prune without directory-wide scanning or following paths.
    fn prune_folder(name: &CString) -> StorageResult<CString> {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(name.as_bytes());
        CString::new(format!(".vontaqfs-prune-{digest:x}")).map_err(|_| invalid())
    }
    pub(crate) struct NativeDirectory {
        root: File,
    }
    impl NativeDirectory {
        /// Stable identity of the actual opened directory, not the pathname.
        pub(crate) fn identity(&self) -> StorageResult<String> {
            let meta = self.root.metadata()?;
            Ok(format!("unix:{}:{}", meta.dev(), meta.ino()))
        }
        /// Reject a root that changed between preflight and handle acquisition.
        /// This binds one export to the selected directory inode; historical
        /// grants still need a persisted identity to detect earlier replacement.
        pub(crate) fn ensure_root_identity(
            &self,
            observed: &std::fs::Metadata,
        ) -> StorageResult<()> {
            let held = self.root.metadata()?;
            if held.dev() != observed.dev() || held.ino() != observed.ino() {
                return Err(StorageError::PathInvalid(
                    "native destination changed before root handle was secured".into(),
                ));
            }
            Ok(())
        }
        pub(crate) fn open(path: &Path) -> StorageResult<Self> {
            if !path.is_absolute() {
                return Err(invalid());
            }
            let anchor = CString::new("/").map_err(|_| invalid())?;
            let mut dir = opened(unsafe {
                libc::open(
                    anchor.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            })?;
            for part in path.components() {
                match part {
                    Component::RootDir => {}
                    Component::Normal(name) => {
                        dir = open_dir(&dir, &component(name)?)?;
                    }
                    _ => return Err(invalid()),
                }
            }
            Ok(Self { root: dir })
        }
        fn parent(&self, relative: &Path, create: bool) -> StorageResult<(File, CString)> {
            let names = split(relative)?;
            let mut dir = self.root.try_clone()?;
            for name in &names[..names.len() - 1] {
                if create {
                    let ok = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) };
                    if ok != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
                    {
                        return Err(io::Error::last_os_error().into());
                    }
                }
                dir = open_dir(&dir, name)?;
            }
            Ok((dir, names.last().ok_or_else(invalid)?.clone()))
        }
        pub(crate) fn exists(&self, path: &Path) -> StorageResult<bool> {
            let (parent, name) = match self.parent(path, false) {
                Ok(v) => v,
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(false)
                }
                Err(error) => return Err(error),
            };
            Ok(stat(&parent, &name)?.is_some())
        }
        pub(crate) fn read(&self, path: &Path) -> StorageResult<File> {
            let (parent, name) = self.parent(path, false)?;
            let file = opened(unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            })?;
            if !file.metadata()?.is_file() {
                return Err(invalid());
            }
            Ok(file)
        }
        pub(crate) fn read_if_exists(&self, path: &Path) -> StorageResult<Option<File>> {
            let (parent, name) = match self.parent(path, false) {
                Ok(v) => v,
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(None)
                }
                Err(error) => return Err(error),
            };
            if stat(&parent, &name)?.is_none() {
                return Ok(None);
            }
            let file = opened(unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            })?;
            if !file.metadata()?.is_file() {
                return Err(invalid());
            }
            Ok(Some(file))
        }
        pub(crate) fn temporary(
            &self,
            path: &Path,
            create_parents: bool,
        ) -> StorageResult<NativeTemporary> {
            let (parent, target) = self.parent(path, create_parents)?;
            // Refuse to replace anything except a regular file, including a dangling symlink.
            let _ = stat(&parent, &target)?;
            for _ in 0..16 {
                let name = CString::new(format!(".vontaqfs-{}.tmp", Uuid::new_v4()))
                    .map_err(|_| invalid())?;
                let fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_WRONLY
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                };
                if fd >= 0 {
                    return Ok(NativeTemporary {
                        dir: parent,
                        name,
                        target,
                        file: opened(fd)?,
                        published: false,
                    });
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.into());
                }
            }
            Err(StorageError::StorageUnavailable(
                "cannot allocate safe native temporary file".into(),
            ))
        }
        pub(crate) fn write_atomic(&self, path: &Path, contents: &[u8]) -> StorageResult<()> {
            let mut temp = self.temporary(path, true)?;
            temp.file.write_all(contents)?;
            temp.publish()
        }
        /// Recover content from an interrupted quarantine before further export.
        /// Never overwrite a user file that appeared at its prior name. The
        /// durable marker is synced before any rename of the original inode.
        pub(crate) fn recover_prune_for(&self, path: &Path) -> StorageResult<bool> {
            let (parent, name) = match self.parent(path, false) {
                Ok(value) => value,
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(false)
                }
                Err(error) => return Err(error),
            };
            let folder = prune_folder(&name)?;
            let quarantine = match open_dir(&parent, &folder) {
                Ok(value) => value,
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(false)
                }
                Err(error) => return Err(error),
            };
            let marker = CString::new("target").map_err(|_| invalid())?;
            let item = CString::new("item").map_err(|_| invalid())?;
            let marker_file = opened(unsafe {
                libc::openat(
                    quarantine.as_raw_fd(),
                    marker.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            })?;
            if !marker_file.metadata()?.is_file() {
                return Err(invalid());
            }
            let mut expected = Vec::new();
            marker_file.take(256).read_to_end(&mut expected)?;
            if expected != name.as_bytes_with_nul() {
                return Err(StorageError::StorageUnavailable(
                    "prune quarantine marker is not valid for this destination".into(),
                ));
            }
            let mut recovered = false;
            if let Some(staged) = stat(&quarantine, &item)? {
                // If an earlier recovery linked the inode back but crashed
                // before removing `item`, recognize that idempotently.
                let already_restored = match stat(&parent, &name)? {
                    Some(current) if current.st_dev == staged.st_dev && current.st_ino == staged.st_ino => true,
                    Some(_) => return Err(StorageError::StorageUnavailable(
                        "prune quarantine conflicts with a different destination file; recovery required".into())),
                    None => false,
                };
                if !already_restored {
                    // Atomic no-replace restore; a concurrent user-created file
                    // blocks recovery without erasing either copy.
                    if unsafe {
                        libc::linkat(
                            quarantine.as_raw_fd(),
                            item.as_ptr(),
                            parent.as_raw_fd(),
                            name.as_ptr(),
                            0,
                        )
                    } < 0
                    {
                        return Err(StorageError::StorageUnavailable(
                            "prune quarantine conflicts with a destination file; recovery required"
                                .into(),
                        ));
                    }
                }
                parent.sync_all()?;
                if unsafe { libc::unlinkat(quarantine.as_raw_fd(), item.as_ptr(), 0) } < 0 {
                    return Err(io::Error::last_os_error().into());
                }
                recovered = true;
            }
            if unsafe { libc::unlinkat(quarantine.as_raw_fd(), marker.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            quarantine.sync_all()?;
            if unsafe { libc::unlinkat(parent.as_raw_fd(), folder.as_ptr(), libc::AT_REMOVEDIR) }
                < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            parent.sync_all()?;
            Ok(recovered)
        }
        /// Prune a tracked file only when the bytes moved into quarantine still
        /// match the previous export manifest. We must not hash one pathname and
        /// then unlink an unrelated file that appeared at the same pathname.
        ///
        /// A private same-directory quarantine retains unexpected content for
        /// restoration. In a crash it stays on disk rather than deleting data;
        /// the export journal still records an interrupted operation.
        pub(crate) fn prune_tracked(
            &self,
            path: &Path,
            expected_sha256: &str,
        ) -> StorageResult<bool> {
            use sha2::{Digest, Sha256};
            if expected_sha256.len() != 64
                || !expected_sha256.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(invalid());
            }
            let (parent, name) = match self.parent(path, false) {
                Ok(value) => value,
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(false)
                }
                Err(error) => return Err(error),
            };
            // The initial hash is a cheap exclusion for user-modified files.
            // The second hash on the *quarantined* inode is authoritative.
            let Some(initial) = self.read_if_exists(path)? else {
                return Ok(false);
            };
            let mut hasher = Sha256::new();
            let mut initial = initial;
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let length = initial.read(&mut chunk)?;
                if length == 0 {
                    break;
                }
                hasher.update(&chunk[..length]);
            }
            if format!("{:x}", hasher.finalize()) != expected_sha256.to_ascii_lowercase() {
                return Ok(false);
            }
            let original_metadata = initial.metadata()?;
            // A deterministic directory lets recovery locate an interrupted
            // rename by the tracked destination path. Existing quarantine is
            // never overwritten: callers must reconcile it first.
            let folder = prune_folder(&name)?;
            if unsafe { libc::mkdirat(parent.as_raw_fd(), folder.as_ptr(), 0o700) } < 0 {
                return Err(StorageError::StorageUnavailable(format!(
                    "tracked prune quarantine already exists or could not be created: {}",
                    io::Error::last_os_error()
                )));
            }
            let quarantine_dir = open_dir(&parent, &folder)?;
            let marker = CString::new("target").map_err(|_| invalid())?;
            let mut marker_file = opened(unsafe {
                libc::openat(
                    quarantine_dir.as_raw_fd(),
                    marker.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            })?;
            marker_file.write_all(name.as_bytes_with_nul())?;
            marker_file.sync_all()?;
            quarantine_dir.sync_all()?;
            parent.sync_all()?;
            let item = CString::new("item").map_err(|_| invalid())?;
            if unsafe {
                libc::renameat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    quarantine_dir.as_raw_fd(),
                    item.as_ptr(),
                )
            } < 0
            {
                let error = io::Error::last_os_error();
                let _ = unsafe { libc::unlinkat(quarantine_dir.as_raw_fd(), marker.as_ptr(), 0) };
                let _ = unsafe {
                    libc::unlinkat(parent.as_raw_fd(), folder.as_ptr(), libc::AT_REMOVEDIR)
                };
                return Err(error.into());
            }
            parent.sync_all()?;
            quarantine_dir.sync_all()?;
            // Now any concurrent replacement at the old pathname is untouched.
            // Compare the exact inode moved, not the old path name.
            let confirmed = (|| -> StorageResult<bool> {
                let file = opened(unsafe {
                    libc::openat(
                        quarantine_dir.as_raw_fd(),
                        item.as_ptr(),
                        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                    )
                })?;
                let meta = file.metadata()?;
                if !meta.is_file() || !same_file(&original_metadata, &meta) {
                    return Ok(false);
                }
                let mut reader = file;
                let mut hash = Sha256::new();
                loop {
                    let length = reader.read(&mut chunk)?;
                    if length == 0 {
                        break;
                    }
                    hash.update(&chunk[..length]);
                }
                Ok(
                    format!("{:x}", hash.finalize()) == expected_sha256.to_ascii_lowercase()
                        && same_file(&original_metadata, &reader.metadata()?),
                )
            })();
            // Errors and changes both require restoring the quarantined file.
            // linkat cannot overwrite a different file created at the old path;
            // in that case the quarantine survives as recovery evidence.
            if !matches!(&confirmed, Ok(true))
                && unsafe {
                    libc::linkat(
                        quarantine_dir.as_raw_fd(),
                        item.as_ptr(),
                        parent.as_raw_fd(),
                        name.as_ptr(),
                        0,
                    )
                } < 0
            {
                return Err(StorageError::StorageUnavailable(format!(
                    "tracked prune changed during verification; retained recovery file in {}",
                    folder.to_string_lossy()
                )));
            }
            if unsafe { libc::unlinkat(quarantine_dir.as_raw_fd(), item.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            if unsafe { libc::unlinkat(quarantine_dir.as_raw_fd(), marker.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            quarantine_dir.sync_all()?;
            if unsafe { libc::unlinkat(parent.as_raw_fd(), folder.as_ptr(), libc::AT_REMOVEDIR) }
                < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            parent.sync_all()?;
            confirmed
        }
        pub(crate) fn unlink(&self, path: &Path) -> StorageResult<()> {
            let (parent, name) = self.parent(path, false)?;
            if stat(&parent, &name)?.is_none() {
                return Ok(());
            }
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            parent.sync_all()?;
            Ok(())
        }
        pub(crate) fn rename(&self, source: &Path, target: &Path) -> StorageResult<()> {
            let (from_dir, from_name) = self.parent(source, false)?;
            let (to_dir, to_name) = self.parent(target, false)?;
            if stat(&from_dir, &from_name)?.is_none() {
                return Err(StorageError::NotFound("native journal".into()));
            }
            let _ = stat(&to_dir, &to_name)?;
            if unsafe {
                libc::renameat(
                    from_dir.as_raw_fd(),
                    from_name.as_ptr(),
                    to_dir.as_raw_fd(),
                    to_name.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            to_dir.sync_all()?;
            Ok(())
        }
    }
    pub(crate) struct NativeTemporary {
        dir: File,
        name: CString,
        target: CString,
        file: File,
        published: bool,
    }
    impl NativeTemporary {
        /// Atomically publish only if the target is still absent. Unlike a
        /// preflight `exists()` check, linkat fails without replacing a user
        /// file that appeared while export bytes were being staged.
        pub(crate) fn publish_new(mut self) -> StorageResult<()> {
            self.file.sync_all()?;
            if unsafe {
                libc::linkat(
                    self.dir.as_raw_fd(),
                    self.name.as_ptr(),
                    self.dir.as_raw_fd(),
                    self.target.as_ptr(),
                    0,
                )
            } < 0
            {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    return Err(StorageError::Conflict(
                        "native target appeared before publication".into(),
                    ));
                }
                return Err(error.into());
            }
            // The hard link and the temp refer to one inode. If cleanup fails,
            // the destination still contains the completed bytes; the temp is
            // left as recoverable evidence, never a partial destination.
            self.dir.sync_all()?;
            self.published = true;
            if unsafe { libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), 0) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
            self.dir.sync_all()?;
            Ok(())
        }
        pub(crate) fn writer(&self) -> StorageResult<File> {
            Ok(self.file.try_clone()?)
        }
        pub(crate) fn sync(&self) -> StorageResult<()> {
            self.file.sync_all()?;
            Ok(())
        }
        pub(crate) fn publish(mut self) -> StorageResult<()> {
            self.file.sync_all()?;
            // Recheck against symlink insertion at the final leaf. renameat does
            // not traverse that leaf, even if it changes after this check.
            let _ = stat(&self.dir, &self.target)?;
            if unsafe {
                libc::renameat(
                    self.dir.as_raw_fd(),
                    self.name.as_ptr(),
                    self.dir.as_raw_fd(),
                    self.target.as_ptr(),
                )
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            self.published = true;
            self.dir.sync_all()?;
            Ok(())
        }
    }
    impl Drop for NativeTemporary {
        fn drop(&mut self) {
            if !self.published {
                let _ = unsafe { libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), 0) };
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::*;
    fn unavailable() -> StorageError {
        StorageError::StorageUnavailable("native destination requires a verified handle-relative implementation on this platform".into())
    }
    pub(crate) struct NativeDirectory;
    impl NativeDirectory {
        pub(crate) fn identity(&self) -> StorageResult<String> {
            Err(unavailable())
        }
        pub(crate) fn ensure_root_identity(
            &self,
            _observed: &std::fs::Metadata,
        ) -> StorageResult<()> {
            Err(unavailable())
        }
        pub(crate) fn open(_path: &Path) -> StorageResult<Self> {
            Err(unavailable())
        }
        pub(crate) fn exists(&self, _path: &Path) -> StorageResult<bool> {
            Err(unavailable())
        }
        pub(crate) fn read(&self, _path: &Path) -> StorageResult<File> {
            Err(unavailable())
        }
        pub(crate) fn read_if_exists(&self, _path: &Path) -> StorageResult<Option<File>> {
            Err(unavailable())
        }
        pub(crate) fn temporary(
            &self,
            _path: &Path,
            _create_parents: bool,
        ) -> StorageResult<NativeTemporary> {
            Err(unavailable())
        }
        pub(crate) fn write_atomic(&self, _path: &Path, _data: &[u8]) -> StorageResult<()> {
            Err(unavailable())
        }
        pub(crate) fn recover_prune_for(&self, _path: &Path) -> StorageResult<bool> {
            Err(unavailable())
        }
        pub(crate) fn prune_tracked(
            &self,
            _path: &Path,
            _expected_sha256: &str,
        ) -> StorageResult<bool> {
            Err(unavailable())
        }
        pub(crate) fn unlink(&self, _path: &Path) -> StorageResult<()> {
            Err(unavailable())
        }
        pub(crate) fn rename(&self, _src: &Path, _dst: &Path) -> StorageResult<()> {
            Err(unavailable())
        }
    }
    pub(crate) struct NativeTemporary;
    impl NativeTemporary {
        pub(crate) fn publish_new(self) -> StorageResult<()> {
            Err(unavailable())
        }
        pub(crate) fn writer(&self) -> StorageResult<File> {
            Err(unavailable())
        }
        pub(crate) fn sync(&self) -> StorageResult<()> {
            Err(unavailable())
        }
        pub(crate) fn publish(self) -> StorageResult<()> {
            Err(unavailable())
        }
    }
}
#[cfg(windows)]
#[path = "native_dir_windows.rs"]
mod imp;
pub(crate) use imp::NativeDirectory;

/// A bounded, handle-relative read for tracking metadata. Missing is distinct
/// from unreadable/unsafe: never interpret a symlink as "no old manifest".
pub(crate) fn read_small(
    dir: &NativeDirectory,
    path: &Path,
    limit: u64,
) -> StorageResult<Option<Vec<u8>>> {
    let Some(file) = dir.read_if_exists(path)? else {
        return Ok(None);
    };
    let size = file.metadata()?.len();
    if size > limit {
        return Err(StorageError::RequestInvalid(
            "native tracking file exceeds size limit".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(StorageError::RequestInvalid(
            "native tracking file exceeds size limit".into(),
        ));
    }
    Ok(Some(bytes))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use sha2::Digest;
    use std::{fs, os::unix::fs::symlink, path::PathBuf};

    fn sandbox(label: &str) -> PathBuf {
        // macOS temp_dir() commonly starts with /var, a symlink to /private/var.
        // Keep production O_NOFOLLOW unchanged: create test roots under the
        // resolved physical directory instead of traversing that symlink.
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("vontaq-native-dir-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn nested_publication_and_read_are_handle_relative() {
        let dir = sandbox("nested");
        let output = NativeDirectory::open(&dir).unwrap();
        output
            .write_atomic(Path::new("a/b/content.txt"), b"first")
            .unwrap();
        assert_eq!(fs::read(dir.join("a/b/content.txt")).unwrap(), b"first");
        output
            .write_atomic(Path::new("a/b/content.txt"), b"second")
            .unwrap();
        assert_eq!(fs::read(dir.join("a/b/content.txt")).unwrap(), b"second");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn symlink_parent_and_symlink_leaf_cannot_escape() {
        let dir = sandbox("links");
        let outside = sandbox("outside");
        let output = NativeDirectory::open(&dir).unwrap();
        symlink(&outside, dir.join("linked")).unwrap();
        assert!(output
            .write_atomic(Path::new("linked/escape.txt"), b"bad")
            .is_err());
        assert!(!outside.join("escape.txt").exists());
        symlink(outside.join("missing.txt"), dir.join("dangling")).unwrap();
        assert!(output.write_atomic(Path::new("dangling"), b"bad").is_err());
        assert!(!outside.join("missing.txt").exists());
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn opening_replaced_root_is_detected_against_preflight_identity() {
        let parent = sandbox("root-identity");
        let selected = parent.join("selected");
        fs::create_dir(&selected).unwrap();
        let observed = fs::symlink_metadata(&selected).unwrap();
        fs::rename(&selected, parent.join("old")).unwrap();
        fs::create_dir(&selected).unwrap();
        let output = NativeDirectory::open(&selected).unwrap();
        assert!(output.ensure_root_identity(&observed).is_err());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn changing_root_path_does_not_redirect_open_descriptor() {
        let parent = sandbox("root-swap");
        let outside = sandbox("root-swap-outside");
        let selected = parent.join("selected");
        fs::create_dir(&selected).unwrap();
        let output = NativeDirectory::open(&selected).unwrap();
        let moved = parent.join("moved");
        fs::rename(&selected, &moved).unwrap();
        symlink(&outside, &selected).unwrap();
        output
            .write_atomic(Path::new("hello.txt"), b"safe")
            .unwrap();
        assert_eq!(fs::read(moved.join("hello.txt")).unwrap(), b"safe");
        assert!(!outside.join("hello.txt").exists());
        fs::remove_dir_all(parent).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn abandoned_unique_temp_is_deleted() {
        let dir = sandbox("temp-drop");
        let output = NativeDirectory::open(&dir).unwrap();
        {
            let temp = output.temporary(Path::new("output.txt"), false).unwrap();
            let mut writer = temp.writer().unwrap();
            std::io::Write::write_all(&mut writer, b"not committed").unwrap();
        }
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuse_symlink_in_ancestor_of_selected_root() {
        let parent = sandbox("root-link");
        let target = sandbox("root-target");
        symlink(&target, parent.join("alias")).unwrap();
        assert!(NativeDirectory::open(&parent.join("alias")).is_err());
        fs::remove_dir_all(parent).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn publishing_new_never_overwrites_concurrent_user_file() {
        let dir = sandbox("publish-new");
        let output = NativeDirectory::open(&dir).unwrap();
        let temp = output.temporary(Path::new("file.txt"), false).unwrap();
        std::io::Write::write_all(&mut temp.writer().unwrap(), b"export").unwrap();
        fs::write(dir.join("file.txt"), b"user").unwrap();
        assert!(matches!(temp.publish_new(), Err(StorageError::Conflict(_))));
        assert_eq!(fs::read(dir.join("file.txt")).unwrap(), b"user");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tracked_prune_preserves_changed_file_and_removes_only_verified_bytes() {
        let dir = sandbox("tracked-prune");
        let output = NativeDirectory::open(&dir).unwrap();
        output
            .write_atomic(Path::new("a/file.txt"), b"original")
            .unwrap();
        let checksum = format!("{:x}", sha2::Sha256::digest(b"original"));
        assert!(!output
            .prune_tracked(
                Path::new("a/file.txt"),
                &format!("{:x}", sha2::Sha256::digest(b"different"))
            )
            .unwrap());
        assert_eq!(fs::read(dir.join("a/file.txt")).unwrap(), b"original");
        assert!(output
            .prune_tracked(Path::new("a/file.txt"), &checksum)
            .unwrap());
        assert!(!dir.join("a/file.txt").exists());
        assert_eq!(fs::read_dir(dir.join("a")).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_prune_restores_original_without_overwriting_other_files() {
        let dir = sandbox("prune-crash-recovery");
        let output = NativeDirectory::open(&dir).unwrap();
        output
            .write_atomic(Path::new("file.txt"), b"previous-export")
            .unwrap();
        let folder = format!(".vontaqfs-prune-{:x}", sha2::Sha256::digest(b"file.txt"));
        let quarantine = dir.join(&folder);
        fs::create_dir(&quarantine).unwrap();
        fs::write(quarantine.join("target"), b"file.txt\0").unwrap();
        fs::rename(dir.join("file.txt"), quarantine.join("item")).unwrap();
        assert!(output.recover_prune_for(Path::new("file.txt")).unwrap());
        assert_eq!(fs::read(dir.join("file.txt")).unwrap(), b"previous-export");
        assert!(!quarantine.exists());
        assert!(!output.recover_prune_for(Path::new("file.txt")).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_prune_conflict_preserves_quarantine() {
        let dir = sandbox("prune-crash-conflict");
        let output = NativeDirectory::open(&dir).unwrap();
        let folder = format!(".vontaqfs-prune-{:x}", sha2::Sha256::digest(b"file.txt"));
        let quarantine = dir.join(&folder);
        fs::create_dir(&quarantine).unwrap();
        fs::write(quarantine.join("target"), b"file.txt\0").unwrap();
        fs::write(quarantine.join("item"), b"previous-export").unwrap();
        fs::write(dir.join("file.txt"), b"user-write").unwrap();
        assert!(output.recover_prune_for(Path::new("file.txt")).is_err());
        assert_eq!(fs::read(dir.join("file.txt")).unwrap(), b"user-write");
        assert_eq!(
            fs::read(quarantine.join("item")).unwrap(),
            b"previous-export"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tracked_prune_never_unlinks_symlink_target() {
        let dir = sandbox("tracked-symlink");
        let outside = sandbox("tracked-outside");
        fs::write(outside.join("file"), b"original").unwrap();
        symlink(outside.join("file"), dir.join("link")).unwrap();
        let output = NativeDirectory::open(&dir).unwrap();
        let checksum = format!("{:x}", sha2::Sha256::digest(b"original"));
        assert!(output.prune_tracked(Path::new("link"), &checksum).is_err());
        assert_eq!(fs::read(outside.join("file")).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}

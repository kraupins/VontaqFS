//! Kernel-owned storage-root exclusion. The lock file is never unlinked to steal
//! ownership: ownership is the open OS handle, not a PID written to disk.
use crate::{StorageError, StorageResult};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

#[derive(Debug)]
pub struct StorageRootLock {
    // Keep the exact locked inode / Windows non-shared handle alive until Core drops.
    _file: File,
}

impl StorageRootLock {
    pub fn acquire(root: &Path) -> StorageResult<Self> {
        let runtime = root.join("runtime");
        fs::create_dir_all(&runtime)?;
        if fs::symlink_metadata(&runtime)?.file_type().is_symlink() {
            return Err(StorageError::StorageUnavailable(
                "runtime directory must not be a symlink".into(),
            ));
        }
        let path = runtime.join("storage-root.lock");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(StorageError::StorageUnavailable(
                    "storage lock path is not a regular file".into(),
                ));
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // No sharing of read/write/delete access while the engine is alive.
            options.share_mode(0);
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
        let file = options.open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)))
            {
                StorageError::StorageRootBusy
            } else {
                error.into()
            }
        })?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = std::io::Error::last_os_error();
                return if error.raw_os_error() == Some(libc::EWOULDBLOCK)
                    || error.raw_os_error() == Some(libc::EAGAIN)
                {
                    Err(StorageError::StorageRootBusy)
                } else {
                    Err(error.into())
                };
            }
        }
        Ok(Self { _file: file })
    }
}

use std::path::{Component, Path, PathBuf};

use icu_normalizer::ComposingNormalizer;

use crate::{StorageError, StorageResult};

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalPath {
    normalized: String,
    collision_key: String,
}

impl LogicalPath {
    pub fn parse(raw: &str) -> StorageResult<Self> {
        if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') || raw.contains('\\') {
            return Err(StorageError::PathInvalid(
                "logical path is empty, too long, contains NUL, or uses native separators".into(),
            ));
        }
        if !raw.starts_with('/') || raw.starts_with("//") {
            return Err(StorageError::PathInvalid(
                "logical paths must use one absolute POSIX-style root slash".into(),
            ));
        }
        let trimmed = raw.trim_start_matches('/');
        if trimmed.is_empty() {
            return Ok(Self {
                normalized: "/".into(),
                collision_key: "/".into(),
            });
        }

        let nfc = ComposingNormalizer::new_nfc();
        let mut segments = Vec::new();
        let mut collision = Vec::new();
        for segment in trimmed.split('/') {
            validate_segment(segment)?;
            let normalized = nfc.normalize(segment).into_owned();
            validate_segment(&normalized)?;
            segments.push(normalized.clone());
            collision.push(normalized.to_lowercase());
        }
        Ok(Self {
            normalized: format!("/{}", segments.join("/")),
            collision_key: format!("/{}", collision.join("/")),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.normalized
    }
    pub fn collision_key(&self) -> &str {
        &self.collision_key
    }

    pub fn relative(&self) -> PathBuf {
        if self.normalized == "/" {
            return PathBuf::new();
        }
        self.normalized.trim_start_matches('/').split('/').collect()
    }
}

fn validate_segment(segment: &str) -> StorageResult<()> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(StorageError::PathInvalid(
            "empty/dot/traversal segment is not allowed".into(),
        ));
    }
    if segment.eq_ignore_ascii_case(".vontaqfs") {
        return Err(StorageError::PathInvalid(
            "the .vontaqfs namespace is runtime-owned".into(),
        ));
    }
    if segment.chars().any(|ch| ch <= '\u{1f}')
        || segment.ends_with(' ')
        || segment.ends_with('.')
        || segment.contains(':')
        || segment.contains('*')
        || segment.contains('?')
        || segment.contains('"')
        || segment.contains('<')
        || segment.contains('>')
        || segment.contains('|')
    {
        return Err(StorageError::PathInvalid(
            "segment is not portable across Windows/macOS".into(),
        ));
    }
    let stem = segment.split('.').next().unwrap_or(segment);
    if WINDOWS_RESERVED
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
    {
        return Err(StorageError::PathInvalid(format!(
            "reserved Windows device name: {segment}"
        )));
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_escape(data_root: &Path, candidate: &Path) -> StorageResult<()> {
    match std::fs::symlink_metadata(data_root) {
        Ok(metadata) if is_link_like(&metadata) => {
            return Err(StorageError::PathInvalid(
                "managed data root is a symlink/reparse point".into(),
            ))
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(StorageError::PathInvalid(
                "managed data root is not a directory".into(),
            ))
        }
        Ok(_) => {}
        Err(error) => return Err(error.into()),
    }
    if !candidate.starts_with(data_root) {
        return Err(StorageError::PathInvalid(
            "resolved path escaped the managed space".into(),
        ));
    }
    let rel = candidate
        .strip_prefix(data_root)
        .map_err(|_| StorageError::PathInvalid("resolved path escaped the managed space".into()))?;
    let mut current = data_root.to_path_buf();
    for component in rel.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(StorageError::PathInvalid(
                "non-normal native path component".into(),
            ));
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_like(&metadata) => {
                return Err(StorageError::PathInvalid(
                    "symlink/reparse traversal is not allowed".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_reserved_and_runtime_namespace() {
        for value in [
            "relative",
            "//double-root",
            "/../escape",
            "/a//b",
            "/.vontaqfs/x",
            "/CON",
            "/file. ",
            "/control\u{1f}",
            "/a\\b",
        ] {
            assert!(LogicalPath::parse(value).is_err(), "{value}");
        }
        assert_eq!(
            LogicalPath::parse("/ok/file.txt").unwrap().as_str(),
            "/ok/file.txt"
        );
    }

    #[test]
    fn unicode_and_case_collisions_share_one_portable_key() {
        let composed = LogicalPath::parse("/Café/File.JSON").unwrap();
        let decomposed = LogicalPath::parse("/Cafe\u{301}/file.json").unwrap();
        assert_eq!(composed.collision_key(), decomposed.collision_key());
    }
}

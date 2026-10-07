use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageClass {
    Persistent,
    Cache,
    Temporary,
}

impl StorageClass {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Persistent => "persistent",
            Self::Cache => "cache",
            Self::Temporary => "temporary",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "persistent" => Some(Self::Persistent),
            "cache" => Some(Self::Cache),
            "temporary" => Some(Self::Temporary),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageCategory {
    #[default]
    UserData,
    Generated,
    Index,
    Backup,
    Snapshot,
    Custom,
}

impl StorageCategory {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::UserData => "user-data",
            Self::Generated => "generated",
            Self::Index => "index",
            Self::Backup => "backup",
            Self::Snapshot => "snapshot",
            Self::Custom => "custom",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "user-data" => Some(Self::UserData),
            "generated" => Some(Self::Generated),
            "index" => Some(Self::Index),
            "backup" => Some(Self::Backup),
            "snapshot" => Some(Self::Snapshot),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationKind {
    FigmaPlugin,
    FigmaWidget,
    OtherSupportedClient,
}

impl ApplicationKind {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::FigmaPlugin => "figma-plugin",
            Self::FigmaWidget => "figma-widget",
            Self::OtherSupportedClient => "other-supported-client",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "figma-plugin" => Some(Self::FigmaPlugin),
            "figma-widget" => Some(Self::FigmaWidget),
            "other-supported-client" => Some(Self::OtherSupportedClient),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRecord {
    pub id: String,
    pub kind: ApplicationKind,
    pub external_id: String,
    pub display_name: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRecord {
    pub id: String,
    pub application_id: String,
    pub client_instance_id: String,
    pub credential_hash: String,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceRecord {
    pub id: String,
    pub owner_application_id: String,
    pub key: String,
    pub display_name: Option<String>,
    pub storage_class: StorageClass,
    pub storage_category: StorageCategory,
    pub created_at_ms: i64,
    pub last_used_at_ms: i64,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub format_version: u32,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KvRecord {
    pub key: String,
    pub value: serde_json::Value,
    pub version: u64,
    pub etag: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub content_type: Option<String>,
    pub format_id: Option<String>,
    pub opaque: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub path: String,
    pub version: u64,
    pub etag: String,
    pub size: u64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opaque: Option<bool>,
}

impl FileRecord {
    pub fn metadata(&self) -> FileMetadata {
        FileMetadata {
            content_type: self.content_type.clone(),
            format_id: self.format_id.clone(),
            opaque: self.opaque,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFormatDescriptor {
    pub id: String,
    pub application_id: String,
    pub extension: Option<String>,
    pub display_name: String,
    pub content_type: Option<String>,
    pub opaque: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpaceManifest {
    pub format: String,
    pub format_version: u32,
    pub space_id: String,
    pub owner_application_id: String,
    pub application_kind: ApplicationKind,
    pub application_external_id: String,
    pub application_display_name: String,
    pub key: String,
    pub display_name: Option<String>,
    pub storage_class: StorageClass,
    #[serde(default)]
    pub storage_category: StorageCategory,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RepairOutcome {
    Healthy,
    Repaired,
    DestructiveRecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    pub space_id: String,
    pub outcome: RepairOutcome,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub repaired_entries: u64,
    pub quarantined_runtime_artifacts: u64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceExportReport {
    pub space_id: String,
    pub destination: String,
    pub exported_files: u64,
    pub exported_bytes: u64,
    pub archive_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceClearReport {
    pub space_id: String,
    pub deleted_files: u64,
    pub deleted_kv_entries: u64,
    pub released_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DirectoryGrantCapability {
    Read,
    Write,
    ReadWrite,
}

impl DirectoryGrantCapability {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read-write",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "read-write" => Some(Self::ReadWrite),
            _ => None,
        }
    }

    pub fn can_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
    pub fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryGrantRecord {
    pub id: String,
    pub application_id: String,
    pub label: String,
    pub physical_path: String,
    pub capability: DirectoryGrantCapability,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeExportMode {
    File,
    Files,
    Directory,
    Archive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportConflictPolicy {
    Replace,
    Skip,
    Rename,
    Ask,
    UpdateChanged,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportBookkeepingPolicy {
    #[default]
    Destination,
    Internal,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportPrunePolicy {
    #[default]
    None,
    Tracked,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DirectoryExportLayout {
    #[default]
    Preserve,
    Contents,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportPresetRecord {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub destination_grant_id: String,
    pub mode: NativeExportMode,
    pub conflict_policy: ExportConflictPolicy,
    pub source_path: String,
    pub archive_format: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeExportReport {
    pub space_id: String,
    pub destination_label: String,
    pub guarantee: String,
    pub added: u64,
    pub changed: u64,
    pub skipped: u64,
    pub unchanged: u64,
    pub conflicts: u64,
    pub deleted: u64,
    pub exported_bytes: u64,
    pub manifest_written: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImportMode {
    File,
    Files,
    Directory,
    Archive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ImportConflictPolicy {
    Replace,
    Skip,
    Rename,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeImportReport {
    pub space_id: String,
    pub source_label: String,
    pub imported_files: Vec<FileRecord>,
    pub imported_bytes: u64,
    pub skipped: u64,
    pub conflicts: u64,
    pub archive_extracted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortableBackupReport {
    pub space_id: String,
    pub destination: String,
    pub backup_format_version: u32,
    pub backed_up_files: u64,
    pub backed_up_bytes: u64,
    pub kv_entries: u64,
    pub archive_sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreConflictPolicy {
    Fail,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortableRestoreReport {
    pub application_id: String,
    pub space_id: String,
    pub space_key: String,
    pub restored_files: u64,
    pub restored_bytes: u64,
    pub restored_kv_entries: u64,
    pub created_application: bool,
    pub created_space: bool,
    pub pairing_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub id: String,
    pub space_id: String,
    pub created_at_ms: i64,
    pub label: Option<String>,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub source_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRestoreReport {
    pub snapshot_id: String,
    pub space_id: String,
    pub restored_files: u64,
    pub restored_bytes: u64,
    pub restored_kv_entries: u64,
    pub guarantee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageDiagnostics {
    pub registry_quick_check: String,
    pub application_count: u64,
    pub active_pairing_count: u64,
    pub space_count: u64,
    pub persistent_space_count: u64,
    pub cache_space_count: u64,
    pub temporary_space_count: u64,
    pub logical_bytes: u64,
    pub file_count: u64,
}

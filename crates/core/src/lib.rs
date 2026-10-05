mod error;
mod model;
mod path;
mod storage;

pub use error::{StorageError, StorageResult};
pub use model::{
    ApplicationFormatDescriptor, ApplicationKind, ApplicationRecord, DirectoryGrantCapability,
    DirectoryGrantRecord, ExportConflictPolicy, ExportPresetRecord, FileMetadata, FileRecord,
    ImportConflictPolicy, KvRecord, NativeExportMode, NativeExportReport, NativeImportMode,
    NativeImportReport, PairingRecord, PortableBackupReport, PortableRestoreReport, RepairOutcome,
    RepairReport, RestoreConflictPolicy, SnapshotRecord, SnapshotRestoreReport, SpaceExportReport,
    SpaceRecord, StorageCategory, StorageClass, StorageDiagnostics,
};
pub use path::LogicalPath;
pub use storage::{AtomicWriteFault, StorageEngine};

pub(crate) use model::SpaceManifest;

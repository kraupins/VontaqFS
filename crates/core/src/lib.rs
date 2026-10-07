mod error;
mod model;
mod path;
mod storage;

pub use error::{StorageError, StorageResult};
pub use model::{
    ApplicationFormatDescriptor, ApplicationKind, ApplicationRecord, DirectoryExportLayout,
    DirectoryGrantCapability, DirectoryGrantRecord, ExportBookkeepingPolicy, ExportConflictPolicy,
    ExportPresetRecord, ExportPrunePolicy, FileMetadata, FileRecord, ImportConflictPolicy,
    KvRecord, NativeExportMode, NativeExportReport, NativeImportMode, NativeImportReport,
    PairingRecord, PortableBackupReport, PortableRestoreReport, RepairOutcome, RepairReport,
    RestoreConflictPolicy, SnapshotRecord, SnapshotRestoreReport, SpaceClearReport,
    SpaceExportReport, SpaceRecord, StorageCategory, StorageClass, StorageDiagnostics,
};
pub use path::LogicalPath;
pub use storage::{AtomicWriteFault, StorageEngine};

pub(crate) use model::SpaceManifest;

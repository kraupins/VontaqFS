mod admission;
mod error;
mod model;
mod native_dir;
mod path;
mod root_lock;
mod storage;

pub use admission::{AdmissionLease, AdmissionManager, Claim, ClaimMode, MutationContext};
pub use error::{StorageError, StorageResult};
pub use model::{
    ApplicationFormatDescriptor, ApplicationKind, ApplicationRecord, DirectoryExportLayout,
    DirectoryGrantCapability, DirectoryGrantRecord, ExportBookkeepingPolicy, ExportConflictPolicy,
    ExportPresetRecord, ExportPrunePolicy, FileMetadata, FileRecord, ImportConflictPolicy,
    KvRecord, MutationDisposition, MutationResult, NativeExportMode, NativeExportReport,
    NativeImportMode, NativeImportReport, PairingRecord, PairingTransportPolicy,
    PortableBackupReport, PortableRestoreReport, RepairOutcome, RepairReport,
    RestoreConflictPolicy, SnapshotRecord, SnapshotRestoreReport, SpaceClearReport,
    SpaceExportReport, SpaceRecord, StorageCategory, StorageClass, StorageDiagnostics,
};
pub use path::LogicalPath;
pub use storage::{
    AtomicWriteFault, BatchCapacityOperation, BatchExecution, DestinationGrantAdmission,
    StorageEngine, StreamOperationRecord,
};

pub(crate) use model::SpaceManifest;

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    fs::{File, OpenOptions},
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, Semaphore},
    time::{timeout, Duration},
};
use uuid::Uuid;
pub use vontaqfs_core::SnapshotRecord;
use vontaqfs_core::{
    ApplicationFormatDescriptor, ApplicationKind, ApplicationRecord, DirectoryGrantCapability,
    DirectoryGrantRecord, ExportConflictPolicy, ExportPresetRecord, FileMetadata, FileRecord,
    ImportConflictPolicy, KvRecord, LogicalPath, NativeExportMode, NativeImportMode,
    RestoreConflictPolicy, SpaceRecord, StorageCategory, StorageClass, StorageDiagnostics,
    StorageEngine, StorageError, StorageResult,
};
use vontaqfs_protocol::{
    BatchItemErrorWire, BatchItemResultWire, BatchOperationWire, BatchRequest, BatchResponse,
    ClientIdentity, ClientKind, CreateDestinationGrantRequest, CreateSnapshotRequest,
    DeleteExportPresetRequest, DeleteFormatRequest, DeleteSnapshotRequest, DestinationGrantWire,
    DirectoryGrantCapabilityWire, ErrorBody, ErrorCode, ErrorResponse, EventKindWire,
    EventPollRequest, EventPollResponse, ExportConflictPolicyWire, ExportPresetWire,
    FileMetadataWire, FileStatRequest, FileStatResponse, FileWire, FormatDescriptorWire,
    FsCopyRequest, FsDeleteRequest, FsMoveRequest, HealthResponse, ImportConflictPolicyWire,
    KvGetRequest, KvGetResponse, KvSetRequest, KvWire, ListDestinationGrantsResponse,
    ListExportPresetsResponse, ListFormatsResponse, ListSnapshotsRequest, ListSnapshotsResponse,
    ListSpacesResponse, NativeExportModeWire, NativeImportModeWire, OpenSpaceRequest,
    OperationCancelRequest, OperationPresentationWire, OperationRequestWire,
    OperationStatusRequest, PairingPollRequest, PairingPollResponse, PairingRequest,
    PairingRequestResponse, PairingStatus, RegisterFormatRequest, RestoreSnapshotRequest,
    RevokeDestinationGrantRequest, RuntimeEventWire, RuntimeIdentityChallengeRequest,
    RuntimeIdentityChallengeResponse, SaveExportPresetRequest, SessionRequest, SessionResponse,
    SmallFileReadRequest, SmallFileReadResponse, SmallFileWriteRequest, SnapshotWire, SpaceWire,
    StartNativeExportRequest, StartNativeImportRequest, StorageCategoryWire, StorageClassWire,
    StreamChunkAck, StreamReadBeginRequest, StreamReadBeginResponse, StreamWriteBeginRequest,
    StreamWriteBeginResponse, StreamWriteCommitRequest, CONTROL_CONTENT_TYPE,
    DEFAULT_STREAM_CHUNK_BYTES, DIRECT_PAYLOAD_TARGET_BYTES, EVENT_BUFFER_CAPACITY,
    EVENT_LONG_POLL_MAX_MS, MAX_ACTIVE_BULK_REQUESTS, MAX_ACTIVE_REQUESTS, MAX_ACTIVE_SESSIONS,
    MAX_BATCH_OPERATIONS, MAX_BATCH_PAYLOAD_BYTES, MAX_BULK_REQUESTS_PER_SESSION,
    MAX_CONTROL_BODY_BYTES, MAX_HEADER_BYTES, MAX_MANAGED_FILE_BYTES, MAX_PENDING_PAIRINGS,
    MAX_STREAM_CHUNK_BYTES, MAX_STREAM_WRITES_PER_SESSION, PAIRING_TTL_MS, PROTOCOL_MAX,
    PROTOCOL_MIN, RUNTIME_ENDPOINT_PORTS, RUNTIME_IDENTITY_DOMAIN_SEPARATOR, SESSION_TTL_MS,
    STREAM_IDLE_TIMEOUT_MS,
};

const PAIRING_RATE_WINDOW_MS: i64 = 60_000;
const PAIRING_RATE_MAX: usize = 10;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
const SLEEP_RESUME_GAP_MS: i64 = 45_000;
const LIFECYCLE_RUNNING: u8 = 0;
const LIFECYCLE_SUSPENDED: u8 = 1;
const LIFECYCLE_DRAINING: u8 = 2;
const LIFECYCLE_STOPPED: u8 = 3;
const OPERATION_RETENTION_MS: i64 = 5 * 60 * 1000;
const OPERATION_DESKTOP_COMPLETION_HOLD_MS: i64 = 1_200;
const OPERATION_MAX_ROWS: usize = 2048;

#[derive(Debug, Default)]
pub struct RuntimeFoundation;

impl RuntimeFoundation {
    pub fn health(&self) -> HealthResponse {
        HealthResponse::foundation(env!("CARGO_PKG_VERSION"))
    }
}

#[derive(Debug)]
pub struct RuntimeStorage {
    engine: StorageEngine,
}

impl RuntimeStorage {
    pub fn open(root: impl AsRef<Path>) -> StorageResult<Self> {
        Ok(Self {
            engine: StorageEngine::open(root)?,
        })
    }

    pub fn engine(&self) -> &StorageEngine {
        &self.engine
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairingSummary {
    pub request_id: String,
    pub application_id: String,
    pub application: ClientIdentity,
    pub client_instance_id: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingApproval {
    pub persisted_pairing_id: String,
}

#[derive(Debug)]
pub enum RuntimeAdminError {
    NotFound,
    Expired,
    AlreadyDecided,
    Storage(StorageError),
    Internal(String),
}

impl fmt::Display for RuntimeAdminError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "pending pairing not found"),
            Self::Expired => write!(f, "pending pairing expired"),
            Self::AlreadyDecided => write!(f, "pending pairing is already decided"),
            Self::Storage(error) => write!(f, "storage error: {error}"),
            Self::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for RuntimeAdminError {}
impl From<StorageError> for RuntimeAdminError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

#[derive(Debug)]
pub enum RuntimeBindError {
    Storage(StorageError),
    Io(std::io::Error),
}

impl fmt::Display for RuntimeBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(f, "storage initialization failed: {error}"),
            Self::Io(error) => write!(f, "runtime bind failed: {error}"),
        }
    }
}

impl std::error::Error for RuntimeBindError {}
impl From<StorageError> for RuntimeBindError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}
impl From<std::io::Error> for RuntimeBindError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeLifecycleStatus {
    Running,
    Suspended,
    Draining,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub automatic_update_check: bool,
    pub launch_on_login: bool,
}
impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            automatic_update_check: true,
            launch_on_login: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterConfiguration {
    pub automatic_check: bool,
    pub endpoint: Option<String>,
    pub public_key_configured: bool,
    pub release_transport_ready: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LongOperationStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LongOperationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOperationSummary {
    pub operation: LongOperationSnapshot,
    pub application_name: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LongOperationSnapshot {
    pub id: String,
    pub kind: String,
    pub phase: String,
    pub presentation: OperationPresentationWire,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub bytes_completed: Option<u64>,
    pub bytes_total: Option<u64>,
    pub items_completed: Option<u64>,
    pub items_total: Option<u64>,
    pub throughput_bytes_per_second: Option<u64>,
    pub eta_ms: Option<u64>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub cancellable: bool,
    pub status: LongOperationStatus,
    pub error: Option<LongOperationError>,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub format: String,
    pub generated_at_ms: i64,
    pub runtime_version: String,
    pub protocol_min: u16,
    pub protocol_max: u16,
    pub storage_format_version: u32,
    pub os: String,
    pub architecture: String,
    pub lifecycle: RuntimeLifecycleStatus,
    pub active_sessions: u64,
    pub pending_pairings: u64,
    pub endpoint: RuntimeEndpointStatus,
    pub storage: StorageDiagnostics,
    pub updater: UpdaterConfiguration,
    pub recent_sanitized_logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPairingSummary {
    pub id: String,
    pub client_instance_id: String,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
    pub active_sessions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDirectoryGrantSummary {
    pub id: String,
    pub label: String,
    pub capability: DirectoryGrantCapability,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopExportPresetSummary {
    pub id: String,
    pub name: String,
    pub destination_id: String,
    pub mode: NativeExportMode,
    pub conflict_policy: ExportConflictPolicy,
    pub source_path: String,
    pub archive_format: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApplicationSummary {
    pub application: ApplicationRecord,
    pub pairings: Vec<DesktopPairingSummary>,
    pub spaces: Vec<SpaceRecord>,
    pub destinations: Vec<DesktopDirectoryGrantSummary>,
    pub export_presets: Vec<DesktopExportPresetSummary>,
    pub active_sessions: u64,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    state: Arc<RuntimeState>,
}

impl RuntimeHandle {
    pub fn endpoint_status(&self) -> RuntimeEndpointStatus {
        self.state.endpoint_status.clone()
    }

    pub fn pending_pairings(&self) -> Vec<PendingPairingSummary> {
        self.state.prune_ephemeral();
        let now = now_ms();
        let pending = self
            .state
            .pending_pairings
            .lock()
            .expect("pending pairing mutex poisoned");
        let mut values = pending
            .values()
            .filter(|entry| {
                entry.expires_at_ms > now && matches!(&entry.decision, PairingDecision::Pending)
            })
            .map(PendingPairing::summary)
            .collect::<Vec<_>>();
        values.sort_by_key(|entry| (entry.created_at_ms, entry.request_id.clone()));
        values
    }

    pub fn approve_pairing(&self, request_id: &str) -> Result<PairingApproval, RuntimeAdminError> {
        self.state.prune_ephemeral();
        let mut pending =
            self.state.pending_pairings.lock().map_err(|_| {
                RuntimeAdminError::Internal("pending pairing mutex poisoned".into())
            })?;
        let entry = pending
            .get_mut(request_id)
            .ok_or(RuntimeAdminError::NotFound)?;
        if entry.expires_at_ms <= now_ms() {
            return Err(RuntimeAdminError::Expired);
        }
        if !matches!(&entry.decision, PairingDecision::Pending) {
            return Err(RuntimeAdminError::AlreadyDecided);
        }

        let credential = random_secret_hex(32);
        let credential_hash = sha256_hex(credential.as_bytes());
        let previous_pairings = self
            .state
            .storage
            .active_pairings_for_client(&entry.application_id, &entry.client_instance_id)?;
        let pairing = self.state.storage.create_pairing(
            &entry.application_id,
            &entry.client_instance_id,
            &credential_hash,
        )?;
        if !previous_pairings.is_empty() {
            let previous_ids = previous_pairings
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>();
            let mut sessions = self
                .state
                .sessions
                .lock()
                .map_err(|_| RuntimeAdminError::Internal("session mutex poisoned".into()))?;
            sessions.retain(|_, session| !previous_ids.iter().any(|id| id == &session.pairing_id));
        }
        entry.decision = PairingDecision::Approved {
            credential: Some(credential),
        };
        Ok(PairingApproval {
            persisted_pairing_id: pairing.id,
        })
    }

    pub fn deny_pairing(&self, request_id: &str) -> Result<(), RuntimeAdminError> {
        self.state.prune_ephemeral();
        let mut pending =
            self.state.pending_pairings.lock().map_err(|_| {
                RuntimeAdminError::Internal("pending pairing mutex poisoned".into())
            })?;
        let entry = pending
            .get_mut(request_id)
            .ok_or(RuntimeAdminError::NotFound)?;
        if entry.expires_at_ms <= now_ms() {
            return Err(RuntimeAdminError::Expired);
        }
        if !matches!(&entry.decision, PairingDecision::Pending) {
            return Err(RuntimeAdminError::AlreadyDecided);
        }
        entry.decision = PairingDecision::Denied;
        Ok(())
    }

    pub fn revoke_pairing(&self, persisted_pairing_id: &str) -> Result<bool, RuntimeAdminError> {
        let revoked = self.state.storage.revoke_pairing(persisted_pairing_id)?;
        if revoked {
            let revoked_tokens = {
                let mut sessions =
                    self.state.sessions.lock().map_err(|_| {
                        RuntimeAdminError::Internal("session mutex poisoned".into())
                    })?;
                let mut revoked_tokens = Vec::new();
                sessions.retain(|token, session| {
                    if session.pairing_id == persisted_pairing_id {
                        revoked_tokens.push((token.clone(), session.expires_at_ms));
                        false
                    } else {
                        true
                    }
                });
                revoked_tokens
            };
            if !revoked_tokens.is_empty() {
                let mut tombstones = self.state.revoked_sessions.lock().map_err(|_| {
                    RuntimeAdminError::Internal("revoked-session mutex poisoned".into())
                })?;
                tombstones.extend(revoked_tokens);
            }
        }
        Ok(revoked)
    }

    pub fn lifecycle_status(&self) -> RuntimeLifecycleStatus {
        self.state.lifecycle_status()
    }

    pub fn active_session_count(&self) -> usize {
        self.state.prune_ephemeral();
        self.state
            .sessions
            .lock()
            .map(|value| value.len())
            .unwrap_or(0)
    }

    pub fn desktop_applications(
        &self,
    ) -> Result<Vec<DesktopApplicationSummary>, RuntimeAdminError> {
        self.state.prune_ephemeral();
        let session_counts = {
            let sessions = self
                .state
                .sessions
                .lock()
                .map_err(|_| RuntimeAdminError::Internal("session mutex poisoned".into()))?;
            let mut by_pairing = HashMap::<String, u64>::new();
            for session in sessions.values() {
                *by_pairing.entry(session.pairing_id.clone()).or_default() += 1;
            }
            by_pairing
        };
        let spaces = self.state.storage.list_all_spaces()?;
        let mut result = Vec::new();
        for application in self.state.storage.list_applications()? {
            let app_spaces = spaces
                .iter()
                .filter(|space| space.owner_application_id == application.id)
                .cloned()
                .collect::<Vec<_>>();
            let pairings = self
                .state
                .storage
                .pairings_for_application(&application.id)?
                .into_iter()
                .map(|pairing| DesktopPairingSummary {
                    active_sessions: session_counts.get(&pairing.id).copied().unwrap_or(0),
                    id: pairing.id,
                    client_instance_id: pairing.client_instance_id,
                    created_at_ms: pairing.created_at_ms,
                    last_used_at_ms: pairing.last_used_at_ms,
                    revoked_at_ms: pairing.revoked_at_ms,
                })
                .collect::<Vec<_>>();
            let active_sessions = pairings.iter().map(|pairing| pairing.active_sessions).sum();
            let destinations = self
                .state
                .storage
                .list_directory_grants(&application.id)?
                .into_iter()
                .map(|grant| DesktopDirectoryGrantSummary {
                    id: grant.id,
                    label: grant.label,
                    capability: grant.capability,
                    created_at_ms: grant.created_at_ms,
                    last_used_at_ms: grant.last_used_at_ms,
                })
                .collect::<Vec<_>>();
            let export_presets = self
                .state
                .storage
                .list_export_presets(&application.id)?
                .into_iter()
                .map(|preset| DesktopExportPresetSummary {
                    id: preset.id,
                    name: preset.name,
                    destination_id: preset.destination_grant_id,
                    mode: preset.mode,
                    conflict_policy: preset.conflict_policy,
                    source_path: preset.source_path,
                    archive_format: preset.archive_format,
                    created_at_ms: preset.created_at_ms,
                    updated_at_ms: preset.updated_at_ms,
                })
                .collect::<Vec<_>>();
            result.push(DesktopApplicationSummary {
                application,
                pairings,
                spaces: app_spaces,
                destinations,
                export_presets,
                active_sessions,
            });
        }
        result.sort_by(|a, b| {
            a.application
                .display_name
                .to_lowercase()
                .cmp(&b.application.display_name.to_lowercase())
                .then_with(|| a.application.id.cmp(&b.application.id))
        });
        Ok(result)
    }

    pub fn revoke_desktop_destination(
        &self,
        application_id: &str,
        destination_id: &str,
    ) -> Result<bool, RuntimeAdminError> {
        if self
            .state
            .storage
            .get_application(application_id)?
            .is_none()
        {
            return Err(RuntimeAdminError::NotFound);
        }
        Ok(self
            .state
            .storage
            .revoke_directory_grant(application_id, destination_id)?)
    }

    pub fn delete_desktop_export_preset(
        &self,
        application_id: &str,
        preset_id: &str,
    ) -> Result<bool, RuntimeAdminError> {
        if self
            .state
            .storage
            .get_application(application_id)?
            .is_none()
        {
            return Err(RuntimeAdminError::NotFound);
        }
        Ok(self
            .state
            .storage
            .delete_export_preset(application_id, preset_id)?)
    }

    pub fn space_data_path(&self, space_id: &str) -> Result<PathBuf, RuntimeAdminError> {
        self.state
            .storage
            .space_data_path(space_id)
            .map_err(Into::into)
    }

    pub fn desktop_preferences(&self) -> Result<DesktopPreferences, RuntimeAdminError> {
        self.state
            .preferences
            .lock()
            .map(|value| value.clone())
            .map_err(|_| RuntimeAdminError::Internal("preferences mutex poisoned".into()))
    }

    pub fn set_automatic_update_check(
        &self,
        enabled: bool,
    ) -> Result<DesktopPreferences, RuntimeAdminError> {
        self.state
            .update_preferences(|preferences| preferences.automatic_update_check = enabled)
    }

    pub fn set_launch_on_login_preference(
        &self,
        enabled: bool,
    ) -> Result<DesktopPreferences, RuntimeAdminError> {
        self.state
            .update_preferences(|preferences| preferences.launch_on_login = enabled)
    }

    pub fn updater_configuration(&self) -> Result<UpdaterConfiguration, RuntimeAdminError> {
        let preferences = self.desktop_preferences()?;
        let endpoint = configured_value(
            "VONTAQFS_UPDATE_ENDPOINT",
            option_env!("VONTAQFS_UPDATE_ENDPOINT"),
        );
        let public_key_configured = configured_value(
            "VONTAQFS_UPDATER_PUBLIC_KEY",
            option_env!("VONTAQFS_UPDATER_PUBLIC_KEY"),
        )
        .is_some();
        let endpoint_is_https = endpoint
            .as_ref()
            .is_some_and(|value| value.starts_with("https://"));
        Ok(UpdaterConfiguration {
            automatic_check: preferences.automatic_update_check,
            release_transport_ready: endpoint_is_https && public_key_configured,
            endpoint,
            public_key_configured,
        })
    }

    pub fn suspend(&self) -> Result<(), RuntimeAdminError> {
        if self
            .state
            .lifecycle
            .compare_exchange(
                LIFECYCLE_RUNNING,
                LIFECYCLE_SUSPENDED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            self.state.invalidate_sessions_and_streams();
            self.state.audit_log("runtime-suspended", "ok");
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), RuntimeAdminError> {
        if self
            .state
            .lifecycle
            .compare_exchange(
                LIFECYCLE_SUSPENDED,
                LIFECYCLE_RUNNING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            self.state.invalidate_sessions_and_streams();
            self.state.audit_log("runtime-resumed", "ok");
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), RuntimeAdminError> {
        self.state.begin_shutdown();
        let deadline = tokio::time::Instant::now() + SHUTDOWN_GRACE + Duration::from_secs(1);
        while self.state.lifecycle.load(Ordering::SeqCst) != LIFECYCLE_STOPPED {
            if tokio::time::Instant::now() >= deadline {
                return Err(RuntimeAdminError::Internal(
                    "runtime shutdown timed out".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    }

    pub fn diagnostics_report(&self) -> Result<DiagnosticsReport, RuntimeAdminError> {
        let storage = self.state.storage.diagnostics_summary()?;
        let updater = self.updater_configuration()?;
        Ok(DiagnosticsReport {
            format: "vontaqfs-diagnostics".into(),
            generated_at_ms: now_ms(),
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            protocol_min: PROTOCOL_MIN,
            protocol_max: PROTOCOL_MAX,
            storage_format_version: vontaqfs_protocol::STORAGE_FORMAT_VERSION,
            os: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            lifecycle: self.lifecycle_status(),
            active_sessions: self.active_session_count() as u64,
            pending_pairings: self.pending_pairings().len() as u64,
            endpoint: self.endpoint_status(),
            storage,
            updater,
            recent_sanitized_logs: self.state.read_recent_logs(200),
        })
    }

    pub fn export_diagnostics(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<PathBuf, RuntimeAdminError> {
        let destination = destination.as_ref();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
        }
        let bytes = serde_json::to_vec_pretty(&self.diagnostics_report()?)
            .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
        let temp = destination.with_extension("json.tmp");
        {
            let mut file = File::create(&temp)
                .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
            file.write_all(&bytes)
                .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
            file.sync_all()
                .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
        }
        std::fs::rename(&temp, destination)
            .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
        self.state.audit_log("diagnostics-exported", "ok");
        Ok(destination.to_path_buf())
    }

    pub fn start_export_space(
        &self,
        space_id: String,
        destination: PathBuf,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        let (snapshot, cancel) = self
            .state
            .begin_operation("export-space", "preparing", true)?;
        let operation_id = snapshot.id.clone();
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            state.update_operation(
                &operation_id,
                "exporting",
                Some(0),
                None,
                LongOperationStatus::Running,
                None,
                None,
            );
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let result = tokio::task::spawn_blocking(move || {
                storage.export_space_zip(
                    &space_id,
                    &destination,
                    || cancel_for_work.load(Ordering::SeqCst),
                    |done, total| {
                        state_for_progress.update_operation(
                            &op_for_progress,
                            "exporting",
                            Some(done),
                            total,
                            LongOperationStatus::Running,
                            None,
                            None,
                        );
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(report).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
        });
        Ok(snapshot)
    }

    pub fn start_backup_space(
        &self,
        space_id: String,
        destination: PathBuf,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams; backup requires a stable space view".into(),
            ));
        }
        let (snapshot, cancel) = self
            .state
            .begin_operation("backup-space", "preparing", true)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let work_space_id = space_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                storage.create_portable_backup(
                    &work_space_id,
                    &destination,
                    || cancel_for_work.load(Ordering::SeqCst),
                    |items, total_items, bytes, total_bytes| {
                        state_for_progress.update_operation_metrics(
                            &op_for_progress,
                            "backing-up",
                            items,
                            total_items,
                            bytes,
                            total_bytes,
                        )
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(report).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn start_restore_backup(
        &self,
        source: PathBuf,
        replace_existing: bool,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        let maintenance_id = format!("restore-backup-{}", Uuid::new_v4());
        self.state.lock_global_maintenance(&maintenance_id)?;
        if self.active_session_count() != 0 {
            self.state.unlock_global_maintenance(&maintenance_id);
            return Err(RuntimeAdminError::Internal(
                "portable restore requires all client sessions to be disconnected".into(),
            ));
        }
        if self.state.has_active_long_operations() {
            self.state.unlock_global_maintenance(&maintenance_id);
            return Err(RuntimeAdminError::Internal(
                "portable restore requires other long-running storage operations to finish first"
                    .into(),
            ));
        }
        let (snapshot, cancel) =
            match self
                .state
                .begin_maintenance_operation("restore-backup", "validating", true)
            {
                Ok(value) => value,
                Err(error) => {
                    self.state.unlock_global_maintenance(&maintenance_id);
                    return Err(error);
                }
            };
        let operation_id = snapshot.id.clone();
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let conflict = if replace_existing {
                RestoreConflictPolicy::Replace
            } else {
                RestoreConflictPolicy::Fail
            };
            let result = tokio::task::spawn_blocking(move || {
                storage.restore_portable_backup(
                    &source,
                    conflict,
                    || cancel_for_work.load(Ordering::SeqCst),
                    |phase, items, total_items, bytes, total_bytes, cancellable| {
                        state_for_progress.set_operation_cancellable(&op_for_progress, cancellable);
                        state_for_progress.update_operation_metrics(
                            &op_for_progress,
                            phase,
                            items,
                            total_items,
                            bytes,
                            total_bytes,
                        );
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(report).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
            state.unlock_global_maintenance(&maintenance_id);
        });
        Ok(snapshot)
    }

    pub fn list_space_snapshots(
        &self,
        space_id: String,
    ) -> Result<Vec<SnapshotRecord>, RuntimeAdminError> {
        Ok(self.state.storage.list_snapshots(&space_id)?)
    }

    pub fn start_create_snapshot(
        &self,
        space_id: String,
        label: Option<String>,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams; snapshot requires a stable space view".into(),
            ));
        }
        let (snapshot, cancel) =
            self.state
                .begin_operation("snapshot-create", "snapshotting", true)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let work_space_id = space_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                storage.create_snapshot(
                    &work_space_id,
                    label.as_deref(),
                    || cancel_for_work.load(Ordering::SeqCst),
                    |items, total_items, bytes, total_bytes| {
                        state_for_progress.update_operation_metrics(
                            &op_for_progress,
                            "snapshotting",
                            items,
                            total_items,
                            bytes,
                            total_bytes,
                        )
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(record)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(record).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn start_restore_snapshot(
        &self,
        space_id: String,
        snapshot_id: String,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_snapshot(&space_id, &snapshot_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams; snapshot restore requires exclusive access".into(),
            ));
        }
        let (snapshot, cancel) = self
            .state
            .begin_operation("snapshot-restore", "staging", true)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let work_space_id = space_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                storage.restore_snapshot(
                    &work_space_id,
                    &snapshot_id,
                    || cancel_for_work.load(Ordering::SeqCst),
                    |phase, items, total_items, bytes, total_bytes, cancellable| {
                        state_for_progress.set_operation_cancellable(&op_for_progress, cancellable);
                        state_for_progress.update_operation_metrics(
                            &op_for_progress,
                            phase,
                            items,
                            total_items,
                            bytes,
                            total_bytes,
                        );
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(report).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn delete_space_snapshot(
        &self,
        space_id: String,
        snapshot_id: String,
    ) -> Result<bool, RuntimeAdminError> {
        Ok(self
            .state
            .storage
            .delete_snapshot(&space_id, &snapshot_id)?)
    }

    pub fn start_repair_space(
        &self,
        space_id: String,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams; repair requires exclusive access".into(),
            ));
        }
        let (snapshot, cancel) = self.state.begin_operation("repair-space", "verify", true)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            state.update_operation(
                &operation_id,
                "verify",
                Some(0),
                None,
                LongOperationStatus::Running,
                None,
                None,
            );
            let storage = Arc::clone(&state.storage);
            let op_for_progress = operation_id.clone();
            let state_for_progress = Arc::clone(&state);
            let cancel_for_work = Arc::clone(&cancel);
            let work_space_id = space_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                storage.repair_space(
                    &work_space_id,
                    || cancel_for_work.load(Ordering::SeqCst),
                    |phase, done, total, cancellable| {
                        state_for_progress.set_operation_cancellable(&op_for_progress, cancellable);
                        state_for_progress.update_operation(
                            &op_for_progress,
                            phase,
                            Some(done),
                            total,
                            LongOperationStatus::Running,
                            None,
                            None,
                        );
                    },
                )
            })
            .await;
            match result {
                Ok(Ok(report)) => {
                    let phase = if matches!(
                        report.outcome,
                        vontaqfs_core::RepairOutcome::DestructiveRecoveryRequired
                    ) {
                        "destructive-recovery-required"
                    } else {
                        "complete"
                    };
                    state.complete_operation(
                        &operation_id,
                        phase,
                        serde_json::to_value(report).ok(),
                    );
                }
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn start_reconcile_usage(
        &self,
        space_id: String,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        let (snapshot, cancel) = self
            .state
            .begin_operation("reconcile-usage", "scanning", true)?;
        let operation_id = snapshot.id.clone();
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let cancel_for_work = Arc::clone(&cancel);
            let result = tokio::task::spawn_blocking(move || {
                storage.reconcile_space_usage(&space_id, || cancel_for_work.load(Ordering::SeqCst))
            })
            .await;
            match result {
                Ok(Ok(space)) => state.complete_operation(
                    &operation_id,
                    "complete",
                    serde_json::to_value(space).ok(),
                ),
                Ok(Err(StorageError::OperationCancelled)) => {
                    state.cancelled_operation(&operation_id)
                }
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => {
                    state.fail_operation(&operation_id, format!("operation worker failed: {error}"))
                }
            }
        });
        Ok(snapshot)
    }

    pub fn start_clear_cache_space(
        &self,
        space_id: String,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        let space = self
            .state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if space.storage_class != StorageClass::Cache {
            return Err(RuntimeAdminError::Internal(
                "clear cache is only valid for cache spaces".into(),
            ));
        }
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams".into(),
            ));
        }
        let (snapshot, _cancel) =
            self.state
                .begin_operation("clear-cache", "deleting-cache", false)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let work_space_id = space_id.clone();
            let result =
                tokio::task::spawn_blocking(move || storage.clear_cache_space(&work_space_id))
                    .await;
            match result { Ok(Ok((bytes,files)))=>state.complete_operation(&operation_id,"complete",Some(serde_json::json!({"spaceId":space_id.clone(),"removedBytes":bytes,"removedFiles":files}))), Ok(Err(error))=>state.fail_operation(&operation_id,error.to_string()), Err(error)=>state.fail_operation(&operation_id,format!("operation worker failed: {error}")), }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn start_delete_space(
        &self,
        space_id: String,
    ) -> Result<LongOperationSnapshot, RuntimeAdminError> {
        self.state
            .storage
            .get_space(&space_id)?
            .ok_or(RuntimeAdminError::NotFound)?;
        if self.state.has_active_stream_for_space(&space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage has active streams".into(),
            ));
        }
        let (snapshot, _cancel) =
            self.state
                .begin_operation("delete-space", "deleting-storage", false)?;
        let operation_id = snapshot.id.clone();
        self.state.lock_space_exclusive(&space_id, &operation_id)?;
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            let storage = Arc::clone(&state.storage);
            let work_space_id = space_id.clone();
            let result =
                tokio::task::spawn_blocking(move || storage.delete_space(&work_space_id)).await;
            match result {
                Ok(Ok((bytes, files))) => state.complete_operation(&operation_id, "complete", Some(serde_json::json!({"spaceId":space_id.clone(),"removedBytes":bytes,"removedFiles":files}))),
                Ok(Err(error)) => state.fail_operation(&operation_id, error.to_string()),
                Err(error) => state.fail_operation(&operation_id, format!("operation worker failed: {error}")),
            }
            state.unlock_space_exclusive(&space_id, &operation_id);
        });
        Ok(snapshot)
    }

    pub fn desktop_operations(&self) -> Result<Vec<DesktopOperationSummary>, RuntimeAdminError> {
        self.state.prune_operations();
        let now = now_ms();
        let visible = {
            let operations = self
                .state
                .operations
                .lock()
                .map_err(|_| RuntimeAdminError::Internal("operation mutex poisoned".into()))?;
            operations
                .values()
                .filter(|entry| {
                    if entry.desktop_dismissed
                        || entry.snapshot.presentation != OperationPresentationWire::Vontaqfs
                    {
                        return false;
                    }
                    matches!(
                        entry.snapshot.status,
                        LongOperationStatus::Queued
                            | LongOperationStatus::Running
                            | LongOperationStatus::Cancelling
                            | LongOperationStatus::Failed
                    ) || entry.snapshot.updated_at_ms
                        >= now.saturating_sub(OPERATION_DESKTOP_COMPLETION_HOLD_MS)
                })
                .map(|entry| (entry.snapshot.clone(), entry.owner_application_id.clone()))
                .collect::<Vec<_>>()
        };
        let applications = self
            .state
            .storage
            .list_applications()?
            .into_iter()
            .map(|application| (application.id, application.display_name))
            .collect::<HashMap<_, _>>();
        let mut rows = visible
            .into_iter()
            .map(|(operation, owner_application_id)| {
                let application_name = owner_application_id
                    .as_ref()
                    .and_then(|id| applications.get(id))
                    .cloned()
                    .unwrap_or_else(|| "VontaqFS".into());
                DesktopOperationSummary {
                    label: operation_display_label(&operation.kind, &application_name),
                    application_name,
                    operation,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| (row.operation.started_at_ms, row.operation.id.clone()));
        Ok(rows)
    }

    pub fn dismiss_desktop_operation(&self, operation_id: &str) -> Result<bool, RuntimeAdminError> {
        let mut operations = self
            .state
            .operations
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("operation mutex poisoned".into()))?;
        let Some(entry) = operations.get_mut(operation_id) else {
            return Ok(false);
        };
        if !matches!(
            entry.snapshot.status,
            LongOperationStatus::Completed
                | LongOperationStatus::Failed
                | LongOperationStatus::Cancelled
        ) {
            return Ok(false);
        }
        entry.desktop_dismissed = true;
        Ok(true)
    }

    pub fn operation_status(
        &self,
        operation_id: &str,
    ) -> Result<Option<LongOperationSnapshot>, RuntimeAdminError> {
        self.state
            .operations
            .lock()
            .map(|operations| {
                operations
                    .get(operation_id)
                    .map(|entry| entry.snapshot.clone())
            })
            .map_err(|_| RuntimeAdminError::Internal("operation mutex poisoned".into()))
    }

    pub fn cancel_operation(&self, operation_id: &str) -> Result<bool, RuntimeAdminError> {
        let mut operations = self
            .state
            .operations
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("operation mutex poisoned".into()))?;
        let Some(entry) = operations.get_mut(operation_id) else {
            return Ok(false);
        };
        if !entry.snapshot.cancellable
            || !matches!(
                entry.snapshot.status,
                LongOperationStatus::Queued
                    | LongOperationStatus::Running
                    | LongOperationStatus::Cancelling
            )
        {
            return Ok(false);
        }
        entry.cancel.store(true, Ordering::SeqCst);
        entry.snapshot.status = LongOperationStatus::Cancelling;
        entry.snapshot.phase = "cancelling".into();
        entry.snapshot.updated_at_ms = now_ms();
        Ok(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEndpointStatus {
    pub selected_port: Option<u16>,
    pub occupied_ports: Vec<u16>,
    pub official_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEndpointSelection {
    version: u16,
    port: u16,
}

pub trait RuntimeHostServices: Send + Sync {
    fn choose_directory(
        &self,
        application_name: &str,
        purpose: &str,
    ) -> Result<Option<PathBuf>, String>;
    fn choose_files(
        &self,
        application_name: &str,
        purpose: &str,
        multiple: bool,
    ) -> Result<Option<Vec<PathBuf>>, String>;
    fn confirm_export_replace(
        &self,
        application_name: &str,
        item_label: &str,
    ) -> Result<bool, String>;
    fn confirm_import_replace(
        &self,
        application_name: &str,
        item_label: &str,
    ) -> Result<bool, String>;
}

#[derive(Default)]
pub struct HeadlessHostServices;
impl RuntimeHostServices for HeadlessHostServices {
    fn choose_directory(
        &self,
        _application_name: &str,
        _purpose: &str,
    ) -> Result<Option<PathBuf>, String> {
        Err(
            "DESTINATION_GRANT_REQUIRED: native directory picker is unavailable in this host"
                .into(),
        )
    }
    fn choose_files(
        &self,
        _application_name: &str,
        _purpose: &str,
        _multiple: bool,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        Err("DESTINATION_GRANT_REQUIRED: native file picker is unavailable in this host".into())
    }
    fn confirm_export_replace(
        &self,
        _application_name: &str,
        _item_label: &str,
    ) -> Result<bool, String> {
        Err("EXPORT_CONFLICT: native conflict confirmation is unavailable in this host".into())
    }
    fn confirm_import_replace(
        &self,
        _application_name: &str,
        _item_label: &str,
    ) -> Result<bool, String> {
        Err("CONFLICT: native import conflict confirmation is unavailable in this host".into())
    }
}

pub struct RuntimeServer {
    listeners: Vec<TcpListener>,
    state: Arc<RuntimeState>,
}

impl RuntimeServer {
    pub async fn bind(root: impl AsRef<Path>) -> Result<Self, RuntimeBindError> {
        Self::bind_with_host_services(root, Arc::new(HeadlessHostServices)).await
    }

    pub async fn bind_with_host_services(
        root: impl AsRef<Path>,
        host_services: Arc<dyn RuntimeHostServices>,
    ) -> Result<Self, RuntimeBindError> {
        let root = root.as_ref().to_path_buf();
        let storage = StorageEngine::open(&root)?;
        let mut occupied_ports = Vec::new();
        let mut selected_port = None;
        let mut listeners = Vec::new();
        for port in endpoint_candidate_order(&root) {
            match bind_official_endpoint(port).await? {
                Some(bound) => {
                    selected_port = Some(port);
                    listeners = bound;
                    persist_selected_endpoint(&root, port)?;
                    break;
                }
                None => occupied_ports.push(port),
            }
        }
        let endpoint_status = RuntimeEndpointStatus {
            selected_port,
            occupied_ports,
            official_ports: RUNTIME_ENDPOINT_PORTS.to_vec(),
        };
        Ok(Self {
            listeners,
            state: Arc::new(RuntimeState::new(
                storage,
                root,
                endpoint_status,
                host_services,
            )),
        })
    }

    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle {
            state: Arc::clone(&self.state),
        }
    }

    pub async fn serve(self) -> Result<(), std::io::Error> {
        self.state.audit_log("runtime-started", "ok");
        let cleanup_state = Arc::clone(&self.state);
        let cleanup = tokio::spawn(async move {
            let mut last_tick_ms = now_ms();
            loop {
                if cleanup_state.shutdown_requested.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    _ = cleanup_state.shutdown_notify.notified() => break,
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {
                        let current_tick_ms = now_ms();
                        if current_tick_ms.saturating_sub(last_tick_ms) >= SLEEP_RESUME_GAP_MS {
                            cleanup_state.handle_detected_resume();
                        }
                        last_tick_ms = current_tick_ms;
                        cleanup_state.prune_ephemeral();
                    },
                }
            }
        });
        let mut acceptors = Vec::new();
        for listener in self.listeners {
            let state = Arc::clone(&self.state);
            acceptors.push(tokio::spawn(async move {
                loop {
                    if state.shutdown_requested.load(Ordering::SeqCst) {
                        break;
                    }
                    let accepted = tokio::select! {
                        _ = state.shutdown_notify.notified() => break,
                        _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
                        accepted = listener.accept() => accepted,
                    };
                    let Ok((stream, peer)) = accepted else {
                        break;
                    };
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    let Ok(permit) = Arc::clone(&state.request_slots).acquire_owned().await else {
                        break;
                    };
                    let state = Arc::clone(&state);
                    tokio::spawn(async move {
                        let _permit = permit;
                        let _ = serve_connection(stream, state).await;
                    });
                }
            }));
        }
        while !self.state.shutdown_requested.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.state.shutdown_notify.notify_waiters();
        for acceptor in acceptors {
            let _ = acceptor.await;
        }
        let _ = cleanup.await;
        let deadline = tokio::time::Instant::now() + SHUTDOWN_GRACE;
        while (self.state.request_slots.available_permits() < MAX_ACTIVE_REQUESTS
            || self.state.has_active_long_operations())
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.state.invalidate_sessions_and_streams();
        self.state
            .lifecycle
            .store(LIFECYCLE_STOPPED, Ordering::SeqCst);
        self.state.audit_log("runtime-stopped", "ok");
        Ok(())
    }
}

async fn bind_official_endpoint(port: u16) -> Result<Option<Vec<TcpListener>>, RuntimeBindError> {
    if !RUNTIME_ENDPOINT_PORTS.contains(&port) {
        return Err(RuntimeBindError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "endpoint is not in the official VontaqFS set",
        )));
    }
    let v4_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let v4 = match TcpListener::bind(v4_addr).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => return Ok(None),
        Err(error) => return Err(RuntimeBindError::Io(error)),
    };
    let mut listeners = vec![v4];
    let v6_addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port);
    match TcpListener::bind(v6_addr).await {
        Ok(listener) => listeners.push(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => return Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
            ) => {}
        Err(error) => return Err(RuntimeBindError::Io(error)),
    }
    Ok(Some(listeners))
}

fn endpoint_candidate_order(root: &Path) -> Vec<u16> {
    let persisted = load_selected_endpoint(root);
    let mut ports = Vec::with_capacity(RUNTIME_ENDPOINT_PORTS.len());
    if let Some(port) = persisted.filter(|port| RUNTIME_ENDPOINT_PORTS.contains(port)) {
        ports.push(port);
    }
    for port in RUNTIME_ENDPOINT_PORTS {
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports
}

fn selected_endpoint_path(root: &Path) -> PathBuf {
    root.join("runtime/endpoint.json")
}

fn load_selected_endpoint(root: &Path) -> Option<u16> {
    let bytes = std::fs::read(selected_endpoint_path(root)).ok()?;
    let value = serde_json::from_slice::<PersistedEndpointSelection>(&bytes).ok()?;
    (value.version == 1 && RUNTIME_ENDPOINT_PORTS.contains(&value.port)).then_some(value.port)
}

fn persist_selected_endpoint(root: &Path, port: u16) -> Result<(), RuntimeBindError> {
    if !RUNTIME_ENDPOINT_PORTS.contains(&port) {
        return Err(RuntimeBindError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to persist non-official VontaqFS endpoint",
        )));
    }
    let path = selected_endpoint_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let body = format!("{{\n  \"version\": 1,\n  \"port\": {port}\n}}\n");
    std::fs::write(&temp, body.as_bytes())?;
    std::fs::rename(temp, path)?;
    Ok(())
}

struct RuntimeState {
    storage: Arc<StorageEngine>,
    pending_pairings: Mutex<HashMap<String, PendingPairing>>,
    sessions: Mutex<HashMap<String, SessionState>>,
    revoked_sessions: Mutex<HashMap<String, i64>>,
    pairing_rate: Mutex<HashMap<String, Vec<i64>>>,
    request_slots: Arc<Semaphore>,
    bulk_slots: Arc<Semaphore>,
    pairing_bulk_slots: Mutex<HashMap<String, Arc<Semaphore>>>,
    write_streams: Mutex<HashMap<String, WriteStreamState>>,
    read_streams: Mutex<HashMap<String, ReadStreamState>>,
    active_write_paths: Mutex<HashMap<String, String>>,
    events: Mutex<VecDeque<BufferedEvent>>,
    event_sequence: AtomicU64,
    event_notify: Notify,
    lifecycle: AtomicU8,
    shutdown_requested: AtomicBool,
    shutdown_notify: Notify,
    preferences: Mutex<DesktopPreferences>,
    preferences_path: PathBuf,
    log_path: PathBuf,
    operations: Mutex<HashMap<String, LongOperationEntry>>,
    exclusive_spaces: Mutex<HashMap<String, String>>,
    global_maintenance: Mutex<Option<String>>,
    endpoint_status: RuntimeEndpointStatus,
    host_services: Arc<dyn RuntimeHostServices>,
}

impl RuntimeState {
    fn new(
        storage: StorageEngine,
        root: PathBuf,
        endpoint_status: RuntimeEndpointStatus,
        host_services: Arc<dyn RuntimeHostServices>,
    ) -> Self {
        Self {
            storage: Arc::new(storage),
            pending_pairings: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            revoked_sessions: Mutex::new(HashMap::new()),
            pairing_rate: Mutex::new(HashMap::new()),
            request_slots: Arc::new(Semaphore::new(MAX_ACTIVE_REQUESTS)),
            bulk_slots: Arc::new(Semaphore::new(MAX_ACTIVE_BULK_REQUESTS)),
            pairing_bulk_slots: Mutex::new(HashMap::new()),
            write_streams: Mutex::new(HashMap::new()),
            read_streams: Mutex::new(HashMap::new()),
            active_write_paths: Mutex::new(HashMap::new()),
            events: Mutex::new(VecDeque::new()),
            event_sequence: AtomicU64::new(0),
            event_notify: Notify::new(),
            lifecycle: AtomicU8::new(LIFECYCLE_RUNNING),
            shutdown_requested: AtomicBool::new(false),
            shutdown_notify: Notify::new(),
            preferences: Mutex::new(load_or_initialize_preferences(
                &root.join("runtime/preferences.json"),
            )),
            preferences_path: root.join("runtime/preferences.json"),
            log_path: root.join("runtime/logs/runtime.log"),
            operations: Mutex::new(HashMap::new()),
            exclusive_spaces: Mutex::new(HashMap::new()),
            global_maintenance: Mutex::new(None),
            endpoint_status,
            host_services,
        }
    }

    fn lifecycle_status(&self) -> RuntimeLifecycleStatus {
        match self.lifecycle.load(Ordering::SeqCst) {
            LIFECYCLE_SUSPENDED => RuntimeLifecycleStatus::Suspended,
            LIFECYCLE_DRAINING => RuntimeLifecycleStatus::Draining,
            LIFECYCLE_STOPPED => RuntimeLifecycleStatus::Stopped,
            _ => RuntimeLifecycleStatus::Running,
        }
    }

    fn health(&self) -> HealthResponse {
        let mut health = HealthResponse::ready(env!("CARGO_PKG_VERSION"));
        health.status = if self.endpoint_status.selected_port.is_none() {
            "port-conflict".into()
        } else {
            match self.lifecycle_status() {
                RuntimeLifecycleStatus::Running => "ready",
                RuntimeLifecycleStatus::Suspended => "suspended",
                RuntimeLifecycleStatus::Draining => "draining",
                RuntimeLifecycleStatus::Stopped => "stopped",
            }
            .into()
        };
        health
    }

    fn begin_shutdown(&self) {
        let current = self.lifecycle.load(Ordering::SeqCst);
        if current == LIFECYCLE_STOPPED || current == LIFECYCLE_DRAINING {
            return;
        }
        self.lifecycle.store(LIFECYCLE_DRAINING, Ordering::SeqCst);
        self.shutdown_requested.store(true, Ordering::SeqCst);
        // Ask cancellable privileged work to stop before the bounded drain.
        // Non-cancellable critical phases are allowed to finish inside SHUTDOWN_GRACE.
        if let Ok(operations) = self.operations.lock() {
            for entry in operations.values() {
                if entry.snapshot.cancellable
                    && matches!(
                        entry.snapshot.status,
                        LongOperationStatus::Queued
                            | LongOperationStatus::Running
                            | LongOperationStatus::Cancelling
                    )
                {
                    entry.cancel.store(true, Ordering::SeqCst);
                }
            }
        }
        self.emit_event(
            "*",
            EventKindWire::RuntimeShuttingDown,
            None,
            None,
            None,
            None,
            None,
        );
        self.audit_log("runtime-shutdown-requested", "ok");
        self.shutdown_notify.notify_waiters();
        self.event_notify.notify_waiters();
    }

    fn handle_detected_resume(&self) {
        if matches!(
            self.lifecycle_status(),
            RuntimeLifecycleStatus::Draining | RuntimeLifecycleStatus::Stopped
        ) {
            return;
        }
        self.invalidate_sessions_and_streams();
        self.lifecycle.store(LIFECYCLE_RUNNING, Ordering::SeqCst);
        self.audit_log("runtime-resumed-after-sleep-gap", "ok");
    }

    fn invalidate_sessions_and_streams(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
        if let Ok(mut revoked_sessions) = self.revoked_sessions.lock() {
            revoked_sessions.clear();
        }
        let mut expired = Vec::new();
        if let Ok(mut writes) = self.write_streams.lock() {
            expired.extend(writes.drain().map(|(_, stream)| stream));
        }
        if let Ok(mut paths) = self.active_write_paths.lock() {
            for mut stream in expired {
                paths.remove(&write_path_key(&stream.space_id, &stream.path));
                stream.file.take();
                if stream.committed.is_none() {
                    let _ = self
                        .storage
                        .abort_stream_temp(&stream.space_id, &stream.temp_path);
                }
            }
        }
        if let Ok(mut reads) = self.read_streams.lock() {
            reads.clear();
        }
    }

    fn update_preferences<F>(&self, mut update: F) -> Result<DesktopPreferences, RuntimeAdminError>
    where
        F: FnMut(&mut DesktopPreferences),
    {
        let mut guard = self
            .preferences
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("preferences mutex poisoned".into()))?;
        let mut next = guard.clone();
        update(&mut next);
        persist_preferences(&self.preferences_path, &next)
            .map_err(|error| RuntimeAdminError::Internal(error.to_string()))?;
        *guard = next.clone();
        Ok(next)
    }

    fn audit_log(&self, event: &str, result: &str) {
        if let Some(parent) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(
                file,
                "{} event={} result={}",
                now_ms(),
                sanitize_log_atom(event),
                sanitize_log_atom(result)
            );
        }
    }

    fn read_recent_logs(&self, limit: usize) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(&self.log_path) else {
            return Vec::new();
        };
        let lines = text
            .lines()
            .rev()
            .take(limit)
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        lines.into_iter().rev().collect()
    }

    fn begin_operation(
        &self,
        kind: &str,
        phase: &str,
        cancellable: bool,
    ) -> Result<(LongOperationSnapshot, Arc<AtomicBool>), RuntimeAdminError> {
        let maintenance = self
            .global_maintenance
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("global-maintenance mutex poisoned".into()))?;
        if maintenance.is_some() {
            return Err(RuntimeAdminError::Internal(
                "storage is temporarily locked by a privileged restore operation".into(),
            ));
        }
        let result = self.begin_maintenance_operation(kind, phase, cancellable);
        drop(maintenance);
        result
    }

    fn begin_maintenance_operation(
        &self,
        kind: &str,
        phase: &str,
        cancellable: bool,
    ) -> Result<(LongOperationSnapshot, Arc<AtomicBool>), RuntimeAdminError> {
        let id = Uuid::new_v4().to_string();
        self.insert_operation(
            id,
            None,
            OperationPresentationWire::Client,
            kind,
            phase,
            cancellable,
            None,
            None,
        )
        .map_err(|error| RuntimeAdminError::Internal(error.message))
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_application_operation(
        &self,
        application_id: &str,
        request: &OperationRequestWire,
        kind: &str,
        phase: &str,
        cancellable: bool,
        bytes_total: Option<u64>,
        items_total: Option<u64>,
    ) -> Result<(LongOperationSnapshot, Arc<AtomicBool>), ApiError> {
        let maintenance = self
            .global_maintenance
            .lock()
            .map_err(|_| ApiError::internal())?;
        if maintenance.is_some() {
            return Err(ApiError::new(
                503,
                ErrorCode::StorageUnavailable,
                "storage is temporarily locked by a privileged restore operation",
            ));
        }
        validate_operation_id(&request.id)?;
        let result = self.insert_operation(
            request.id.clone(),
            Some(application_id.to_owned()),
            request.presentation,
            kind,
            phase,
            cancellable,
            bytes_total,
            items_total,
        );
        drop(maintenance);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_operation(
        &self,
        id: String,
        owner_application_id: Option<String>,
        presentation: OperationPresentationWire,
        kind: &str,
        phase: &str,
        cancellable: bool,
        bytes_total: Option<u64>,
        items_total: Option<u64>,
    ) -> Result<(LongOperationSnapshot, Arc<AtomicBool>), ApiError> {
        self.prune_operations();
        let mut operations = self.operations.lock().map_err(|_| ApiError::internal())?;
        if let Some(existing) = operations.get(&id) {
            if existing.owner_application_id == owner_application_id
                && existing.snapshot.kind == kind
                && existing.snapshot.presentation == presentation
            {
                return Ok((existing.snapshot.clone(), Arc::clone(&existing.cancel)));
            }
            return Err(ApiError::new(
                409,
                ErrorCode::Conflict,
                "operation id is already bound to a different operation",
            ));
        }
        if operations.len() >= OPERATION_MAX_ROWS {
            return Err(ApiError::new(
                429,
                ErrorCode::RateLimited,
                "too many retained operations",
            ));
        }
        let now = now_ms();
        let (completed, total) = if bytes_total.is_some() {
            (Some(0), bytes_total)
        } else if items_total.is_some() {
            (Some(0), items_total)
        } else {
            (None, None)
        };
        let snapshot = LongOperationSnapshot {
            id: id.clone(),
            kind: kind.into(),
            phase: phase.into(),
            presentation,
            completed,
            total,
            bytes_completed: bytes_total.map(|_| 0),
            bytes_total,
            items_completed: items_total.map(|_| 0),
            items_total,
            throughput_bytes_per_second: None,
            eta_ms: None,
            started_at_ms: now,
            updated_at_ms: now,
            cancellable,
            status: LongOperationStatus::Queued,
            error: None,
            result: None,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        operations.insert(
            id,
            LongOperationEntry {
                snapshot: snapshot.clone(),
                cancel: Arc::clone(&cancel),
                owner_application_id,
                desktop_dismissed: false,
            },
        );
        Ok((snapshot, cancel))
    }

    #[allow(clippy::too_many_arguments)]
    fn update_operation(
        &self,
        id: &str,
        phase: &str,
        completed: Option<u64>,
        total: Option<u64>,
        status: LongOperationStatus,
        error: Option<LongOperationError>,
        result: Option<serde_json::Value>,
    ) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = phase.into();
                entry.snapshot.completed = completed;
                entry.snapshot.total = total;
                entry.snapshot.status = status;
                entry.snapshot.error = error;
                entry.snapshot.result = result;
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
    }

    fn update_operation_bytes(&self, id: &str, phase: &str, done: u64, total: Option<u64>) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = phase.into();
                entry.snapshot.status = LongOperationStatus::Running;
                entry.snapshot.completed = Some(done);
                entry.snapshot.total = total;
                entry.snapshot.bytes_completed = Some(done);
                entry.snapshot.bytes_total = total;
                entry.snapshot.updated_at_ms = now_ms();
                let elapsed_ms = entry
                    .snapshot
                    .updated_at_ms
                    .saturating_sub(entry.snapshot.started_at_ms)
                    .max(1) as u64;
                let throughput = done.saturating_mul(1000) / elapsed_ms;
                entry.snapshot.throughput_bytes_per_second = (throughput > 0).then_some(throughput);
                entry.snapshot.eta_ms = match (total, entry.snapshot.throughput_bytes_per_second) {
                    (Some(total), Some(rate)) if rate > 0 && total > done => {
                        Some(total.saturating_sub(done).saturating_mul(1000) / rate)
                    }
                    _ => None,
                };
            }
        }
    }

    fn update_operation_items(&self, id: &str, phase: &str, done: u64, total: Option<u64>) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = phase.into();
                entry.snapshot.status = LongOperationStatus::Running;
                entry.snapshot.completed = Some(done);
                entry.snapshot.total = total;
                entry.snapshot.items_completed = Some(done);
                entry.snapshot.items_total = total;
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
    }

    fn complete_operation(&self, id: &str, phase: &str, result: Option<serde_json::Value>) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = phase.into();
                entry.snapshot.status = LongOperationStatus::Completed;
                entry.snapshot.cancellable = false;
                entry.snapshot.error = None;
                entry.snapshot.result = result;
                entry.snapshot.updated_at_ms = now_ms();
                if let Some(total) = entry.snapshot.bytes_total {
                    entry.snapshot.bytes_completed = Some(total);
                    entry.snapshot.completed = Some(total);
                    entry.snapshot.total = Some(total);
                    entry.snapshot.eta_ms = Some(0);
                } else if let Some(total) = entry.snapshot.items_total {
                    entry.snapshot.items_completed = Some(total);
                    entry.snapshot.completed = Some(total);
                    entry.snapshot.total = Some(total);
                }
            }
        }
        self.audit_log("long-operation-completed", "ok");
    }
    fn cancelled_operation(&self, id: &str) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = "cancelled".into();
                entry.snapshot.status = LongOperationStatus::Cancelled;
                entry.snapshot.cancellable = false;
                entry.snapshot.error = None;
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
        self.audit_log("long-operation-cancelled", "ok");
    }
    fn fail_operation_code(&self, id: &str, code: &str, error: String) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = "failed".into();
                entry.snapshot.status = LongOperationStatus::Failed;
                entry.snapshot.cancellable = false;
                entry.snapshot.error = Some(LongOperationError {
                    code: code.into(),
                    message: sanitize_error_message(&error),
                });
                entry.snapshot.result = None;
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
        self.audit_log("long-operation-failed", "error");
    }
    fn fail_operation(&self, id: &str, error: String) {
        self.fail_operation_code(id, "INTERNAL_ERROR", error);
    }
    fn update_operation_metrics(
        &self,
        id: &str,
        phase: &str,
        items_completed: u64,
        items_total: Option<u64>,
        bytes_completed: u64,
        bytes_total: Option<u64>,
    ) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.phase = phase.into();
                entry.snapshot.status = LongOperationStatus::Running;
                entry.snapshot.items_completed = Some(items_completed);
                entry.snapshot.items_total = items_total;
                entry.snapshot.bytes_completed = Some(bytes_completed);
                entry.snapshot.bytes_total = bytes_total;
                entry.snapshot.completed = items_total
                    .map(|_| items_completed)
                    .or(bytes_total.map(|_| bytes_completed));
                entry.snapshot.total = items_total.or(bytes_total);
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
    }

    fn operation_status_for_application(
        &self,
        application_id: &str,
        operation_id: &str,
    ) -> Result<LongOperationSnapshot, ApiError> {
        validate_operation_id(operation_id)?;
        let operations = self.operations.lock().map_err(|_| ApiError::internal())?;
        let entry = operations.get(operation_id).ok_or_else(|| {
            ApiError::new(404, ErrorCode::OperationNotFound, "operation not found")
        })?;
        if entry.owner_application_id.as_deref() != Some(application_id) {
            return Err(ApiError::new(
                403,
                ErrorCode::OperationNotOwned,
                "operation belongs to another application",
            ));
        }
        Ok(entry.snapshot.clone())
    }
    fn cancel_application_operation(
        &self,
        application_id: &str,
        operation_id: &str,
    ) -> Result<LongOperationSnapshot, ApiError> {
        validate_operation_id(operation_id)?;
        let mut operations = self.operations.lock().map_err(|_| ApiError::internal())?;
        let entry = operations.get_mut(operation_id).ok_or_else(|| {
            ApiError::new(404, ErrorCode::OperationNotFound, "operation not found")
        })?;
        if entry.owner_application_id.as_deref() != Some(application_id) {
            return Err(ApiError::new(
                403,
                ErrorCode::OperationNotOwned,
                "operation belongs to another application",
            ));
        }
        if !entry.snapshot.cancellable
            || !matches!(
                entry.snapshot.status,
                LongOperationStatus::Queued
                    | LongOperationStatus::Running
                    | LongOperationStatus::Cancelling
            )
        {
            return Err(ApiError::new(
                409,
                ErrorCode::OperationNotCancellable,
                "operation is not cancellable",
            ));
        }
        entry.cancel.store(true, Ordering::SeqCst);
        entry.snapshot.status = LongOperationStatus::Cancelling;
        entry.snapshot.phase = "cancelling".into();
        entry.snapshot.updated_at_ms = now_ms();
        Ok(entry.snapshot.clone())
    }
    fn operation_cancelled(&self, id: &str) -> bool {
        self.operations
            .lock()
            .ok()
            .and_then(|operations| {
                operations
                    .get(id)
                    .map(|entry| entry.cancel.load(Ordering::SeqCst))
            })
            .unwrap_or(false)
    }
    fn cancel_operation_if_active(&self, id: &str) {
        let active = self
            .operations
            .lock()
            .ok()
            .and_then(|operations| {
                operations.get(id).map(|entry| {
                    matches!(
                        entry.snapshot.status,
                        LongOperationStatus::Queued
                            | LongOperationStatus::Running
                            | LongOperationStatus::Cancelling
                    )
                })
            })
            .unwrap_or(false);
        if active {
            self.cancelled_operation(id);
        }
    }
    #[allow(clippy::nonminimal_bool)]
    fn prune_operations(&self) {
        let now = now_ms();
        if let Ok(mut operations) = self.operations.lock() {
            operations.retain(|_, entry| {
                matches!(
                    entry.snapshot.status,
                    LongOperationStatus::Queued
                        | LongOperationStatus::Running
                        | LongOperationStatus::Cancelling
                ) || (matches!(entry.snapshot.status, LongOperationStatus::Failed)
                    && entry.snapshot.presentation == OperationPresentationWire::Vontaqfs
                    && !entry.desktop_dismissed)
                    || entry.snapshot.updated_at_ms > now - OPERATION_RETENTION_MS
            });
            if operations.len() >= OPERATION_MAX_ROWS {
                let mut terminal = operations
                    .iter()
                    .filter(|(_, entry)| {
                        !matches!(
                            entry.snapshot.status,
                            LongOperationStatus::Queued
                                | LongOperationStatus::Running
                                | LongOperationStatus::Cancelling
                        ) && !(matches!(entry.snapshot.status, LongOperationStatus::Failed)
                            && entry.snapshot.presentation == OperationPresentationWire::Vontaqfs
                            && !entry.desktop_dismissed)
                    })
                    .map(|(id, entry)| (id.clone(), entry.snapshot.updated_at_ms))
                    .collect::<Vec<_>>();
                terminal.sort_by_key(|(_, updated)| *updated);
                let remove_count = operations
                    .len()
                    .saturating_sub(OPERATION_MAX_ROWS)
                    .saturating_add(1);
                for (id, _) in terminal.into_iter().take(remove_count) {
                    operations.remove(&id);
                }
            }
        }
    }
    fn has_active_stream_for_space(&self, space_id: &str) -> bool {
        self.write_streams
            .lock()
            .map(|streams| streams.values().any(|s| s.space_id == space_id))
            .unwrap_or(true)
            || self
                .read_streams
                .lock()
                .map(|streams| streams.values().any(|s| s.space_id == space_id))
                .unwrap_or(true)
    }
    fn lock_space_exclusive(
        &self,
        space_id: &str,
        operation_id: &str,
    ) -> Result<(), RuntimeAdminError> {
        let mut locks = self
            .exclusive_spaces
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("exclusive-space mutex poisoned".into()))?;
        if locks.contains_key(space_id) {
            return Err(RuntimeAdminError::Internal(
                "storage already has an exclusive maintenance operation".into(),
            ));
        }
        locks.insert(space_id.into(), operation_id.into());
        Ok(())
    }
    fn unlock_space_exclusive(&self, space_id: &str, operation_id: &str) {
        if let Ok(mut locks) = self.exclusive_spaces.lock() {
            if locks
                .get(space_id)
                .is_some_and(|value| value == operation_id)
            {
                locks.remove(space_id);
            }
        }
    }
    fn lock_global_maintenance(&self, owner: &str) -> Result<(), RuntimeAdminError> {
        let mut lock = self
            .global_maintenance
            .lock()
            .map_err(|_| RuntimeAdminError::Internal("global-maintenance mutex poisoned".into()))?;
        if lock.is_some() {
            return Err(RuntimeAdminError::Internal(
                "storage already has a privileged global maintenance operation".into(),
            ));
        }
        *lock = Some(owner.into());
        Ok(())
    }
    fn unlock_global_maintenance(&self, owner: &str) {
        if let Ok(mut lock) = self.global_maintenance.lock() {
            if lock.as_deref() == Some(owner) {
                *lock = None;
            }
        }
    }
    fn set_operation_cancellable(&self, id: &str, cancellable: bool) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.get_mut(id) {
                entry.snapshot.cancellable = cancellable;
                entry.snapshot.updated_at_ms = now_ms();
            }
        }
    }
    fn has_active_long_operations(&self) -> bool {
        self.operations
            .lock()
            .map(|operations| {
                operations.values().any(|entry| {
                    matches!(
                        entry.snapshot.status,
                        LongOperationStatus::Queued
                            | LongOperationStatus::Running
                            | LongOperationStatus::Cancelling
                    )
                })
            })
            .unwrap_or(true)
    }

    fn prune_ephemeral(&self) {
        let now = now_ms();
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.retain(|_, session| session.expires_at_ms > now);
        }
        if let Ok(mut revoked_sessions) = self.revoked_sessions.lock() {
            revoked_sessions.retain(|_, expires_at_ms| *expires_at_ms > now);
        }
        if let Ok(mut pending) = self.pending_pairings.lock() {
            pending.retain(|_, entry| entry.expires_at_ms > now);
        }
        if let Ok(mut rate) = self.pairing_rate.lock() {
            rate.retain(|_, samples| {
                samples.retain(|at| *at > now - PAIRING_RATE_WINDOW_MS);
                !samples.is_empty()
            });
        }
        let mut expired_writes = Vec::new();
        if let Ok(mut writes) = self.write_streams.lock() {
            let ids = writes
                .iter()
                .filter(|(_, stream)| stream.last_activity_ms <= now - STREAM_IDLE_TIMEOUT_MS)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                if let Some(stream) = writes.remove(&id) {
                    expired_writes.push(stream);
                }
            }
        }
        if !expired_writes.is_empty() {
            if let Ok(mut paths) = self.active_write_paths.lock() {
                for mut stream in expired_writes {
                    paths.remove(&write_path_key(&stream.space_id, &stream.path));
                    stream.file.take();
                    if stream.committed.is_none() {
                        let _ = self
                            .storage
                            .abort_stream_temp(&stream.space_id, &stream.temp_path);
                    }
                    if let Some(operation_id) = stream.operation_id.as_deref() {
                        self.fail_operation_code(
                            operation_id,
                            "STREAM_EXPIRED",
                            "stream expired after being idle".into(),
                        );
                    }
                }
            }
        }
        let mut expired_read_operations = Vec::new();
        if let Ok(mut reads) = self.read_streams.lock() {
            let ids = reads
                .iter()
                .filter(|(_, stream)| stream.last_activity_ms <= now - STREAM_IDLE_TIMEOUT_MS)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                if let Some(stream) = reads.remove(&id) {
                    if let Some(operation_id) = stream.operation_id {
                        expired_read_operations.push(operation_id);
                    }
                }
            }
        }
        for operation_id in expired_read_operations {
            self.fail_operation_code(
                &operation_id,
                "STREAM_EXPIRED",
                "stream expired after being idle".into(),
            );
        }
    }

    async fn acquire_bulk(
        &self,
        pairing_id: &str,
    ) -> Result<
        (
            tokio::sync::OwnedSemaphorePermit,
            tokio::sync::OwnedSemaphorePermit,
        ),
        ApiError,
    > {
        let pairing_slots = {
            let mut slots = self
                .pairing_bulk_slots
                .lock()
                .map_err(|_| ApiError::internal())?;
            Arc::clone(
                slots
                    .entry(pairing_id.to_owned())
                    .or_insert_with(|| Arc::new(Semaphore::new(MAX_BULK_REQUESTS_PER_SESSION))),
            )
        };
        // Take the per-pairing permit first so one client cannot occupy all
        // global permits while waiting for its own fairness allowance.
        let pairing_permit = pairing_slots
            .acquire_owned()
            .await
            .map_err(|_| ApiError::internal())?;
        let global_permit = Arc::clone(&self.bulk_slots)
            .acquire_owned()
            .await
            .map_err(|_| ApiError::internal())?;
        Ok((pairing_permit, global_permit))
    }

    fn ensure_no_active_write_conflict(
        &self,
        space_id: &str,
        path: &str,
        include_descendants: bool,
    ) -> Result<(), ApiError> {
        let exact = write_path_key(space_id, path);
        let prefix = format!("{}\0{}/", space_id, path.trim_end_matches('/'));
        let paths = self
            .active_write_paths
            .lock()
            .map_err(|_| ApiError::internal())?;
        let conflict = paths
            .keys()
            .any(|key| key == &exact || (include_descendants && key.starts_with(&prefix)));
        if conflict {
            return Err(ApiError::new(
                409,
                ErrorCode::Conflict,
                "path is locked by an active stream write",
            ));
        }
        Ok(())
    }

    fn check_pairing_rate(&self, key: &str) -> Result<(), ApiError> {
        let now = now_ms();
        let mut rate = self.pairing_rate.lock().map_err(|_| ApiError::internal())?;
        let samples = rate.entry(key.to_owned()).or_default();
        samples.retain(|at| *at > now - PAIRING_RATE_WINDOW_MS);
        if samples.len() >= PAIRING_RATE_MAX {
            return Err(ApiError::new(
                429,
                ErrorCode::RateLimited,
                "too many pairing requests",
            ));
        }
        samples.push(now);
        Ok(())
    }

    fn authenticate(&self, request: &HttpRequest) -> Result<SessionState, ApiError> {
        self.prune_ephemeral();
        if self
            .global_maintenance
            .lock()
            .map_err(|_| ApiError::internal())?
            .is_some()
        {
            return Err(ApiError::new(
                503,
                ErrorCode::StorageUnavailable,
                "storage is temporarily locked by a privileged restore operation",
            ));
        }
        let authorization = request
            .headers
            .get("authorization")
            .ok_or_else(ApiError::auth_invalid)?;
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or_else(ApiError::auth_invalid)?;
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApiError::auth_invalid());
        }
        if self
            .revoked_sessions
            .lock()
            .map_err(|_| ApiError::internal())?
            .contains_key(token)
        {
            return Err(ApiError::new(
                401,
                ErrorCode::AuthRevoked,
                "pairing access has been revoked",
            ));
        }
        let session = {
            let sessions = self.sessions.lock().map_err(|_| ApiError::internal())?;
            sessions
                .get(token)
                .cloned()
                .ok_or_else(ApiError::auth_invalid)?
        };
        if session.expires_at_ms <= now_ms() {
            return Err(ApiError::auth_invalid());
        }
        let pairing = self
            .storage
            .pairing_by_id(&session.pairing_id)
            .map_err(ApiError::from_storage)?;
        if !matches!(pairing, Some(record) if record.revoked_at_ms.is_none()) {
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.remove(token);
            }
            if let Ok(mut revoked_sessions) = self.revoked_sessions.lock() {
                revoked_sessions.insert(token.to_owned(), session.expires_at_ms);
            }
            return Err(ApiError::new(
                401,
                ErrorCode::AuthRevoked,
                "pairing access has been revoked",
            ));
        }
        Ok(session)
    }

    fn authorize_space(
        &self,
        session: &SessionState,
        space_id: &str,
    ) -> Result<SpaceRecord, ApiError> {
        if self
            .exclusive_spaces
            .lock()
            .map_err(|_| ApiError::internal())?
            .contains_key(space_id)
        {
            return Err(ApiError::new(
                503,
                ErrorCode::StorageUnavailable,
                "storage is temporarily locked by a privileged maintenance operation",
            ));
        }
        let space = self
            .storage
            .get_space(space_id)
            .map_err(ApiError::from_storage)?
            .ok_or_else(|| ApiError::new(404, ErrorCode::SpaceNotFound, "space not found"))?;
        if space.owner_application_id != session.application_id {
            return Err(ApiError::new(
                403,
                ErrorCode::PermissionDenied,
                "space is not owned by this application",
            ));
        }
        Ok(space)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_event(
        &self,
        application_id: &str,
        event_type: EventKindWire,
        space_id: Option<&str>,
        path: Option<&str>,
        from_path: Option<&str>,
        to_path: Option<&str>,
        key: Option<&str>,
    ) {
        let sequence = self.event_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEventWire {
            sequence,
            event_type,
            space_id: space_id.map(str::to_owned),
            path: path.map(str::to_owned),
            from_path: from_path.map(str::to_owned),
            to_path: to_path.map(str::to_owned),
            key: key.map(str::to_owned),
            at_ms: now_ms(),
        };
        if let Ok(mut events) = self.events.lock() {
            events.push_back(BufferedEvent {
                application_id: application_id.to_owned(),
                event,
            });
            while events.len() > EVENT_BUFFER_CAPACITY {
                events.pop_front();
            }
        }
        self.event_notify.notify_waiters();
    }

    fn collect_events(
        &self,
        session: &SessionState,
        input: &EventPollRequest,
    ) -> Result<EventPollResponse, ApiError> {
        if let Some(space_id) = input.space_id.as_deref() {
            self.authorize_space(session, space_id)?;
        }
        let events = self.events.lock().map_err(|_| ApiError::internal())?;
        let oldest = events.front().map_or(
            self.event_sequence.load(Ordering::SeqCst).saturating_add(1),
            |item| item.event.sequence,
        );
        let overflow = input.after_sequence > 0 && input.after_sequence.saturating_add(1) < oldest;
        let mut out = Vec::new();
        for item in events.iter() {
            if (item.application_id != "*" && item.application_id != session.application_id)
                || item.event.sequence <= input.after_sequence
            {
                continue;
            }
            if let Some(space_id) = input.space_id.as_deref() {
                if item.event.space_id.as_deref() != Some(space_id) {
                    continue;
                }
            }
            if let Some(prefix) = input.path_prefix.as_deref() {
                let matches = item
                    .event
                    .path
                    .as_deref()
                    .is_some_and(|path| event_path_matches(path, prefix))
                    || item
                        .event
                        .from_path
                        .as_deref()
                        .is_some_and(|path| event_path_matches(path, prefix))
                    || item
                        .event
                        .to_path
                        .as_deref()
                        .is_some_and(|path| event_path_matches(path, prefix));
                if !matches {
                    continue;
                }
            }
            out.push(item.event.clone());
        }
        Ok(EventPollResponse {
            events: out,
            latest_sequence: self.event_sequence.load(Ordering::SeqCst),
            overflow,
        })
    }

    async fn poll_events(
        &self,
        session: &SessionState,
        input: &EventPollRequest,
    ) -> Result<EventPollResponse, ApiError> {
        // Register the waiter before the first collection so an event cannot slip
        // between the empty check and Notify subscription.
        let notified = self.event_notify.notified();
        let initial = self.collect_events(session, input)?;
        if initial.overflow || !initial.events.is_empty() || input.wait_ms.unwrap_or(0) == 0 {
            return Ok(initial);
        }
        let wait_ms = input
            .wait_ms
            .unwrap_or(EVENT_LONG_POLL_MAX_MS)
            .min(EVENT_LONG_POLL_MAX_MS);
        let _ = timeout(Duration::from_millis(wait_ms), notified).await;
        self.collect_events(session, input)
    }
}

struct PendingPairing {
    request_id: String,
    application_id: String,
    application: ClientIdentity,
    client_instance_id: String,
    created_at_ms: i64,
    expires_at_ms: i64,
    decision: PairingDecision,
}

impl PendingPairing {
    fn summary(&self) -> PendingPairingSummary {
        PendingPairingSummary {
            request_id: self.request_id.clone(),
            application_id: self.application_id.clone(),
            application: self.application.clone(),
            client_instance_id: self.client_instance_id.clone(),
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
        }
    }
}

enum PairingDecision {
    Pending,
    Approved { credential: Option<String> },
    Denied,
}

#[derive(Clone)]
struct SessionState {
    pairing_id: String,
    application_id: String,
    expires_at_ms: i64,
}

struct WriteStreamState {
    pairing_id: String,
    application_id: String,
    space_id: String,
    path: String,
    request_id: String,
    if_match: Option<String>,
    declared_size: Option<u64>,
    metadata: Option<FileMetadata>,
    temp_path: PathBuf,
    file: Option<File>,
    hasher: Sha256,
    next_seq: u64,
    received_bytes: u64,
    last_chunk_sha256: Option<String>,
    last_chunk_len: usize,
    last_activity_ms: i64,
    existed_before: bool,
    committed: Option<FileRecord>,
    operation_id: Option<String>,
}

struct ReadStreamState {
    pairing_id: String,
    space_id: String,
    path: String,
    etag: String,
    next_seq: u64,
    size: u64,
    last_activity_ms: i64,
    operation_id: Option<String>,
}

struct BufferedEvent {
    application_id: String,
    event: RuntimeEventWire,
}

struct LongOperationEntry {
    snapshot: LongOperationSnapshot,
    cancel: Arc<AtomicBool>,
    owner_application_id: Option<String>,
    desktop_dismissed: bool,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    version: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: Option<&'static str>,
    body: Vec<u8>,
    allow_origin: Option<String>,
    extra_headers: Vec<(&'static str, &'static str)>,
}

impl HttpResponse {
    fn json<T: Serialize>(status: u16, value: &T) -> Result<Self, ApiError> {
        let body = serde_json::to_vec(value).map_err(|_| ApiError::internal())?;
        Ok(Self {
            status,
            content_type: Some(CONTROL_CONTENT_TYPE),
            body,
            allow_origin: None,
            extra_headers: Vec::new(),
        })
    }

    fn binary(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: Some("application/octet-stream"),
            body,
            allow_origin: None,
            extra_headers: Vec::new(),
        }
    }

    fn no_content() -> Self {
        Self {
            status: 204,
            content_type: None,
            body: Vec::new(),
            allow_origin: None,
            extra_headers: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: u16,
    code: ErrorCode,
    message: String,
}

fn storage_error_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::PathInvalid(_) => "PATH_INVALID",
        StorageError::PathConflict(_) => "PATH_CONFLICT",
        StorageError::NotFound(_) => "NOT_FOUND",
        StorageError::Conflict(_) => "CONFLICT",
        StorageError::StorageUnavailable(_) => "STORAGE_UNAVAILABLE",
        StorageError::DiskSpaceLow(_) => "DISK_SPACE_LOW",
        StorageError::StorageCorrupt(_) | StorageError::SchemaUnsupported(_) => "STORAGE_CORRUPT",
        StorageError::RequestInvalid(_) => "REQUEST_INVALID",
        StorageError::OperationCancelled => "OPERATION_CANCELLED",
        StorageError::Internal(_)
        | StorageError::Io(_)
        | StorageError::Sqlite(_)
        | StorageError::Json(_) => "INTERNAL_ERROR",
    }
}

fn native_export_error_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::OperationCancelled => "EXPORT_CANCELLED",
        StorageError::PathInvalid(_)
        | StorageError::PathConflict(_)
        | StorageError::Conflict(_) => "EXPORT_CONFLICT",
        StorageError::NotFound(_) => "NOT_FOUND",
        StorageError::StorageUnavailable(_) | StorageError::Io(_) => "DESTINATION_UNAVAILABLE",
        StorageError::DiskSpaceLow(_) => "DISK_SPACE_LOW",
        StorageError::StorageCorrupt(_) | StorageError::SchemaUnsupported(_) => "STORAGE_CORRUPT",
        StorageError::RequestInvalid(message) if message.contains("archive") => {
            "ARCHIVE_UNSUPPORTED"
        }
        StorageError::RequestInvalid(message) if message.contains("conflict") => "EXPORT_CONFLICT",
        StorageError::RequestInvalid(_) => "REQUEST_INVALID",
        StorageError::Internal(_) | StorageError::Sqlite(_) | StorageError::Json(_) => {
            "INTERNAL_ERROR"
        }
    }
}

fn native_import_error_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::OperationCancelled => "IMPORT_CANCELLED",
        StorageError::PathInvalid(_)
        | StorageError::PathConflict(_)
        | StorageError::Conflict(_) => "CONFLICT",
        StorageError::NotFound(_) => "NOT_FOUND",
        StorageError::StorageUnavailable(_) | StorageError::Io(_) => "DESTINATION_UNAVAILABLE",
        StorageError::DiskSpaceLow(_) => "DISK_SPACE_LOW",
        StorageError::StorageCorrupt(_) | StorageError::SchemaUnsupported(_) => "STORAGE_CORRUPT",
        StorageError::RequestInvalid(message) if message.contains("archive") => {
            "ARCHIVE_UNSUPPORTED"
        }
        StorageError::RequestInvalid(_) => "REQUEST_INVALID",
        StorageError::Internal(_) | StorageError::Sqlite(_) | StorageError::Json(_) => {
            "INTERNAL_ERROR"
        }
    }
}

fn snapshot_error_code(error: &StorageError) -> &'static str {
    match error {
        StorageError::NotFound(_) => "SNAPSHOT_NOT_FOUND",
        StorageError::Conflict(_) | StorageError::PathConflict(_) => "SNAPSHOT_RESTORE_CONFLICT",
        StorageError::OperationCancelled => "OPERATION_CANCELLED",
        other => storage_error_code(other),
    }
}

impl ApiError {
    fn new(status: u16, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn internal() -> Self {
        Self::new(500, ErrorCode::InternalError, "internal runtime error")
    }
    fn auth_invalid() -> Self {
        Self::new(
            401,
            ErrorCode::AuthInvalid,
            "authentication is invalid or expired",
        )
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, ErrorCode::RequestInvalid, message)
    }

    fn from_storage(error: StorageError) -> Self {
        match error {
            StorageError::PathInvalid(message) => Self::new(400, ErrorCode::PathInvalid, message),
            StorageError::PathConflict(message) => Self::new(409, ErrorCode::PathConflict, message),
            StorageError::NotFound(message) => Self::new(404, ErrorCode::NotFound, message),
            StorageError::Conflict(message) => Self::new(409, ErrorCode::Conflict, message),
            StorageError::StorageUnavailable(message) => {
                Self::new(503, ErrorCode::StorageUnavailable, message)
            }
            StorageError::DiskSpaceLow(message) => Self::new(507, ErrorCode::DiskSpaceLow, message),
            StorageError::StorageCorrupt(message) => {
                Self::new(503, ErrorCode::StorageCorrupt, message)
            }
            StorageError::SchemaUnsupported(_) => Self::new(
                500,
                ErrorCode::StorageCorrupt,
                "storage schema is unsupported",
            ),
            StorageError::RequestInvalid(message) => Self::bad_request(message),
            StorageError::OperationCancelled => {
                Self::new(409, ErrorCode::Conflict, "operation was cancelled")
            }
            StorageError::Internal(_)
            | StorageError::Io(_)
            | StorageError::Sqlite(_)
            | StorageError::Json(_) => Self::internal(),
        }
    }

    fn response(&self) -> HttpResponse {
        let body = serde_json::to_vec(&ErrorResponse {
            error: ErrorBody {
                code: self.code,
                message: self.message.clone(),
            },
        })
        .unwrap_or_else(|_| {
            br#"{"error":{"code":"INTERNAL_ERROR","message":"internal runtime error"}}"#.to_vec()
        });
        HttpResponse {
            status: self.status,
            content_type: Some(CONTROL_CONTENT_TYPE),
            body,
            allow_origin: None,
            extra_headers: Vec::new(),
        }
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    state: Arc<RuntimeState>,
) -> Result<(), std::io::Error> {
    let request = match timeout(REQUEST_READ_TIMEOUT, read_request(&mut stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => {
            let response = error.response();
            return write_response(&mut stream, response).await;
        }
        Err(_) => {
            let response =
                ApiError::new(408, ErrorCode::RequestInvalid, "request timed out").response();
            return write_response(&mut stream, response).await;
        }
    };

    let origin = request.headers.get("origin").cloned();
    let response = match handle_request(&state, request).await {
        Ok(response) => response,
        Err(error) => error.response(),
    };
    let response = apply_cors(response, origin.as_deref());
    write_response(&mut stream, response).await
}

async fn handle_request(
    state: &Arc<RuntimeState>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    validate_http_boundary(&request)?;

    if request.method == "OPTIONS" {
        let mut response = HttpResponse::no_content();
        response
            .extra_headers
            .push(("Access-Control-Allow-Methods", "GET, POST, PUT, OPTIONS"));
        response.extra_headers.push((
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization",
        ));
        response
            .extra_headers
            .push(("Access-Control-Max-Age", "600"));
        return Ok(response);
    }

    if request.path != "/v1/health" {
        match state.lifecycle_status() {
            RuntimeLifecycleStatus::Draining => {
                return Err(ApiError::new(
                    503,
                    ErrorCode::RuntimeShuttingDown,
                    "runtime is shutting down",
                ))
            }
            RuntimeLifecycleStatus::Stopped => {
                return Err(ApiError::new(
                    503,
                    ErrorCode::RuntimeStopped,
                    "runtime is stopped",
                ))
            }
            RuntimeLifecycleStatus::Suspended => {
                return Err(ApiError::new(
                    503,
                    ErrorCode::RuntimeStarting,
                    "runtime is suspended and will require reconnect after resume",
                ))
            }
            RuntimeLifecycleStatus::Running => {}
        }
    }

    if request.path != "/v1/streams/write/begin" && request.path.starts_with("/v1/streams/write/") {
        return handle_write_stream_route(state, request).await;
    }
    if request.path != "/v1/streams/read/begin" && request.path.starts_with("/v1/streams/read/") {
        return handle_read_stream_route(state, request).await;
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") => HttpResponse::json(200, &state.health()),
        ("POST", "/v1/identity/challenge") => {
            require_control_content_type(&request)?;
            let input: RuntimeIdentityChallengeRequest = parse_json(&request.body)?;
            validate_client_instance_id(&input.client_instance_id)?;
            if input.application.external_id.trim().is_empty()
                || input.application.external_id.len() > 512
            {
                return Err(ApiError::bad_request(
                    "application identity is empty or too long",
                ));
            }
            if input.nonce.len() != 64 || !input.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(ApiError::bad_request(
                    "identity challenge nonce must be 256-bit hex",
                ));
            }
            let application = state
                .storage
                .application_by_identity(
                    to_core_kind(input.application.kind),
                    &input.application.external_id,
                )
                .map_err(ApiError::from_storage)?
                .ok_or_else(|| {
                    ApiError::new(
                        404,
                        ErrorCode::PairingRuntimeNotFound,
                        "no active pairing for this application/client",
                    )
                })?;
            let pairing = state
                .storage
                .active_pairings_for_client(&application.id, &input.client_instance_id)
                .map_err(ApiError::from_storage)?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ApiError::new(
                        404,
                        ErrorCode::PairingRuntimeNotFound,
                        "no active pairing for this application/client",
                    )
                })?;
            let key = decode_hex_32(&pairing.credential_hash).ok_or_else(ApiError::internal)?;
            let mac = hmac_sha256_hex(
                &key,
                &[
                    RUNTIME_IDENTITY_DOMAIN_SEPARATOR.as_bytes(),
                    input.nonce.as_bytes(),
                ]
                .concat(),
            );
            HttpResponse::json(200, &RuntimeIdentityChallengeResponse { mac })
        }
        ("POST", "/v1/pairings/request") => {
            require_control_content_type(&request)?;
            let input: PairingRequest = parse_json(&request.body)?;
            validate_identity(&input.application, &input.client_instance_id)?;
            state.prune_ephemeral();
            let rate_key = format!(
                "{:?}:{}:{}",
                input.application.kind, input.application.external_id, input.client_instance_id
            );
            state.check_pairing_rate(&rate_key)?;

            let application = state
                .storage
                .register_application(
                    to_core_kind(input.application.kind),
                    &input.application.external_id,
                    &input.application.display_name,
                )
                .map_err(ApiError::from_storage)?;
            let now = now_ms();
            let mut pending = state
                .pending_pairings
                .lock()
                .map_err(|_| ApiError::internal())?;
            if let Some(existing) = pending.values().find(|entry| {
                entry.application_id == application.id
                    && entry.client_instance_id == input.client_instance_id
                    && entry.expires_at_ms > now
                    && matches!(&entry.decision, PairingDecision::Pending)
            }) {
                return HttpResponse::json(
                    200,
                    &PairingRequestResponse {
                        pairing_id: existing.request_id.clone(),
                        status: PairingStatus::Pending,
                        expires_at_ms: existing.expires_at_ms,
                    },
                );
            }
            if pending
                .values()
                .filter(|entry| {
                    entry.expires_at_ms > now && matches!(&entry.decision, PairingDecision::Pending)
                })
                .count()
                >= MAX_PENDING_PAIRINGS
            {
                return Err(ApiError::new(
                    429,
                    ErrorCode::RateLimited,
                    "too many pending pairing requests",
                ));
            }
            let request_id = Uuid::new_v4().to_string();
            let expires_at_ms = now + PAIRING_TTL_MS;
            pending.insert(
                request_id.clone(),
                PendingPairing {
                    request_id: request_id.clone(),
                    application_id: application.id,
                    application: input.application,
                    client_instance_id: input.client_instance_id,
                    created_at_ms: now,
                    expires_at_ms,
                    decision: PairingDecision::Pending,
                },
            );
            HttpResponse::json(
                202,
                &PairingRequestResponse {
                    pairing_id: request_id,
                    status: PairingStatus::Pending,
                    expires_at_ms,
                },
            )
        }
        ("POST", "/v1/pairings/poll") => {
            require_control_content_type(&request)?;
            let input: PairingPollRequest = parse_json(&request.body)?;
            let mut pending = state
                .pending_pairings
                .lock()
                .map_err(|_| ApiError::internal())?;
            let entry = pending.get_mut(&input.pairing_id).ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::PairingExpired,
                    "pairing request not found or expired",
                )
            })?;
            let now = now_ms();
            if entry.expires_at_ms <= now {
                return HttpResponse::json(
                    200,
                    &PairingPollResponse {
                        pairing_id: input.pairing_id,
                        status: PairingStatus::Expired,
                        expires_at_ms: entry.expires_at_ms,
                        pairing_credential: None,
                    },
                );
            }
            let (status, credential) = match &mut entry.decision {
                PairingDecision::Pending => (PairingStatus::Pending, None),
                PairingDecision::Denied => (PairingStatus::Denied, None),
                PairingDecision::Approved { credential } => {
                    (PairingStatus::Approved, credential.take())
                }
            };
            HttpResponse::json(
                200,
                &PairingPollResponse {
                    pairing_id: input.pairing_id,
                    status,
                    expires_at_ms: entry.expires_at_ms,
                    pairing_credential: credential,
                },
            )
        }
        ("POST", "/v1/sessions") => {
            require_control_content_type(&request)?;
            let input: SessionRequest = parse_json(&request.body)?;
            validate_client_instance_id(&input.client_instance_id)?;
            if input.pairing_credential.len() != 64
                || !input
                    .pairing_credential
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(ApiError::auth_invalid());
            }
            let credential_hash = sha256_hex(input.pairing_credential.as_bytes());
            let pairing = state
                .storage
                .find_active_pairing_by_credential_hash(&input.client_instance_id, &credential_hash)
                .map_err(ApiError::from_storage)?
                .ok_or_else(ApiError::auth_invalid)?;
            state
                .storage
                .touch_pairing(&pairing.id)
                .map_err(ApiError::from_storage)?;
            state.prune_ephemeral();
            let token = random_secret_hex(32);
            let expires_at_ms = now_ms() + SESSION_TTL_MS;
            {
                let mut sessions = state.sessions.lock().map_err(|_| ApiError::internal())?;
                if sessions.len() >= MAX_ACTIVE_SESSIONS {
                    return Err(ApiError::new(
                        429,
                        ErrorCode::RateLimited,
                        "too many active runtime sessions",
                    ));
                }
                sessions.insert(
                    token.clone(),
                    SessionState {
                        pairing_id: pairing.id,
                        application_id: pairing.application_id.clone(),
                        expires_at_ms,
                    },
                );
            }
            HttpResponse::json(
                200,
                &SessionResponse {
                    token,
                    expires_at_ms,
                    application_id: pairing.application_id,
                    capabilities: vec![
                        "files".into(),
                        "kv".into(),
                        "spaces".into(),
                        "streams".into(),
                        "events".into(),
                        "formats".into(),
                        "operations".into(),
                        "native-export".into(),
                        "native-import".into(),
                        "saved-directories".into(),
                        "export-presets".into(),
                        "backup".into(),
                        "snapshots".into(),
                        "batch".into(),
                        "storage-category".into(),
                        "system-progress-window".into(),
                    ],
                },
            )
        }
        ("POST", "/v1/spaces/open") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: OpenSpaceRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let storage_class = to_core_storage_class(input.storage_class);
            let storage_category = input.storage_category.map(to_core_storage_category);
            let space = state
                .storage
                .open_space_with_category(
                    &session.application_id,
                    &input.key,
                    storage_class,
                    storage_category,
                    input.display_name.as_deref(),
                )
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &space_wire(space))
        }
        ("POST", "/v1/spaces/list") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let spaces = state
                .storage
                .list_spaces(&session.application_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &ListSpacesResponse {
                    spaces: spaces.into_iter().map(space_wire).collect(),
                },
            )
        }
        ("POST", "/v1/kv/get") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: KvGetRequest = parse_json(&request.body)?;
            state.authorize_space(&session, &input.space_id)?;
            let entry = state
                .storage
                .kv_get(&input.space_id, &input.key)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &KvGetResponse {
                    entry: entry.map(kv_wire),
                },
            )
        }
        ("POST", "/v1/kv/set") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: KvSetRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            let entry = state
                .storage
                .kv_set(
                    &input.space_id,
                    &input.key,
                    &input.value,
                    input.if_version,
                    input.if_match.as_deref(),
                    &input.request_id,
                )
                .map_err(ApiError::from_storage)?;
            state.emit_event(
                &space.owner_application_id,
                EventKindWire::KvChanged,
                Some(&input.space_id),
                None,
                None,
                None,
                Some(&input.key),
            );
            HttpResponse::json(200, &kv_wire(entry))
        }
        ("POST", "/v1/streams/write/begin") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: StreamWriteBeginRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            if input
                .declared_size
                .is_some_and(|size| size > MAX_MANAGED_FILE_BYTES)
            {
                return Err(ApiError::new(
                    413,
                    ErrorCode::FileTooLarge,
                    "declared file exceeds the 16 GiB safety ceiling",
                ));
            }
            if let Some(declared_size) = input.declared_size {
                state
                    .storage
                    .ensure_write_capacity(&input.space_id, declared_size)
                    .map_err(ApiError::from_storage)?;
            }
            state.prune_ephemeral();
            {
                let writes = state
                    .write_streams
                    .lock()
                    .map_err(|_| ApiError::internal())?;
                if let Some((stream_id, stream)) = writes.iter().find(|(_, stream)| {
                    stream.pairing_id == session.pairing_id && stream.request_id == input.request_id
                }) {
                    let requested_metadata = input.metadata.as_ref().map(to_core_file_metadata);
                    let requested_operation_id = input
                        .operation
                        .as_ref()
                        .map(|operation| operation.id.as_str());
                    if stream.space_id != input.space_id
                        || stream.path != input.path
                        || stream.if_match != input.if_match
                        || stream.declared_size != input.declared_size
                        || stream.metadata != requested_metadata
                        || stream.operation_id.as_deref() != requested_operation_id
                    {
                        return Err(ApiError::new(
                            409,
                            ErrorCode::Conflict,
                            "requestId is already bound to a different stream mutation",
                        ));
                    }
                    return HttpResponse::json(
                        200,
                        &StreamWriteBeginResponse {
                            stream_id: stream_id.clone(),
                            max_chunk_bytes: MAX_STREAM_CHUNK_BYTES,
                        },
                    );
                }
                let active = writes
                    .values()
                    .filter(|stream| {
                        stream.pairing_id == session.pairing_id && stream.committed.is_none()
                    })
                    .count();
                if active >= MAX_STREAM_WRITES_PER_SESSION {
                    return Err(ApiError::new(
                        429,
                        ErrorCode::RateLimited,
                        "too many concurrent stream writes for this session",
                    ));
                }
            }
            let key = write_path_key(&input.space_id, &input.path);
            {
                let mut paths = state
                    .active_write_paths
                    .lock()
                    .map_err(|_| ApiError::internal())?;
                if paths.contains_key(&key) {
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "a stream write is already active for this path",
                    ));
                }
                paths.insert(key.clone(), String::new());
            }
            let stream_id = format!("stream-{}", Uuid::new_v4().simple());
            let temp_path =
                match state
                    .storage
                    .prepare_stream_temp(&input.space_id, &input.path, &stream_id)
                {
                    Ok(path) => path,
                    Err(error) => {
                        if let Ok(mut paths) = state.active_write_paths.lock() {
                            paths.remove(&key);
                        }
                        return Err(ApiError::from_storage(error));
                    }
                };
            let file = match OpenOptions::new().append(true).open(&temp_path) {
                Ok(file) => file,
                Err(_) => {
                    let _ = state.storage.abort_stream_temp(&input.space_id, &temp_path);
                    if let Ok(mut paths) = state.active_write_paths.lock() {
                        paths.remove(&key);
                    }
                    return Err(ApiError::internal());
                }
            };
            let existed_before = match state.storage.stat_file(&input.space_id, &input.path) {
                Ok(value) => value.is_some(),
                Err(error) => {
                    drop(file);
                    let _ = state.storage.abort_stream_temp(&input.space_id, &temp_path);
                    if let Ok(mut paths) = state.active_write_paths.lock() {
                        paths.remove(&key);
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            if let Ok(mut paths) = state.active_write_paths.lock() {
                paths.insert(key.clone(), stream_id.clone());
            }
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "write",
                    "writing",
                    true,
                    input.declared_size,
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_bytes(&snapshot.id, "writing", 0, input.declared_size);
            }
            let cleanup_space_id = input.space_id.clone();
            let cleanup_temp_path = temp_path.clone();
            let stream = WriteStreamState {
                pairing_id: session.pairing_id.clone(),
                application_id: space.owner_application_id,
                space_id: input.space_id,
                path: input.path,
                request_id: input.request_id,
                if_match: input.if_match,
                declared_size: input.declared_size,
                metadata: input.metadata.as_ref().map(to_core_file_metadata),
                temp_path,
                file: Some(file),
                hasher: Sha256::new(),
                next_seq: 0,
                received_bytes: 0,
                last_chunk_sha256: None,
                last_chunk_len: 0,
                last_activity_ms: now_ms(),
                existed_before,
                committed: None,
                operation_id: operation.as_ref().map(|(snapshot, _)| snapshot.id.clone()),
            };
            match state.write_streams.lock() {
                Ok(mut streams) => {
                    streams.insert(stream_id.clone(), stream);
                }
                Err(_) => {
                    drop(stream);
                    let _ = state
                        .storage
                        .abort_stream_temp(&cleanup_space_id, &cleanup_temp_path);
                    if let Ok(mut paths) = state.active_write_paths.lock() {
                        paths.remove(&key);
                    }
                    return Err(ApiError::internal());
                }
            }
            HttpResponse::json(
                200,
                &StreamWriteBeginResponse {
                    stream_id,
                    max_chunk_bytes: MAX_STREAM_CHUNK_BYTES,
                },
            )
        }
        ("POST", "/v1/streams/read/begin") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: StreamReadBeginRequest = parse_json(&request.body)?;
            state.authorize_space(&session, &input.space_id)?;
            let metadata = state
                .storage
                .stat_file(&input.space_id, &input.path)
                .map_err(ApiError::from_storage)?
                .ok_or_else(|| ApiError::new(404, ErrorCode::NotFound, "file not found"))?;
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "read",
                    "reading",
                    true,
                    Some(metadata.size),
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_bytes(&snapshot.id, "reading", 0, Some(metadata.size));
            }
            let stream_id = format!("stream-{}", Uuid::new_v4().simple());
            state
                .read_streams
                .lock()
                .map_err(|_| ApiError::internal())?
                .insert(
                    stream_id.clone(),
                    ReadStreamState {
                        pairing_id: session.pairing_id.clone(),
                        space_id: input.space_id,
                        path: input.path,
                        etag: metadata.etag.clone(),
                        next_seq: 0,
                        size: metadata.size,
                        last_activity_ms: now_ms(),
                        operation_id: operation.as_ref().map(|(snapshot, _)| snapshot.id.clone()),
                    },
                );
            HttpResponse::json(
                200,
                &StreamReadBeginResponse {
                    stream_id,
                    file: file_wire(metadata),
                    chunk_size: DEFAULT_STREAM_CHUNK_BYTES,
                },
            )
        }
        ("POST", "/v1/fs/delete") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: FsDeleteRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            state.ensure_no_active_write_conflict(
                &input.space_id,
                &input.path,
                input.recursive.unwrap_or(false),
            )?;
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "delete",
                    "deleting",
                    false,
                    None,
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_items(&snapshot.id, "deleting", 0, None);
            }
            let _bulk = state.acquire_bulk(&session.pairing_id).await?;
            let storage = Arc::clone(&state.storage);
            let space_id = input.space_id.clone();
            let path = input.path.clone();
            let recursive = input.recursive.unwrap_or(false);
            let if_match = input.if_match.clone();
            let request_id = input.request_id.clone();
            let operation_id = operation.as_ref().map(|(snapshot, _)| snapshot.id.clone());
            let progress_state = Arc::clone(state);
            let result = tokio::task::spawn_blocking(move || {
                storage.delete_path_with_progress(
                    &space_id,
                    &path,
                    recursive,
                    if_match.as_deref(),
                    &request_id,
                    || false,
                    |done, total| {
                        if let Some(id) = operation_id.as_deref() {
                            progress_state.update_operation_items(id, "deleting", done, total);
                        }
                    },
                )
            })
            .await
            .map_err(|_| ApiError::internal())?;
            let count = match result {
                Ok(count) => {
                    if let Some((snapshot, _)) = &operation {
                        state.complete_operation(
                            &snapshot.id,
                            "complete",
                            Some(serde_json::json!({"deletedFiles":count})),
                        );
                    }
                    count
                }
                Err(StorageError::OperationCancelled) => {
                    if let Some((snapshot, _)) = &operation {
                        state.cancelled_operation(&snapshot.id);
                    }
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
                Err(error) => {
                    if let Some((snapshot, _)) = &operation {
                        state.fail_operation_code(
                            &snapshot.id,
                            storage_error_code(&error),
                            error.to_string(),
                        );
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            state.emit_event(
                &space.owner_application_id,
                EventKindWire::FileDeleted,
                Some(&input.space_id),
                Some(&input.path),
                None,
                None,
                None,
            );
            HttpResponse::json(200, &serde_json::json!({ "deletedFiles": count }))
        }
        ("POST", "/v1/fs/copy") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: FsCopyRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            state.ensure_no_active_write_conflict(&input.space_id, &input.to, true)?;
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "copy",
                    "copying",
                    true,
                    None,
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_items(&snapshot.id, "copying", 0, None);
            }
            let _bulk = state.acquire_bulk(&session.pairing_id).await?;
            let storage = Arc::clone(&state.storage);
            let space_id = input.space_id.clone();
            let from = input.from.clone();
            let to = input.to.clone();
            let overwrite = input.overwrite.unwrap_or(false);
            let request_id = input.request_id.clone();
            let operation_id = operation.as_ref().map(|(snapshot, _)| snapshot.id.clone());
            let cancel = operation.as_ref().map(|(_, cancel)| Arc::clone(cancel));
            let progress_state = Arc::clone(state);
            let result = tokio::task::spawn_blocking(move || {
                storage.copy_path_with_progress(
                    &space_id,
                    &from,
                    &to,
                    overwrite,
                    &request_id,
                    || {
                        cancel
                            .as_ref()
                            .is_some_and(|flag| flag.load(Ordering::SeqCst))
                    },
                    |done, total| {
                        if let Some(id) = operation_id.as_deref() {
                            progress_state.update_operation_items(id, "copying", done, total);
                        }
                    },
                )
            })
            .await
            .map_err(|_| ApiError::internal())?;
            let count = match result {
                Ok(count) => {
                    if let Some((snapshot, _)) = &operation {
                        state.complete_operation(
                            &snapshot.id,
                            "complete",
                            Some(serde_json::json!({"copiedFiles":count})),
                        );
                    }
                    count
                }
                Err(StorageError::OperationCancelled) => {
                    if let Some((snapshot, _)) = &operation {
                        state.cancelled_operation(&snapshot.id);
                    }
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
                Err(error) => {
                    if let Some((snapshot, _)) = &operation {
                        state.fail_operation_code(
                            &snapshot.id,
                            storage_error_code(&error),
                            error.to_string(),
                        );
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            state.emit_event(
                &space.owner_application_id,
                EventKindWire::FileCreated,
                Some(&input.space_id),
                Some(&input.to),
                None,
                None,
                None,
            );
            HttpResponse::json(200, &serde_json::json!({ "copiedFiles": count }))
        }
        ("POST", "/v1/fs/move") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: FsMoveRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            state.ensure_no_active_write_conflict(&input.space_id, &input.from, true)?;
            state.ensure_no_active_write_conflict(&input.space_id, &input.to, true)?;
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "move",
                    "moving",
                    true,
                    None,
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_items(&snapshot.id, "moving", 0, None);
            }
            let _bulk = state.acquire_bulk(&session.pairing_id).await?;
            let storage = Arc::clone(&state.storage);
            let space_id = input.space_id.clone();
            let from = input.from.clone();
            let to = input.to.clone();
            let overwrite = input.overwrite.unwrap_or(false);
            let if_match = input.if_match.clone();
            let request_id = input.request_id.clone();
            let operation_id = operation.as_ref().map(|(snapshot, _)| snapshot.id.clone());
            let cancel = operation.as_ref().map(|(_, cancel)| Arc::clone(cancel));
            let progress_state = Arc::clone(state);
            let result = tokio::task::spawn_blocking(move || {
                storage.move_path_with_progress(
                    &space_id,
                    &from,
                    &to,
                    overwrite,
                    if_match.as_deref(),
                    &request_id,
                    || {
                        cancel
                            .as_ref()
                            .is_some_and(|flag| flag.load(Ordering::SeqCst))
                    },
                    |done, total| {
                        if let Some(id) = operation_id.as_deref() {
                            progress_state.update_operation_items(id, "moving", done, total);
                        }
                    },
                )
            })
            .await
            .map_err(|_| ApiError::internal())?;
            let count = match result {
                Ok(count) => {
                    if let Some((snapshot, _)) = &operation {
                        state.complete_operation(
                            &snapshot.id,
                            "complete",
                            Some(serde_json::json!({"movedFiles":count})),
                        );
                    }
                    count
                }
                Err(StorageError::OperationCancelled) => {
                    if let Some((snapshot, _)) = &operation {
                        state.cancelled_operation(&snapshot.id);
                    }
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
                Err(error) => {
                    if let Some((snapshot, _)) = &operation {
                        state.fail_operation_code(
                            &snapshot.id,
                            storage_error_code(&error),
                            error.to_string(),
                        );
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            state.emit_event(
                &space.owner_application_id,
                EventKindWire::FileMoved,
                Some(&input.space_id),
                None,
                Some(&input.from),
                Some(&input.to),
                None,
            );
            HttpResponse::json(200, &serde_json::json!({ "movedFiles": count }))
        }
        ("POST", "/v1/batch") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: BatchRequest = parse_json(&request.body)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            if input.operations.is_empty() || input.operations.len() > MAX_BATCH_OPERATIONS {
                return Err(ApiError::new(
                    413,
                    ErrorCode::RequestTooLarge,
                    "batch must contain 1-64 operations",
                ));
            }

            let mut prepared_payloads: Vec<Option<Vec<u8>>> =
                Vec::with_capacity(input.operations.len());
            let mut total_payload_bytes = 0usize;
            for operation in &input.operations {
                let prepared = match operation {
                    BatchOperationWire::WriteFile {
                        request_id,
                        path,
                        data_base64,
                        metadata,
                        ..
                    } => {
                        validate_request_id(request_id)?;
                        LogicalPath::parse(path).map_err(ApiError::from_storage)?;
                        validate_batch_file_metadata(metadata.as_ref())?;
                        state.ensure_no_active_write_conflict(&input.space_id, path, false)?;
                        let bytes = base64_decode(data_base64).map_err(|_| {
                            ApiError::bad_request("batch write dataBase64 is invalid")
                        })?;
                        if bytes.len() > DIRECT_PAYLOAD_TARGET_BYTES {
                            return Err(ApiError::new(413, ErrorCode::RequestTooLarge, "batch write payload exceeds the direct-transfer threshold; use writeFile()/writeTree() so streaming can be selected automatically"));
                        }
                        total_payload_bytes = total_payload_bytes.saturating_add(bytes.len());
                        Some(bytes)
                    }
                    BatchOperationWire::Delete {
                        request_id,
                        path,
                        recursive,
                        ..
                    } => {
                        validate_request_id(request_id)?;
                        LogicalPath::parse(path).map_err(ApiError::from_storage)?;
                        state.ensure_no_active_write_conflict(
                            &input.space_id,
                            path,
                            recursive.unwrap_or(false),
                        )?;
                        None
                    }
                    BatchOperationWire::Copy {
                        request_id,
                        from,
                        to,
                        ..
                    } => {
                        validate_request_id(request_id)?;
                        LogicalPath::parse(from).map_err(ApiError::from_storage)?;
                        LogicalPath::parse(to).map_err(ApiError::from_storage)?;
                        state.ensure_no_active_write_conflict(&input.space_id, to, true)?;
                        None
                    }
                    BatchOperationWire::Move {
                        request_id,
                        from,
                        to,
                        ..
                    } => {
                        validate_request_id(request_id)?;
                        LogicalPath::parse(from).map_err(ApiError::from_storage)?;
                        LogicalPath::parse(to).map_err(ApiError::from_storage)?;
                        state.ensure_no_active_write_conflict(&input.space_id, from, true)?;
                        state.ensure_no_active_write_conflict(&input.space_id, to, true)?;
                        None
                    }
                    BatchOperationWire::KvSet {
                        request_id,
                        key,
                        value,
                        ..
                    } => {
                        validate_request_id(request_id)?;
                        validate_batch_kv(key, value)?;
                        None
                    }
                };
                if total_payload_bytes > MAX_BATCH_PAYLOAD_BYTES {
                    return Err(ApiError::new(
                        413,
                        ErrorCode::RequestTooLarge,
                        "batch decoded payload exceeds the 512 KiB safety limit",
                    ));
                }
                prepared_payloads.push(prepared);
            }

            let total_items = input.operations.len() as u64;
            let bytes_total = (total_payload_bytes > 0).then_some(total_payload_bytes as u64);
            let (snapshot, cancel) = state.begin_application_operation(
                &session.application_id,
                &input.operation,
                "batch",
                "validating",
                true,
                bytes_total,
                Some(total_items),
            )?;
            state.update_operation_metrics(
                &snapshot.id,
                "running",
                0,
                Some(total_items),
                0,
                bytes_total,
            );
            let _bulk = state.acquire_bulk(&session.pairing_id).await?;

            let storage = Arc::clone(&state.storage);
            let state_for_work = Arc::clone(state);
            let space_id = input.space_id.clone();
            let owner_application_id = space.owner_application_id.clone();
            let operation_id = snapshot.id.clone();
            let operations = input.operations;
            let result = tokio::task::spawn_blocking(move || {
                let mut results = Vec::with_capacity(operations.len());
                let mut completed_items = 0u64;
                let mut failed_items = 0u64;
                let mut completed_bytes = 0u64;
                let mut payloads = prepared_payloads.into_iter();
                let mut cancelled = false;

                for (index, operation) in operations.into_iter().enumerate() {
                    let payload = payloads.next().flatten();
                    if cancel.load(Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                    let (operation_type, outcome) = match operation {
                        BatchOperationWire::WriteFile {
                            request_id,
                            path,
                            if_match,
                            metadata,
                            ..
                        } => {
                            let bytes = payload.unwrap_or_default();
                            let existed =
                                storage.stat_file(&space_id, &path).ok().flatten().is_some();
                            let core_metadata = metadata.as_ref().map(to_core_file_metadata);
                            let outcome = storage
                                .write_file_with_metadata(
                                    &space_id,
                                    &path,
                                    &bytes,
                                    if_match.as_deref(),
                                    core_metadata.as_ref(),
                                    &request_id,
                                )
                                .map(|file| {
                                    state_for_work.emit_event(
                                        &owner_application_id,
                                        if existed {
                                            EventKindWire::FileChanged
                                        } else {
                                            EventKindWire::FileCreated
                                        },
                                        Some(&space_id),
                                        Some(&path),
                                        None,
                                        None,
                                        None,
                                    );
                                    BatchItemResultWire {
                                        index,
                                        operation_type: "write-file".into(),
                                        ok: true,
                                        file: Some(file_wire(file)),
                                        kv: None,
                                        affected_files: None,
                                        error: None,
                                    }
                                });
                            completed_bytes = completed_bytes.saturating_add(bytes.len() as u64);
                            ("write-file", outcome)
                        }
                        BatchOperationWire::Delete {
                            request_id,
                            path,
                            recursive,
                            if_match,
                        } => {
                            let outcome = storage
                                .delete_path_with_progress(
                                    &space_id,
                                    &path,
                                    recursive.unwrap_or(false),
                                    if_match.as_deref(),
                                    &request_id,
                                    || cancel.load(Ordering::SeqCst),
                                    |_, _| {},
                                )
                                .map(|count| {
                                    state_for_work.emit_event(
                                        &owner_application_id,
                                        EventKindWire::FileDeleted,
                                        Some(&space_id),
                                        Some(&path),
                                        None,
                                        None,
                                        None,
                                    );
                                    BatchItemResultWire {
                                        index,
                                        operation_type: "delete".into(),
                                        ok: true,
                                        file: None,
                                        kv: None,
                                        affected_files: Some(count),
                                        error: None,
                                    }
                                });
                            ("delete", outcome)
                        }
                        BatchOperationWire::Copy {
                            request_id,
                            from,
                            to,
                            overwrite,
                        } => {
                            let outcome = storage
                                .copy_path_with_progress(
                                    &space_id,
                                    &from,
                                    &to,
                                    overwrite.unwrap_or(false),
                                    &request_id,
                                    || cancel.load(Ordering::SeqCst),
                                    |_, _| {},
                                )
                                .map(|count| {
                                    state_for_work.emit_event(
                                        &owner_application_id,
                                        EventKindWire::FileCreated,
                                        Some(&space_id),
                                        Some(&to),
                                        None,
                                        None,
                                        None,
                                    );
                                    BatchItemResultWire {
                                        index,
                                        operation_type: "copy".into(),
                                        ok: true,
                                        file: None,
                                        kv: None,
                                        affected_files: Some(count),
                                        error: None,
                                    }
                                });
                            ("copy", outcome)
                        }
                        BatchOperationWire::Move {
                            request_id,
                            from,
                            to,
                            overwrite,
                            if_match,
                        } => {
                            let outcome = storage
                                .move_path_with_progress(
                                    &space_id,
                                    &from,
                                    &to,
                                    overwrite.unwrap_or(false),
                                    if_match.as_deref(),
                                    &request_id,
                                    || cancel.load(Ordering::SeqCst),
                                    |_, _| {},
                                )
                                .map(|count| {
                                    state_for_work.emit_event(
                                        &owner_application_id,
                                        EventKindWire::FileMoved,
                                        Some(&space_id),
                                        None,
                                        Some(&from),
                                        Some(&to),
                                        None,
                                    );
                                    BatchItemResultWire {
                                        index,
                                        operation_type: "move".into(),
                                        ok: true,
                                        file: None,
                                        kv: None,
                                        affected_files: Some(count),
                                        error: None,
                                    }
                                });
                            ("move", outcome)
                        }
                        BatchOperationWire::KvSet {
                            request_id,
                            key,
                            value,
                            if_version,
                            if_match,
                        } => {
                            let outcome = storage
                                .kv_set(
                                    &space_id,
                                    &key,
                                    &value,
                                    if_version,
                                    if_match.as_deref(),
                                    &request_id,
                                )
                                .map(|entry| {
                                    state_for_work.emit_event(
                                        &owner_application_id,
                                        EventKindWire::KvChanged,
                                        Some(&space_id),
                                        None,
                                        None,
                                        None,
                                        Some(&key),
                                    );
                                    BatchItemResultWire {
                                        index,
                                        operation_type: "kv-set".into(),
                                        ok: true,
                                        file: None,
                                        kv: Some(kv_wire(entry)),
                                        affected_files: None,
                                        error: None,
                                    }
                                });
                            ("kv-set", outcome)
                        }
                    };

                    completed_items = completed_items.saturating_add(1);
                    match outcome {
                        Ok(item) => results.push(item),
                        Err(StorageError::OperationCancelled) => {
                            cancelled = true;
                            break;
                        }
                        Err(error) => {
                            failed_items = failed_items.saturating_add(1);
                            results.push(BatchItemResultWire {
                                index,
                                operation_type: operation_type.into(),
                                ok: false,
                                file: None,
                                kv: None,
                                affected_files: None,
                                error: Some(BatchItemErrorWire {
                                    code: storage_error_code(&error).into(),
                                    message: error.to_string(),
                                }),
                            });
                        }
                    }
                    state_for_work.update_operation_metrics(
                        &operation_id,
                        "running",
                        completed_items,
                        Some(total_items),
                        completed_bytes,
                        bytes_total,
                    );
                }

                BatchResponse {
                    results,
                    completed_items,
                    failed_items,
                    cancelled,
                }
            })
            .await
            .map_err(|_| ApiError::internal())?;

            if result.cancelled {
                state.cancelled_operation(&snapshot.id);
            } else {
                state.update_operation_metrics(
                    &snapshot.id,
                    "complete",
                    result.completed_items,
                    Some(total_items),
                    total_payload_bytes as u64,
                    bytes_total,
                );
                state.complete_operation(
                    &snapshot.id,
                    "complete",
                    serde_json::to_value(&result).ok(),
                );
            }
            HttpResponse::json(200, &result)
        }
        ("POST", "/v1/operations/status") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: OperationStatusRequest = parse_json(&request.body)?;
            let snapshot = state
                .operation_status_for_application(&session.application_id, &input.operation_id)?;
            HttpResponse::json(200, &snapshot)
        }
        ("POST", "/v1/operations/cancel") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: OperationCancelRequest = parse_json(&request.body)?;
            let snapshot =
                state.cancel_application_operation(&session.application_id, &input.operation_id)?;
            HttpResponse::json(200, &snapshot)
        }
        ("POST", "/v1/formats/register") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: RegisterFormatRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let descriptor = state
                .storage
                .register_format(
                    &session.application_id,
                    &input.id,
                    input.extension.as_deref(),
                    &input.display_name,
                    input.content_type.as_deref(),
                    input.opaque.unwrap_or(false),
                    &input.request_id,
                )
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &format_wire(descriptor))
        }
        ("POST", "/v1/formats/list") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let formats = state
                .storage
                .list_formats(&session.application_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &ListFormatsResponse {
                    formats: formats.into_iter().map(format_wire).collect(),
                },
            )
        }
        ("POST", "/v1/formats/delete") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: DeleteFormatRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let deleted = state
                .storage
                .delete_format(&session.application_id, &input.id, &input.request_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &serde_json::json!({ "deleted": deleted }))
        }
        ("POST", "/v1/destinations/create") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: CreateDestinationGrantRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let application = state
                .storage
                .get_application(&session.application_id)
                .map_err(ApiError::from_storage)?
                .ok_or_else(ApiError::internal)?;
            let host = Arc::clone(&state.host_services);
            let app_name = application.display_name.clone();
            let capability_label = match input.capability {
                DirectoryGrantCapabilityWire::Read => "saved read directory",
                DirectoryGrantCapabilityWire::Write => "saved write destination",
                DirectoryGrantCapabilityWire::ReadWrite => "saved read-write directory",
            };
            let selected = tokio::task::spawn_blocking(move || {
                host.choose_directory(&app_name, capability_label)
            })
            .await
            .map_err(|_| ApiError::internal())?
            .map_err(|message| ApiError::new(409, ErrorCode::DestinationGrantRequired, message))?;
            let path = selected.ok_or_else(|| {
                ApiError::new(
                    409,
                    ErrorCode::DestinationGrantRequired,
                    "no destination directory was selected",
                )
            })?;
            let grant = state
                .storage
                .create_directory_grant(
                    &session.application_id,
                    &path,
                    input.label.as_deref(),
                    grant_capability_from_wire(input.capability),
                )
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &destination_grant_wire(&grant))
        }
        ("POST", "/v1/destinations/list") => {
            let session = state.authenticate(&request)?;
            let grants = state
                .storage
                .list_directory_grants(&session.application_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &ListDestinationGrantsResponse {
                    destinations: grants.iter().map(destination_grant_wire).collect(),
                },
            )
        }
        ("POST", "/v1/destinations/revoke") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: RevokeDestinationGrantRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let revoked = state
                .storage
                .revoke_directory_grant(&session.application_id, &input.destination_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &serde_json::json!({"revoked":revoked}))
        }
        ("POST", "/v1/export-presets/save") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: SaveExportPresetRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let preset = state
                .storage
                .save_export_preset(
                    &session.application_id,
                    input.id.as_deref(),
                    &input.name,
                    &input.destination_id,
                    export_mode_from_wire(input.mode),
                    export_conflict_from_wire(input.conflict),
                    &input.source_path,
                    input.archive_format.as_deref(),
                )
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &export_preset_wire(&preset))
        }
        ("POST", "/v1/export-presets/list") => {
            let session = state.authenticate(&request)?;
            let presets = state
                .storage
                .list_export_presets(&session.application_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &ListExportPresetsResponse {
                    presets: presets.iter().map(export_preset_wire).collect(),
                },
            )
        }
        ("POST", "/v1/export-presets/delete") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: DeleteExportPresetRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let deleted = state
                .storage
                .delete_export_preset(&session.application_id, &input.id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(200, &serde_json::json!({"deleted":deleted}))
        }
        ("POST", "/v1/exports/start") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: StartNativeExportRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            state.authorize_space(&session, &input.space_id)?;
            let application = state
                .storage
                .get_application(&session.application_id)
                .map_err(ApiError::from_storage)?
                .ok_or_else(ApiError::internal)?;
            let (destination_path, destination_label, grant_id) =
                if let Some(id) = input.destination_id.as_deref() {
                    let grant = state
                        .storage
                        .get_directory_grant(&session.application_id, id)
                        .map_err(ApiError::from_storage)?
                        .ok_or_else(|| {
                            ApiError::new(
                                404,
                                ErrorCode::DestinationGrantRequired,
                                "saved destination does not exist",
                            )
                        })?;
                    if grant.revoked_at_ms.is_some() {
                        return Err(ApiError::new(
                            409,
                            ErrorCode::DestinationGrantRevoked,
                            "saved destination has been revoked",
                        ));
                    }
                    if !grant.capability.can_write() {
                        return Err(ApiError::new(
                            403,
                            ErrorCode::DestinationReadOnly,
                            "saved destination does not allow writes",
                        ));
                    }
                    if matches!(input.conflict, ExportConflictPolicyWire::UpdateChanged)
                        && !grant.capability.can_read()
                    {
                        return Err(ApiError::new(
                            403,
                            ErrorCode::PermissionDenied,
                            "update-changed requires a read-write destination grant",
                        ));
                    }
                    (
                        PathBuf::from(&grant.physical_path),
                        grant.label,
                        Some(grant.id),
                    )
                } else {
                    let host = Arc::clone(&state.host_services);
                    let app_name = application.display_name.clone();
                    let selected = tokio::task::spawn_blocking(move || {
                        host.choose_directory(&app_name, "one-off export destination")
                    })
                    .await
                    .map_err(|_| ApiError::internal())?
                    .map_err(|message| {
                        ApiError::new(409, ErrorCode::DestinationGrantRequired, message)
                    })?;
                    let path = selected.ok_or_else(|| {
                        ApiError::new(
                            409,
                            ErrorCode::DestinationGrantRequired,
                            "no export destination was selected",
                        )
                    })?;
                    let label = path
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or("Selected folder")
                        .to_owned();
                    (path, label, None)
                };
            let (snapshot, cancel) = state.begin_application_operation(
                &session.application_id,
                &input.operation,
                "native-export",
                "preparing",
                true,
                None,
                None,
            )?;
            let operation_id = snapshot.id.clone();
            let state2 = Arc::clone(state);
            let storage = Arc::clone(&state.storage);
            let space_id = input.space_id.clone();
            let app_name = application.display_name.clone();
            let host = Arc::clone(&state.host_services);
            let source_paths = input.source_paths.clone();
            let mode = export_mode_from_wire(input.mode);
            let conflict = export_conflict_from_wire(input.conflict);
            let archive_name = input.archive_name.clone();
            tokio::spawn(async move {
                let state_progress = Arc::clone(&state2);
                let op_progress = operation_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    storage.native_export(
                        &space_id,
                        &source_paths,
                        &destination_path,
                        &destination_label,
                        mode,
                        conflict,
                        archive_name.as_deref(),
                        || cancel.load(Ordering::SeqCst),
                        |items, total_items, bytes, total_bytes| {
                            state_progress.update_operation_metrics(
                                &op_progress,
                                "exporting",
                                items,
                                total_items,
                                bytes,
                                total_bytes,
                            )
                        },
                        |item| {
                            host.confirm_export_replace(&app_name, item)
                                .map_err(StorageError::RequestInvalid)
                        },
                    )
                })
                .await;
                match result {
                    Ok(Ok(report)) => {
                        if let Some(id) = grant_id.as_deref() {
                            let _ = state2
                                .storage
                                .touch_directory_grant(&session.application_id, id);
                        }
                        state2.complete_operation(
                            &operation_id,
                            "complete",
                            serde_json::to_value(report).ok(),
                        );
                    }
                    Ok(Err(StorageError::OperationCancelled)) => {
                        state2.cancelled_operation(&operation_id)
                    }
                    Ok(Err(error)) => state2.fail_operation_code(
                        &operation_id,
                        native_export_error_code(&error),
                        error.to_string(),
                    ),
                    Err(error) => state2
                        .fail_operation(&operation_id, format!("operation worker failed: {error}")),
                }
            });
            HttpResponse::json(202, &snapshot)
        }
        ("POST", "/v1/imports/start") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: StartNativeImportRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            state.authorize_space(&session, &input.space_id)?;
            let application = state
                .storage
                .get_application(&session.application_id)
                .map_err(ApiError::from_storage)?
                .ok_or_else(ApiError::internal)?;
            let import_mode = import_mode_from_wire(input.mode);
            let (selected_sources, source_label, grant_id) = if let Some(id) =
                input.source_id.as_deref()
            {
                let grant = state
                    .storage
                    .get_directory_grant(&session.application_id, id)
                    .map_err(ApiError::from_storage)?
                    .ok_or_else(|| {
                        ApiError::new(
                            404,
                            ErrorCode::DestinationGrantRequired,
                            "saved directory grant does not exist",
                        )
                    })?;
                if grant.revoked_at_ms.is_some() {
                    return Err(ApiError::new(
                        409,
                        ErrorCode::DestinationGrantRevoked,
                        "saved directory grant has been revoked",
                    ));
                }
                if !grant.capability.can_read() {
                    return Err(ApiError::new(
                        403,
                        ErrorCode::PermissionDenied,
                        "saved directory grant does not allow reads",
                    ));
                }
                let selected = state
                    .storage
                    .resolve_directory_grant_import_sources(
                        Path::new(&grant.physical_path),
                        &input.source_paths,
                        import_mode,
                    )
                    .map_err(ApiError::from_storage)?;
                (selected, grant.label, Some(grant.id))
            } else {
                if !input.source_paths.is_empty() {
                    return Err(ApiError::bad_request("sourcePaths are only valid with a saved sourceId; one-off import uses the native picker"));
                }
                let host = Arc::clone(&state.host_services);
                let app_name = application.display_name.clone();
                let picked = match input.mode {
                    NativeImportModeWire::Directory => {
                        let selected = tokio::task::spawn_blocking(move || {
                            host.choose_directory(&app_name, "one-off import source")
                        })
                        .await
                        .map_err(|_| ApiError::internal())?
                        .map_err(|message| {
                            ApiError::new(409, ErrorCode::DestinationGrantRequired, message)
                        })?;
                        selected.map(|path| vec![path])
                    }
                    NativeImportModeWire::File | NativeImportModeWire::Archive => {
                        tokio::task::spawn_blocking(move || {
                            host.choose_files(&app_name, "one-off import source", false)
                        })
                        .await
                        .map_err(|_| ApiError::internal())?
                        .map_err(|message| {
                            ApiError::new(409, ErrorCode::DestinationGrantRequired, message)
                        })?
                    }
                    NativeImportModeWire::Files => tokio::task::spawn_blocking(move || {
                        host.choose_files(&app_name, "one-off import sources", true)
                    })
                    .await
                    .map_err(|_| ApiError::internal())?
                    .map_err(|message| {
                        ApiError::new(409, ErrorCode::DestinationGrantRequired, message)
                    })?,
                };
                let selected = picked.ok_or_else(|| {
                    ApiError::new(
                        409,
                        ErrorCode::DestinationGrantRequired,
                        "no import source was selected",
                    )
                })?;
                let label = match input.mode {
                    NativeImportModeWire::Directory => "Selected folder",
                    NativeImportModeWire::Files => "Selected files",
                    NativeImportModeWire::Archive => "Selected ZIP archive",
                    NativeImportModeWire::File => "Selected file",
                }
                .to_owned();
                (selected, label, None)
            };
            let (snapshot, cancel) = state.begin_application_operation(
                &session.application_id,
                &input.operation,
                "native-import",
                "preparing",
                true,
                None,
                None,
            )?;
            let operation_id = snapshot.id.clone();
            let state2 = Arc::clone(state);
            let storage = Arc::clone(&state.storage);
            let space_id = input.space_id.clone();
            let app_name = application.display_name.clone();
            let host = Arc::clone(&state.host_services);
            let target_path = input.target_path.clone();
            let conflict = import_conflict_from_wire(input.conflict);
            let request_id = input.request_id.clone();
            let application_id = session.application_id.clone();
            tokio::spawn(async move {
                let state_progress = Arc::clone(&state2);
                let op_progress = operation_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    storage.native_import(
                        &space_id,
                        &selected_sources,
                        &source_label,
                        import_mode,
                        &target_path,
                        conflict,
                        &request_id,
                        || cancel.load(Ordering::SeqCst),
                        |items, total_items, bytes, total_bytes| {
                            state_progress.update_operation_metrics(
                                &op_progress,
                                "importing",
                                items,
                                total_items,
                                bytes,
                                total_bytes,
                            )
                        },
                        |item| {
                            host.confirm_import_replace(&app_name, item)
                                .map_err(StorageError::RequestInvalid)
                        },
                    )
                })
                .await;
                match result {
                    Ok(Ok(report)) => {
                        if let Some(id) = grant_id.as_deref() {
                            let _ = state2.storage.touch_directory_grant(&application_id, id);
                        }
                        state2.complete_operation(
                            &operation_id,
                            "complete",
                            serde_json::to_value(report).ok(),
                        );
                    }
                    Ok(Err(StorageError::OperationCancelled)) => {
                        state2.cancelled_operation(&operation_id)
                    }
                    Ok(Err(error)) => state2.fail_operation_code(
                        &operation_id,
                        native_import_error_code(&error),
                        error.to_string(),
                    ),
                    Err(error) => state2
                        .fail_operation(&operation_id, format!("operation worker failed: {error}")),
                }
            });
            HttpResponse::json(202, &snapshot)
        }
        ("POST", "/v1/snapshots/create") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: CreateSnapshotRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            state.authorize_space(&session, &input.space_id)?;
            validate_operation_id(&input.operation.id)?;
            if state.has_active_stream_for_space(&input.space_id) {
                return Err(ApiError::new(
                    409,
                    ErrorCode::Conflict,
                    "storage has active streams; snapshot requires a stable space view",
                ));
            }
            state
                .lock_space_exclusive(&input.space_id, &input.operation.id)
                .map_err(|error| ApiError::new(409, ErrorCode::Conflict, error.to_string()))?;
            let (snapshot, cancel) = match state.begin_application_operation(
                &session.application_id,
                &input.operation,
                "snapshot-create",
                "snapshotting",
                true,
                None,
                None,
            ) {
                Ok(value) => value,
                Err(error) => {
                    state.unlock_space_exclusive(&input.space_id, &input.operation.id);
                    return Err(error);
                }
            };
            let operation_id = snapshot.id.clone();
            let space_id = input.space_id.clone();
            let label = input.label.clone();
            let state2 = Arc::clone(state);
            let storage = Arc::clone(&state.storage);
            tokio::spawn(async move {
                let state_progress = Arc::clone(&state2);
                let op_progress = operation_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    storage.create_snapshot(
                        &space_id,
                        label.as_deref(),
                        || cancel.load(Ordering::SeqCst),
                        |items, total_items, bytes, total_bytes| {
                            state_progress.update_operation_metrics(
                                &op_progress,
                                "snapshotting",
                                items,
                                total_items,
                                bytes,
                                total_bytes,
                            )
                        },
                    )
                })
                .await;
                match result {
                    Ok(Ok(record)) => state2.complete_operation(
                        &operation_id,
                        "complete",
                        serde_json::to_value(snapshot_wire(&record)).ok(),
                    ),
                    Ok(Err(StorageError::OperationCancelled)) => {
                        state2.cancelled_operation(&operation_id)
                    }
                    Ok(Err(error)) => state2.fail_operation_code(
                        &operation_id,
                        snapshot_error_code(&error),
                        error.to_string(),
                    ),
                    Err(error) => state2
                        .fail_operation(&operation_id, format!("operation worker failed: {error}")),
                }
                state2.unlock_space_exclusive(&input.space_id, &operation_id);
            });
            HttpResponse::json(202, &snapshot)
        }
        ("POST", "/v1/snapshots/list") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: ListSnapshotsRequest = parse_json(&request.body)?;
            state.authorize_space(&session, &input.space_id)?;
            let snapshots = state
                .storage
                .list_snapshots(&input.space_id)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &ListSnapshotsResponse {
                    snapshots: snapshots.iter().map(snapshot_wire).collect(),
                },
            )
        }
        ("POST", "/v1/snapshots/restore") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: RestoreSnapshotRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            validate_operation_id(&input.operation.id)?;
            if state
                .storage
                .get_snapshot(&input.space_id, &input.snapshot_id)
                .map_err(ApiError::from_storage)?
                .is_none()
            {
                return Err(ApiError::new(
                    404,
                    ErrorCode::SnapshotNotFound,
                    "snapshot not found",
                ));
            }
            if state.has_active_stream_for_space(&input.space_id) {
                return Err(ApiError::new(
                    409,
                    ErrorCode::SnapshotRestoreConflict,
                    "storage has active streams; snapshot restore requires exclusive access",
                ));
            }
            state
                .lock_space_exclusive(&input.space_id, &input.operation.id)
                .map_err(|error| {
                    ApiError::new(409, ErrorCode::SnapshotRestoreConflict, error.to_string())
                })?;
            let (snapshot, cancel) = match state.begin_application_operation(
                &session.application_id,
                &input.operation,
                "snapshot-restore",
                "staging",
                true,
                None,
                None,
            ) {
                Ok(value) => value,
                Err(error) => {
                    state.unlock_space_exclusive(&input.space_id, &input.operation.id);
                    return Err(error);
                }
            };
            let operation_id = snapshot.id.clone();
            let space_id = input.space_id.clone();
            let snapshot_id = input.snapshot_id.clone();
            let application_id = space.owner_application_id.clone();
            let state2 = Arc::clone(state);
            let storage = Arc::clone(&state.storage);
            tokio::spawn(async move {
                let state_progress = Arc::clone(&state2);
                let op_progress = operation_id.clone();
                let work_space_id = space_id.clone();
                let result = tokio::task::spawn_blocking(move || {
                    storage.restore_snapshot(
                        &work_space_id,
                        &snapshot_id,
                        || cancel.load(Ordering::SeqCst),
                        |phase, items, total_items, bytes, total_bytes, cancellable| {
                            state_progress.set_operation_cancellable(&op_progress, cancellable);
                            state_progress.update_operation_metrics(
                                &op_progress,
                                phase,
                                items,
                                total_items,
                                bytes,
                                total_bytes,
                            );
                        },
                    )
                })
                .await;
                match result {
                    Ok(Ok(report)) => {
                        state2.emit_event(
                            &application_id,
                            EventKindWire::OverflowResyncRequired,
                            Some(&space_id),
                            Some("/"),
                            None,
                            None,
                            None,
                        );
                        state2.complete_operation(
                            &operation_id,
                            "complete",
                            serde_json::to_value(report).ok(),
                        );
                    }
                    Ok(Err(StorageError::OperationCancelled)) => {
                        state2.cancelled_operation(&operation_id)
                    }
                    Ok(Err(error)) => state2.fail_operation_code(
                        &operation_id,
                        snapshot_error_code(&error),
                        error.to_string(),
                    ),
                    Err(error) => state2
                        .fail_operation(&operation_id, format!("operation worker failed: {error}")),
                }
                state2.unlock_space_exclusive(&space_id, &operation_id);
            });
            HttpResponse::json(202, &snapshot)
        }
        ("POST", "/v1/snapshots/delete") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: DeleteSnapshotRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            state.authorize_space(&session, &input.space_id)?;
            if state.has_active_stream_for_space(&input.space_id) {
                return Err(ApiError::new(
                    409,
                    ErrorCode::Conflict,
                    "storage has active streams; snapshot deletion requires a stable space view",
                ));
            }
            let deleted = state
                .storage
                .delete_snapshot(&input.space_id, &input.snapshot_id)
                .map_err(ApiError::from_storage)?;
            if !deleted {
                return Err(ApiError::new(
                    404,
                    ErrorCode::SnapshotNotFound,
                    "snapshot not found",
                ));
            }
            HttpResponse::json(200, &serde_json::json!({"deleted": true}))
        }
        ("POST", "/v1/events/poll") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: EventPollRequest = parse_json(&request.body)?;
            let response = state.poll_events(&session, &input).await?;
            HttpResponse::json(200, &response)
        }
        ("POST", "/v1/fs/stat") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: FileStatRequest = parse_json(&request.body)?;
            state.authorize_space(&session, &input.space_id)?;
            let file = state
                .storage
                .stat_file(&input.space_id, &input.path)
                .map_err(ApiError::from_storage)?;
            HttpResponse::json(
                200,
                &FileStatResponse {
                    file: file.map(file_wire),
                },
            )
        }
        ("POST", "/v1/fs/read-small") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: SmallFileReadRequest = parse_json(&request.body)?;
            state.authorize_space(&session, &input.space_id)?;
            let metadata = state
                .storage
                .stat_file(&input.space_id, &input.path)
                .map_err(ApiError::from_storage)?
                .ok_or_else(|| ApiError::new(404, ErrorCode::NotFound, "file not found"))?;
            if metadata.size as usize > DIRECT_PAYLOAD_TARGET_BYTES {
                return Err(ApiError::new(
                    413,
                    ErrorCode::RequestTooLarge,
                    "file exceeds the direct-transfer threshold",
                ));
            }
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "read",
                    "reading",
                    false,
                    Some(metadata.size),
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, cancel)) = &operation {
                state.update_operation_bytes(&snapshot.id, "reading", 0, Some(metadata.size));
                if cancel.load(Ordering::SeqCst) {
                    state.cancelled_operation(&snapshot.id);
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
            }
            let bytes = match state.storage.read_file(&input.space_id, &input.path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    if let Some((snapshot, _)) = &operation {
                        state.fail_operation_code(
                            &snapshot.id,
                            storage_error_code(&error),
                            error.to_string(),
                        );
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            if bytes.len() > DIRECT_PAYLOAD_TARGET_BYTES {
                if let Some((snapshot, _)) = &operation {
                    state.fail_operation_code(
                        &snapshot.id,
                        "REQUEST_TOO_LARGE",
                        "file changed beyond the direct-transfer threshold".into(),
                    );
                }
                return Err(ApiError::new(
                    413,
                    ErrorCode::RequestTooLarge,
                    "file changed beyond the direct-transfer threshold",
                ));
            }
            if let Some((snapshot, _)) = &operation {
                state.update_operation_bytes(
                    &snapshot.id,
                    "reading",
                    bytes.len() as u64,
                    Some(metadata.size),
                );
                state.complete_operation(&snapshot.id, "complete", None);
            }
            HttpResponse::json(
                200,
                &SmallFileReadResponse {
                    data_base64: base64_encode(&bytes),
                    file: file_wire(metadata),
                },
            )
        }
        ("POST", "/v1/fs/write-small") => {
            require_control_content_type(&request)?;
            let session = state.authenticate(&request)?;
            let input: SmallFileWriteRequest = parse_json(&request.body)?;
            validate_request_id(&input.request_id)?;
            let space = state.authorize_space(&session, &input.space_id)?;
            state.ensure_no_active_write_conflict(&input.space_id, &input.path, false)?;
            let existed = state
                .storage
                .stat_file(&input.space_id, &input.path)
                .map_err(ApiError::from_storage)?
                .is_some();
            let bytes = base64_decode(&input.data_base64)
                .map_err(|_| ApiError::bad_request("dataBase64 is invalid"))?;
            if bytes.len() > DIRECT_PAYLOAD_TARGET_BYTES {
                return Err(ApiError::new(
                    413,
                    ErrorCode::RequestTooLarge,
                    "payload exceeds the direct-transfer threshold",
                ));
            }
            let operation = if let Some(requested) = input.operation.as_ref() {
                Some(state.begin_application_operation(
                    &session.application_id,
                    requested,
                    "write",
                    "writing",
                    false,
                    Some(bytes.len() as u64),
                    None,
                )?)
            } else {
                None
            };
            if let Some((snapshot, cancel)) = &operation {
                state.update_operation_bytes(&snapshot.id, "writing", 0, Some(bytes.len() as u64));
                if cancel.load(Ordering::SeqCst) {
                    state.cancelled_operation(&snapshot.id);
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
            }
            let metadata = input.metadata.as_ref().map(to_core_file_metadata);
            let result = match state.storage.write_file_with_metadata(
                &input.space_id,
                &input.path,
                &bytes,
                input.if_match.as_deref(),
                metadata.as_ref(),
                &input.request_id,
            ) {
                Ok(result) => result,
                Err(error) => {
                    if let Some((snapshot, _)) = &operation {
                        state.fail_operation_code(
                            &snapshot.id,
                            storage_error_code(&error),
                            error.to_string(),
                        );
                    }
                    return Err(ApiError::from_storage(error));
                }
            };
            if let Some((snapshot, _)) = &operation {
                state.update_operation_bytes(
                    &snapshot.id,
                    "writing",
                    bytes.len() as u64,
                    Some(bytes.len() as u64),
                );
                state.complete_operation(
                    &snapshot.id,
                    "complete",
                    Some(
                        serde_json::to_value(file_wire(result.clone()))
                            .unwrap_or(serde_json::Value::Null),
                    ),
                );
            }
            state.emit_event(
                &space.owner_application_id,
                if existed {
                    EventKindWire::FileChanged
                } else {
                    EventKindWire::FileCreated
                },
                Some(&input.space_id),
                Some(&input.path),
                None,
                None,
                None,
            );
            HttpResponse::json(200, &file_wire(result))
        }
        _ => Err(ApiError::new(
            404,
            ErrorCode::NotFound,
            "endpoint not found",
        )),
    }
}

async fn handle_write_stream_route(
    state: &Arc<RuntimeState>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let session = state.authenticate(&request)?;
    let tail = request
        .path
        .strip_prefix("/v1/streams/write/")
        .ok_or_else(|| ApiError::bad_request("invalid stream path"))?;
    let parts = tail.split('/').collect::<Vec<_>>();
    if request.method == "PUT" && parts.len() == 2 {
        require_binary_content_type(&request)?;
        if request.body.len() > MAX_STREAM_CHUNK_BYTES {
            return Err(ApiError::new(
                413,
                ErrorCode::RequestTooLarge,
                "stream chunk exceeds 512 KiB",
            ));
        }
        let stream_id = parts[0];
        let seq = parts[1].parse::<u64>().map_err(|_| {
            ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "stream sequence is invalid",
            )
        })?;
        let _bulk = state.acquire_bulk(&session.pairing_id).await?;
        let mut streams = state
            .write_streams
            .lock()
            .map_err(|_| ApiError::internal())?;
        let stream = streams.get_mut(stream_id).ok_or_else(|| {
            ApiError::new(
                404,
                ErrorCode::StreamNotFound,
                "write stream not found or expired",
            )
        })?;
        if stream.pairing_id != session.pairing_id {
            return Err(ApiError::new(
                403,
                ErrorCode::PermissionDenied,
                "stream belongs to another session",
            ));
        }
        if stream.committed.is_some() {
            return Err(ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "stream is already committed",
            ));
        }
        if let Some(operation_id) = stream.operation_id.as_deref() {
            if state.operation_cancelled(operation_id) {
                state.cancelled_operation(operation_id);
                return Err(ApiError::new(
                    409,
                    ErrorCode::Conflict,
                    "operation was cancelled",
                ));
            }
        }
        let chunk_hash = sha256_hex(&request.body);
        if seq + 1 == stream.next_seq
            && stream.last_chunk_sha256.as_deref() == Some(chunk_hash.as_str())
            && stream.last_chunk_len == request.body.len()
        {
            stream.last_activity_ms = now_ms();
            return HttpResponse::json(
                200,
                &StreamChunkAck {
                    stream_id: stream_id.to_owned(),
                    accepted_seq: seq,
                    next_seq: stream.next_seq,
                    received_bytes: stream.received_bytes,
                },
            );
        }
        if seq != stream.next_seq {
            return Err(ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                format!(
                    "expected stream sequence {}, received {seq}",
                    stream.next_seq
                ),
            ));
        }
        let next_size = stream
            .received_bytes
            .saturating_add(request.body.len() as u64);
        if next_size > MAX_MANAGED_FILE_BYTES {
            return Err(ApiError::new(
                413,
                ErrorCode::FileTooLarge,
                "stream exceeds the 16 GiB safety ceiling",
            ));
        }
        if stream
            .declared_size
            .is_some_and(|declared| next_size > declared)
        {
            return Err(ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "stream exceeded declared size",
            ));
        }
        state
            .storage
            .ensure_write_capacity(&stream.space_id, request.body.len() as u64)
            .map_err(ApiError::from_storage)?;
        let file = stream.file.as_mut().ok_or_else(|| {
            ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "stream is not writable",
            )
        })?;
        file.write_all(&request.body).map_err(|_| {
            ApiError::new(
                503,
                ErrorCode::StorageUnavailable,
                "failed to write stream chunk",
            )
        })?;
        stream.hasher.update(&request.body);
        stream.received_bytes = next_size;
        stream.last_chunk_sha256 = Some(chunk_hash);
        stream.last_chunk_len = request.body.len();
        stream.next_seq += 1;
        stream.last_activity_ms = now_ms();
        if let Some(operation_id) = stream.operation_id.as_deref() {
            state.update_operation_bytes(
                operation_id,
                "writing",
                stream.received_bytes,
                stream.declared_size,
            );
        }
        HttpResponse::json(
            200,
            &StreamChunkAck {
                stream_id: stream_id.to_owned(),
                accepted_seq: seq,
                next_seq: stream.next_seq,
                received_bytes: stream.received_bytes,
            },
        )
    } else if request.method == "POST" && parts.len() == 2 && parts[1] == "commit" {
        require_control_content_type(&request)?;
        let input: StreamWriteCommitRequest = parse_json(&request.body)?;
        if input.sha256.len() != 64 || !input.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApiError::bad_request(
                "sha256 must be a 64-character hex digest",
            ));
        }
        let stream_id = parts[0];
        let (application_id, space_id, path, existed_before, result, operation_id) = {
            let mut streams = state
                .write_streams
                .lock()
                .map_err(|_| ApiError::internal())?;
            let stream = streams.get_mut(stream_id).ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::StreamNotFound,
                    "write stream not found or expired",
                )
            })?;
            if stream.pairing_id != session.pairing_id {
                return Err(ApiError::new(
                    403,
                    ErrorCode::PermissionDenied,
                    "stream belongs to another session",
                ));
            }
            if let Some(operation_id) = stream.operation_id.as_deref() {
                if state.operation_cancelled(operation_id) {
                    state.cancelled_operation(operation_id);
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
            }
            if let Some(result) = stream.committed.clone() {
                return HttpResponse::json(200, &file_wire(result));
            }
            if stream
                .declared_size
                .is_some_and(|declared| declared != stream.received_bytes)
            {
                return Err(ApiError::new(
                    409,
                    ErrorCode::StreamSequenceInvalid,
                    "received bytes do not match declared size",
                ));
            }
            let actual = format!("{:x}", stream.hasher.clone().finalize());
            if !actual.eq_ignore_ascii_case(&input.sha256) {
                return Err(ApiError::new(
                    409,
                    ErrorCode::StreamChecksumMismatch,
                    "stream SHA-256 checksum does not match",
                ));
            }
            if let Some(mut file) = stream.file.take() {
                file.flush().map_err(|_| {
                    ApiError::new(503, ErrorCode::StorageUnavailable, "failed to flush stream")
                })?;
                file.sync_all().map_err(|_| {
                    ApiError::new(503, ErrorCode::StorageUnavailable, "failed to sync stream")
                })?;
            }
            let result = state
                .storage
                .commit_stream_file_with_metadata(
                    &stream.space_id,
                    &stream.path,
                    &stream.temp_path,
                    &actual,
                    stream.received_bytes,
                    stream.if_match.as_deref(),
                    stream.metadata.as_ref(),
                    &stream.request_id,
                )
                .map_err(ApiError::from_storage)?;
            stream.committed = Some(result.clone());
            stream.last_activity_ms = now_ms();
            (
                stream.application_id.clone(),
                stream.space_id.clone(),
                stream.path.clone(),
                stream.existed_before,
                result,
                stream.operation_id.clone(),
            )
        };
        if let Ok(mut paths) = state.active_write_paths.lock() {
            paths.remove(&write_path_key(&space_id, &path));
        }
        if let Some(operation_id) = operation_id.as_deref() {
            state.complete_operation(
                operation_id,
                "complete",
                Some(
                    serde_json::to_value(file_wire(result.clone()))
                        .unwrap_or(serde_json::Value::Null),
                ),
            );
        }
        state.emit_event(
            &application_id,
            if existed_before {
                EventKindWire::FileChanged
            } else {
                EventKindWire::FileCreated
            },
            Some(&space_id),
            Some(&path),
            None,
            None,
            None,
        );
        HttpResponse::json(200, &file_wire(result))
    } else if request.method == "POST" && parts.len() == 2 && parts[1] == "abort" {
        require_control_content_type(&request)?;
        let stream_id = parts[0];
        let mut stream = state
            .write_streams
            .lock()
            .map_err(|_| ApiError::internal())?
            .remove(stream_id)
            .ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::StreamNotFound,
                    "write stream not found or expired",
                )
            })?;
        if stream.pairing_id != session.pairing_id {
            state
                .write_streams
                .lock()
                .map_err(|_| ApiError::internal())?
                .insert(stream_id.to_owned(), stream);
            return Err(ApiError::new(
                403,
                ErrorCode::PermissionDenied,
                "stream belongs to another session",
            ));
        }
        if let Ok(mut paths) = state.active_write_paths.lock() {
            paths.remove(&write_path_key(&stream.space_id, &stream.path));
        }
        stream.file.take();
        if stream.committed.is_none() {
            state
                .storage
                .abort_stream_temp(&stream.space_id, &stream.temp_path)
                .map_err(ApiError::from_storage)?;
        }
        if let Some(operation_id) = stream.operation_id.as_deref() {
            state.cancelled_operation(operation_id);
        }
        Ok(HttpResponse::no_content())
    } else {
        Err(ApiError::new(
            404,
            ErrorCode::StreamNotFound,
            "stream endpoint not found",
        ))
    }
}

async fn handle_read_stream_route(
    state: &Arc<RuntimeState>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let session = state.authenticate(&request)?;
    let tail = request
        .path
        .strip_prefix("/v1/streams/read/")
        .ok_or_else(|| ApiError::bad_request("invalid stream path"))?;
    let parts = tail.split('/').collect::<Vec<_>>();
    if request.method == "GET" && parts.len() == 2 {
        let stream_id = parts[0];
        let seq = parts[1].parse::<u64>().map_err(|_| {
            ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "stream sequence is invalid",
            )
        })?;
        let _bulk = state.acquire_bulk(&session.pairing_id).await?;
        let (space_id, path, expected_etag, offset, advance, size, operation_id) = {
            let streams = state
                .read_streams
                .lock()
                .map_err(|_| ApiError::internal())?;
            let stream = streams.get(stream_id).ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::StreamNotFound,
                    "read stream not found or expired",
                )
            })?;
            if stream.pairing_id != session.pairing_id {
                return Err(ApiError::new(
                    403,
                    ErrorCode::PermissionDenied,
                    "stream belongs to another session",
                ));
            }
            if let Some(operation_id) = stream.operation_id.as_deref() {
                if state.operation_cancelled(operation_id) {
                    state.cancelled_operation(operation_id);
                    return Err(ApiError::new(
                        409,
                        ErrorCode::Conflict,
                        "operation was cancelled",
                    ));
                }
            }
            if seq > stream.next_seq {
                return Err(ApiError::new(
                    409,
                    ErrorCode::StreamSequenceInvalid,
                    format!(
                        "expected stream sequence {}, received {seq}",
                        stream.next_seq
                    ),
                ));
            }
            (
                stream.space_id.clone(),
                stream.path.clone(),
                stream.etag.clone(),
                seq.saturating_mul(DEFAULT_STREAM_CHUNK_BYTES as u64),
                seq == stream.next_seq,
                stream.size,
                stream.operation_id.clone(),
            )
        };
        if offset > size {
            return Err(ApiError::new(
                409,
                ErrorCode::StreamSequenceInvalid,
                "read stream sequence is past end of file",
            ));
        }
        let current = state
            .storage
            .stat_file(&space_id, &path)
            .map_err(ApiError::from_storage)?
            .ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::NotFound,
                    "file disappeared during stream read",
                )
            })?;
        if current.etag != expected_etag || current.size != size {
            return Err(ApiError::new(
                409,
                ErrorCode::Conflict,
                "file changed during stream read; reopen the reader",
            ));
        }
        let bytes = state
            .storage
            .read_file_range(&space_id, &path, offset, DEFAULT_STREAM_CHUNK_BYTES)
            .map_err(ApiError::from_storage)?;
        if advance {
            let mut streams = state
                .read_streams
                .lock()
                .map_err(|_| ApiError::internal())?;
            if let Some(stream) = streams.get_mut(stream_id) {
                stream.next_seq += 1;
                stream.last_activity_ms = now_ms();
            }
            if let Some(operation_id) = operation_id.as_deref() {
                let done = offset.saturating_add(bytes.len() as u64).min(size);
                state.update_operation_bytes(operation_id, "reading", done, Some(size));
                if done >= size {
                    state.complete_operation(operation_id, "complete", None);
                }
            }
        }
        Ok(HttpResponse::binary(bytes))
    } else if request.method == "POST" && parts.len() == 2 && parts[1] == "close" {
        require_control_content_type(&request)?;
        let stream_id = parts[0];
        let stream = state
            .read_streams
            .lock()
            .map_err(|_| ApiError::internal())?
            .remove(stream_id)
            .ok_or_else(|| {
                ApiError::new(
                    404,
                    ErrorCode::StreamNotFound,
                    "read stream not found or expired",
                )
            })?;
        if stream.pairing_id != session.pairing_id {
            state
                .read_streams
                .lock()
                .map_err(|_| ApiError::internal())?
                .insert(stream_id.to_owned(), stream);
            return Err(ApiError::new(
                403,
                ErrorCode::PermissionDenied,
                "stream belongs to another session",
            ));
        }
        if let Some(operation_id) = stream.operation_id.as_deref() {
            state.cancel_operation_if_active(operation_id);
        }
        Ok(HttpResponse::no_content())
    } else {
        Err(ApiError::new(
            404,
            ErrorCode::StreamNotFound,
            "stream endpoint not found",
        ))
    }
}

fn grant_capability_from_wire(value: DirectoryGrantCapabilityWire) -> DirectoryGrantCapability {
    match value {
        DirectoryGrantCapabilityWire::Read => DirectoryGrantCapability::Read,
        DirectoryGrantCapabilityWire::Write => DirectoryGrantCapability::Write,
        DirectoryGrantCapabilityWire::ReadWrite => DirectoryGrantCapability::ReadWrite,
    }
}
fn grant_capability_wire(value: DirectoryGrantCapability) -> DirectoryGrantCapabilityWire {
    match value {
        DirectoryGrantCapability::Read => DirectoryGrantCapabilityWire::Read,
        DirectoryGrantCapability::Write => DirectoryGrantCapabilityWire::Write,
        DirectoryGrantCapability::ReadWrite => DirectoryGrantCapabilityWire::ReadWrite,
    }
}
fn destination_grant_wire(value: &DirectoryGrantRecord) -> DestinationGrantWire {
    DestinationGrantWire {
        id: value.id.clone(),
        label: value.label.clone(),
        capability: grant_capability_wire(value.capability),
        status: if value.revoked_at_ms.is_some() {
            "revoked".into()
        } else {
            "available".into()
        },
        created_at_ms: value.created_at_ms,
        last_used_at_ms: value.last_used_at_ms,
    }
}
fn export_mode_from_wire(value: NativeExportModeWire) -> NativeExportMode {
    match value {
        NativeExportModeWire::File => NativeExportMode::File,
        NativeExportModeWire::Files => NativeExportMode::Files,
        NativeExportModeWire::Directory => NativeExportMode::Directory,
        NativeExportModeWire::Archive => NativeExportMode::Archive,
    }
}
fn export_mode_wire(value: NativeExportMode) -> NativeExportModeWire {
    match value {
        NativeExportMode::File => NativeExportModeWire::File,
        NativeExportMode::Files => NativeExportModeWire::Files,
        NativeExportMode::Directory => NativeExportModeWire::Directory,
        NativeExportMode::Archive => NativeExportModeWire::Archive,
    }
}
fn import_mode_from_wire(value: NativeImportModeWire) -> NativeImportMode {
    match value {
        NativeImportModeWire::File => NativeImportMode::File,
        NativeImportModeWire::Files => NativeImportMode::Files,
        NativeImportModeWire::Directory => NativeImportMode::Directory,
        NativeImportModeWire::Archive => NativeImportMode::Archive,
    }
}
fn import_conflict_from_wire(value: ImportConflictPolicyWire) -> ImportConflictPolicy {
    match value {
        ImportConflictPolicyWire::Replace => ImportConflictPolicy::Replace,
        ImportConflictPolicyWire::Skip => ImportConflictPolicy::Skip,
        ImportConflictPolicyWire::Rename => ImportConflictPolicy::Rename,
        ImportConflictPolicyWire::Ask => ImportConflictPolicy::Ask,
    }
}
fn export_conflict_from_wire(value: ExportConflictPolicyWire) -> ExportConflictPolicy {
    match value {
        ExportConflictPolicyWire::Replace => ExportConflictPolicy::Replace,
        ExportConflictPolicyWire::Skip => ExportConflictPolicy::Skip,
        ExportConflictPolicyWire::Rename => ExportConflictPolicy::Rename,
        ExportConflictPolicyWire::Ask => ExportConflictPolicy::Ask,
        ExportConflictPolicyWire::UpdateChanged => ExportConflictPolicy::UpdateChanged,
    }
}
fn export_conflict_wire(value: ExportConflictPolicy) -> ExportConflictPolicyWire {
    match value {
        ExportConflictPolicy::Replace => ExportConflictPolicyWire::Replace,
        ExportConflictPolicy::Skip => ExportConflictPolicyWire::Skip,
        ExportConflictPolicy::Rename => ExportConflictPolicyWire::Rename,
        ExportConflictPolicy::Ask => ExportConflictPolicyWire::Ask,
        ExportConflictPolicy::UpdateChanged => ExportConflictPolicyWire::UpdateChanged,
    }
}
fn export_preset_wire(value: &ExportPresetRecord) -> ExportPresetWire {
    ExportPresetWire {
        id: value.id.clone(),
        name: value.name.clone(),
        destination_id: value.destination_grant_id.clone(),
        mode: export_mode_wire(value.mode),
        conflict: export_conflict_wire(value.conflict_policy),
        source_path: value.source_path.clone(),
        archive_format: value.archive_format.clone(),
        created_at_ms: value.created_at_ms,
        updated_at_ms: value.updated_at_ms,
    }
}
fn snapshot_wire(value: &SnapshotRecord) -> SnapshotWire {
    SnapshotWire {
        id: value.id.clone(),
        space_id: value.space_id.clone(),
        created_at_ms: value.created_at_ms,
        label: value.label.clone(),
        logical_bytes: value.logical_bytes,
        file_count: value.file_count,
        source_generation: value.source_generation,
    }
}

fn operation_display_label(kind: &str, application_name: &str) -> String {
    let action = match kind {
        "write" => "Saving local data",
        "read" => "Reading local data",
        "copy" => "Copying local data",
        "move" => "Moving local data",
        "delete" => "Deleting local data",
        "export-space" | "native-export" => "Exporting data",
        "native-import" => "Importing data",
        "backup-space" => "Creating backup",
        "restore-backup" => "Restoring backup",
        "snapshot-create" => "Creating snapshot",
        "snapshot-restore" => "Restoring snapshot",
        "repair-space" => "Repairing storage",
        "reconcile-usage" => "Checking storage usage",
        "clear-cache" => "Clearing cache",
        "delete-space" => "Deleting storage",
        _ => "Working with local data",
    };
    if application_name == "VontaqFS" {
        action.into()
    } else {
        format!("{action} for {application_name}")
    }
}

fn write_path_key(space_id: &str, path: &str) -> String {
    format!("{space_id}\0{path}")
}

fn event_path_matches(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return path.starts_with('/');
    }
    path == prefix || path.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
}

fn validate_http_boundary(request: &HttpRequest) -> Result<(), ApiError> {
    if request.version != "HTTP/1.1" {
        return Err(ApiError::bad_request("HTTP/1.1 is required"));
    }
    let host = request
        .headers
        .get("host")
        .ok_or_else(|| ApiError::bad_request("Host header is required"))?;
    if !is_allowed_host(host) {
        return Err(ApiError::new(
            403,
            ErrorCode::PermissionDenied,
            "Host is not allowed",
        ));
    }
    if let Some(origin) = request.headers.get("origin") {
        if !is_allowed_origin(origin) {
            return Err(ApiError::new(
                403,
                ErrorCode::PermissionDenied,
                "Origin is not allowed",
            ));
        }
    }
    Ok(())
}

fn require_control_content_type(request: &HttpRequest) -> Result<(), ApiError> {
    let content_type = request
        .headers
        .get("content-type")
        .ok_or_else(|| ApiError::bad_request("Content-Type is required"))?;
    if content_type.trim().to_ascii_lowercase() != CONTROL_CONTENT_TYPE {
        return Err(ApiError::bad_request("unsupported Content-Type"));
    }
    Ok(())
}

fn require_binary_content_type(request: &HttpRequest) -> Result<(), ApiError> {
    let content_type = request
        .headers
        .get("content-type")
        .ok_or_else(|| ApiError::bad_request("Content-Type is required"))?;
    if !content_type
        .trim()
        .eq_ignore_ascii_case("application/octet-stream")
    {
        return Err(ApiError::bad_request(
            "binary stream chunks require application/octet-stream",
        ));
    }
    Ok(())
}

fn is_allowed_host(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    RUNTIME_ENDPOINT_PORTS.iter().any(|port| {
        normalized == format!("localhost:{port}")
            || normalized == format!("127.0.0.1:{port}")
            || normalized == format!("[::1]:{port}")
    })
}

fn is_allowed_origin(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "null" | "https://figma.com" | "https://www.figma.com"
    )
}

fn apply_cors(mut response: HttpResponse, origin: Option<&str>) -> HttpResponse {
    if let Some(origin) = origin.filter(|value| is_allowed_origin(value)) {
        response.allow_origin = Some(origin.to_owned());
    }
    response
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, ApiError> {
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end;
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| ApiError::bad_request("failed to read request"))?;
        if read == 0 {
            return Err(ApiError::bad_request(
                "connection closed before request completed",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEADER_BYTES + MAX_CONTROL_BODY_BYTES {
            return Err(ApiError::new(
                413,
                ErrorCode::RequestTooLarge,
                "request exceeds maximum size",
            ));
        }
        if let Some(index) = find_header_end(&buffer) {
            header_end = index;
            break;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(ApiError::new(
                431,
                ErrorCode::RequestTooLarge,
                "headers exceed maximum size",
            ));
        }
    }

    if header_end > MAX_HEADER_BYTES {
        return Err(ApiError::new(
            431,
            ErrorCode::RequestTooLarge,
            "headers exceed maximum size",
        ));
    }
    let header_bytes = &buffer[..header_end];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|_| ApiError::bad_request("headers are not valid UTF-8"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ApiError::bad_request("request line missing"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("method missing"))?
        .to_owned();
    let path = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("path missing"))?
        .to_owned();
    let version = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("HTTP version missing"))?
        .to_owned();
    if parts.next().is_some() || !path.starts_with('/') || path.contains("//") {
        return Err(ApiError::bad_request("request line is invalid"));
    }

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| ApiError::bad_request("malformed header"))?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.contains_key(&name) {
            return Err(ApiError::bad_request("duplicate or empty header"));
        }
        headers.insert(name, value.trim().to_owned());
    }
    if headers.contains_key("transfer-encoding") {
        return Err(ApiError::bad_request(
            "Transfer-Encoding is not supported on the control API",
        ));
    }
    let content_length = match headers.get("content-length") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request("Content-Length is invalid"))?,
        None => 0,
    };
    let is_binary = headers.get("content-type").is_some_and(|value| {
        value
            .trim()
            .eq_ignore_ascii_case("application/octet-stream")
    });
    let body_limit = if is_binary {
        MAX_STREAM_CHUNK_BYTES
    } else {
        MAX_CONTROL_BODY_BYTES
    };
    if content_length > body_limit {
        return Err(ApiError::new(
            413,
            ErrorCode::RequestTooLarge,
            if is_binary {
                "stream chunk exceeds 512 KiB"
            } else {
                "control body exceeds 1 MiB"
            },
        ));
    }

    let body_start = header_end + 4;
    while buffer.len().saturating_sub(body_start) < content_length {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| ApiError::bad_request("failed to read request body"))?;
        if read == 0 {
            return Err(ApiError::bad_request(
                "connection closed before request body completed",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len().saturating_sub(body_start) > body_limit {
            return Err(ApiError::new(
                413,
                ErrorCode::RequestTooLarge,
                if is_binary {
                    "stream chunk exceeds 512 KiB"
                } else {
                    "control body exceeds 1 MiB"
                },
            ));
        }
    }
    if buffer.len().saturating_sub(body_start) != content_length {
        buffer.truncate(body_start + content_length);
    }
    let body = buffer[body_start..body_start + content_length].to_vec();
    Ok(HttpRequest {
        method,
        path,
        version,
        headers,
        body,
    })
}

async fn write_response(
    stream: &mut TcpStream,
    response: HttpResponse,
) -> Result<(), std::io::Error> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nVary: Origin\r\n",
        response.status,
        reason,
        response.body.len()
    );
    if let Some(content_type) = response.content_type {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    if let Some(origin) = response.allow_origin {
        head.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
    }
    for (name, value) in response.extra_headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| ApiError::bad_request("control body is invalid JSON"))
}

fn validate_identity(identity: &ClientIdentity, client_instance_id: &str) -> Result<(), ApiError> {
    if identity.external_id.trim().is_empty() || identity.external_id.len() > 512 {
        return Err(ApiError::bad_request(
            "application externalId is empty or too long",
        ));
    }
    if identity.display_name.trim().is_empty() || identity.display_name.len() > 512 {
        return Err(ApiError::bad_request(
            "application displayName is empty or too long",
        ));
    }
    validate_client_instance_id(client_instance_id)
}

fn validate_client_instance_id(value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        return Err(ApiError::bad_request(
            "clientInstanceId is empty or too long",
        ));
    }
    Ok(())
}

fn validate_request_id(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(ApiError::bad_request("requestId format is invalid"));
    }
    Ok(())
}

fn validate_operation_id(value: &str) -> Result<(), ApiError> {
    if value.len() < 16
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApiError::bad_request("operationId format is invalid"));
    }
    Ok(())
}

fn validate_batch_file_metadata(metadata: Option<&FileMetadataWire>) -> Result<(), ApiError> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    for (name, value, max) in [
        ("contentType", metadata.content_type.as_deref(), 128usize),
        ("formatId", metadata.format_id.as_deref(), 64usize),
    ] {
        if let Some(value) = value {
            if value.trim().is_empty()
                || value.len() > max
                || value.chars().any(|ch| ch.is_control())
            {
                return Err(ApiError::bad_request(format!(
                    "batch {name} is empty, too long, or contains control characters"
                )));
            }
        }
    }
    if let Some(format_id) = metadata.format_id.as_deref() {
        if !format_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ApiError::bad_request(
                "batch formatId contains unsupported characters",
            ));
        }
    }
    Ok(())
}

fn validate_batch_kv(key: &str, value: &serde_json::Value) -> Result<(), ApiError> {
    if key.is_empty() || key.len() > 1024 || key.contains('\0') {
        return Err(ApiError::bad_request(
            "batch KV key is empty, too long, or contains NUL",
        ));
    }
    let encoded = serde_json::to_vec(value)
        .map_err(|_| ApiError::bad_request("batch KV value is not serializable"))?;
    if encoded.len() > 512 * 1024 {
        return Err(ApiError::new(
            413,
            ErrorCode::RequestTooLarge,
            "batch KV value exceeds 512 KiB",
        ));
    }
    Ok(())
}

fn to_core_kind(kind: ClientKind) -> ApplicationKind {
    match kind {
        ClientKind::FigmaPlugin => ApplicationKind::FigmaPlugin,
        ClientKind::FigmaWidget => ApplicationKind::FigmaWidget,
        ClientKind::OtherSupportedClient => ApplicationKind::OtherSupportedClient,
    }
}

fn to_core_storage_class(value: StorageClassWire) -> StorageClass {
    match value {
        StorageClassWire::Persistent => StorageClass::Persistent,
        StorageClassWire::Cache => StorageClass::Cache,
        StorageClassWire::Temporary => StorageClass::Temporary,
    }
}

fn to_wire_storage_class(value: StorageClass) -> StorageClassWire {
    match value {
        StorageClass::Persistent => StorageClassWire::Persistent,
        StorageClass::Cache => StorageClassWire::Cache,
        StorageClass::Temporary => StorageClassWire::Temporary,
    }
}

fn to_core_storage_category(value: StorageCategoryWire) -> StorageCategory {
    match value {
        StorageCategoryWire::UserData => StorageCategory::UserData,
        StorageCategoryWire::Generated => StorageCategory::Generated,
        StorageCategoryWire::Index => StorageCategory::Index,
        StorageCategoryWire::Backup => StorageCategory::Backup,
        StorageCategoryWire::Snapshot => StorageCategory::Snapshot,
        StorageCategoryWire::Custom => StorageCategory::Custom,
    }
}

fn to_wire_storage_category(value: StorageCategory) -> StorageCategoryWire {
    match value {
        StorageCategory::UserData => StorageCategoryWire::UserData,
        StorageCategory::Generated => StorageCategoryWire::Generated,
        StorageCategory::Index => StorageCategoryWire::Index,
        StorageCategory::Backup => StorageCategoryWire::Backup,
        StorageCategory::Snapshot => StorageCategoryWire::Snapshot,
        StorageCategory::Custom => StorageCategoryWire::Custom,
    }
}

fn space_wire(value: SpaceRecord) -> SpaceWire {
    SpaceWire {
        id: value.id,
        key: value.key,
        display_name: value.display_name,
        storage_class: to_wire_storage_class(value.storage_class),
        storage_category: to_wire_storage_category(value.storage_category),
        created_at_ms: value.created_at_ms,
        last_used_at_ms: value.last_used_at_ms,
        logical_bytes: value.logical_bytes,
        file_count: value.file_count,
        format_version: value.format_version,
        state: value.state,
    }
}

fn kv_wire(value: KvRecord) -> KvWire {
    KvWire {
        key: value.key,
        value: value.value,
        version: value.version,
        etag: value.etag,
        updated_at_ms: value.updated_at_ms,
    }
}

fn file_wire(value: FileRecord) -> FileWire {
    FileWire {
        path: value.path,
        version: value.version,
        etag: value.etag,
        size: value.size,
        updated_at_ms: value.updated_at_ms,
        content_type: value.content_type,
        format_id: value.format_id,
        opaque: value.opaque,
    }
}

fn to_core_file_metadata(value: &FileMetadataWire) -> FileMetadata {
    FileMetadata {
        content_type: value.content_type.clone(),
        format_id: value.format_id.clone(),
        opaque: value.opaque,
    }
}

fn format_wire(value: ApplicationFormatDescriptor) -> FormatDescriptorWire {
    FormatDescriptorWire {
        id: value.id,
        extension: value.extension,
        display_name: value.display_name,
        content_type: value.content_type,
        opaque: value.opaque,
        created_at_ms: value.created_at_ms,
        updated_at_ms: value.updated_at_ms,
    }
}

fn load_or_initialize_preferences(path: &Path) -> DesktopPreferences {
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(value) = serde_json::from_slice(&bytes) {
            return value;
        }
    }
    let value = DesktopPreferences::default();
    let _ = persist_preferences(path, &value);
    value
}
fn persist_preferences(path: &Path, preferences: &DesktopPreferences) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(preferences).map_err(std::io::Error::other)?;
    {
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(temp, path)?;
    Ok(())
}
fn configured_value(name: &str, compiled: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            compiled
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
}

fn sanitize_log_atom(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(80)
        .collect()
}
fn sanitize_error_message(value: &str) -> String {
    value
        .replace("Bearer ", "Bearer [redacted]")
        .chars()
        .take(512)
        .collect()
}

fn random_secret_hex(bytes: usize) -> String {
    if bytes == 0 {
        return String::new();
    }
    let mut seed = Vec::with_capacity(96);
    while seed.len() < bytes.saturating_mul(3) {
        seed.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    let digest = Sha256::digest(&seed);
    if bytes <= digest.len() {
        return hex_encode(&digest[..bytes]);
    }
    let mut out = Vec::with_capacity(bytes);
    let mut counter = 0_u64;
    while out.len() < bytes {
        let mut hasher = Sha256::new();
        hasher.update(&seed);
        hasher.update(counter.to_le_bytes());
        out.extend_from_slice(&hasher.finalize());
        counter = counter.wrapping_add(1);
    }
    out.truncate(bytes);
    hex_encode(&out)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(c & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(value: &str) -> Result<Vec<u8>, ()> {
    if value.len() % 4 != 0 {
        return Err(());
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        if c_pad && !d_pad {
            return Err(());
        }
        if (c_pad || d_pad) && !last {
            return Err(());
        }
        let c = if c_pad { 0 } else { base64_value(chunk[2])? };
        let d = if d_pad { 0 } else { base64_value(chunk[3])? };
        out.push((a << 2) | (b >> 4));
        if !c_pad {
            out.push((b << 4) | (c >> 2));
        }
        if !d_pad {
            out.push((c << 6) | d);
        }
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Result<u8, ()> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(()),
    }
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let offset = index * 2;
        *slot = u8::from_str_radix(&value[offset..offset + 2], 16).ok()?;
    }
    Some(out)
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let mut block = [0_u8; 64];
    if key.len() > block.len() {
        let digest = Sha256::digest(key);
        block[..32].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for index in 0..64 {
        inner_pad[index] ^= block[index];
        outer_pad[index] ^= block[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    hex_lower(&outer.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn request(method: &str, path: &str, body: &[u8]) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            path: path.into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::from([
                ("host".into(), "localhost:47833".into()),
                ("content-type".into(), CONTROL_CONTENT_TYPE.into()),
            ]),
            body: body.to_vec(),
        }
    }

    fn test_state() -> Arc<RuntimeState> {
        let root = std::env::temp_dir().join(format!("vontaqfs-runtime-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Arc::new(RuntimeState::new(
            StorageEngine::open(&root).unwrap(),
            root,
            RuntimeEndpointStatus {
                selected_port: Some(RUNTIME_ENDPOINT_PORTS[0]),
                occupied_ports: vec![],
                official_ports: RUNTIME_ENDPOINT_PORTS.to_vec(),
            },
            Arc::new(HeadlessHostServices),
        ))
    }

    #[test]
    fn host_and_origin_policy_is_narrow() {
        for port in RUNTIME_ENDPOINT_PORTS {
            assert!(is_allowed_host(&format!("localhost:{port}")));
            assert!(is_allowed_host(&format!("127.0.0.1:{port}")));
            assert!(is_allowed_host(&format!("[::1]:{port}")));
        }
        assert!(!is_allowed_host("localhost:47837"));
        assert!(!is_allowed_host("localhost:9999"));
        assert!(!is_allowed_host("evil.example:47833"));
        assert!(is_allowed_origin("null"));
        assert!(is_allowed_origin("https://www.figma.com"));
        assert!(!is_allowed_origin("https://evil.example"));
    }

    #[test]
    fn endpoint_candidate_order_prefers_persisted_official_fallback_without_accepting_random_ports()
    {
        let root =
            std::env::temp_dir().join(format!("vontaqfs-runtime-endpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            endpoint_candidate_order(&root),
            vec![47_833, 47_834, 47_835, 47_836]
        );
        persist_selected_endpoint(&root, 47_835).unwrap();
        assert_eq!(
            endpoint_candidate_order(&root),
            vec![47_835, 47_833, 47_834, 47_836]
        );
        assert!(persist_selected_endpoint(&root, 49_999).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pairing_bound_hmac_matches_the_locked_domain_contract() {
        let key = decode_hex_32("ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb")
            .unwrap();
        let nonce = "b".repeat(64);
        let message = [
            RUNTIME_IDENTITY_DOMAIN_SEPARATOR.as_bytes(),
            nonce.as_bytes(),
        ]
        .concat();
        assert_eq!(
            hmac_sha256_hex(&key, &message),
            "c679b06d2f148847db087959ad558712bcd6bcba218fa03042a6c2ed3a943c09"
        );
    }

    #[test]
    fn generated_credentials_are_256_bit_hex_and_hashes_are_not_plaintext() {
        let credential = random_secret_hex(32);
        assert_eq!(credential.len(), 64);
        let hash = sha256_hex(credential.as_bytes());
        assert_eq!(hash.len(), 64);
        assert_ne!(credential, hash);
    }

    #[tokio::test]
    async fn pairing_session_and_cross_application_isolation_work_without_public_admin_http() {
        let state = test_state();
        let handle = RuntimeHandle {
            state: Arc::clone(&state),
        };

        let first_body = serde_json::to_vec(&PairingRequest {
            application: ClientIdentity {
                kind: ClientKind::FigmaPlugin,
                external_id: "plugin-a".into(),
                display_name: "Plugin A".into(),
            },
            client_instance_id: "instance-a".into(),
        })
        .unwrap();
        let first_response =
            handle_request(&state, request("POST", "/v1/pairings/request", &first_body))
                .await
                .unwrap();
        let first: PairingRequestResponse = serde_json::from_slice(&first_response.body).unwrap();
        let approved = handle.approve_pairing(&first.pairing_id).unwrap();
        let poll = serde_json::to_vec(&PairingPollRequest {
            pairing_id: first.pairing_id.clone(),
        })
        .unwrap();
        let poll_response = handle_request(&state, request("POST", "/v1/pairings/poll", &poll))
            .await
            .unwrap();
        let polled: PairingPollResponse = serde_json::from_slice(&poll_response.body).unwrap();
        let credential = polled.pairing_credential.unwrap();
        let challenge_body = serde_json::to_vec(&RuntimeIdentityChallengeRequest {
            application: vontaqfs_protocol::RuntimeIdentityApplication {
                kind: ClientKind::FigmaPlugin,
                external_id: "plugin-a".into(),
            },
            client_instance_id: "instance-a".into(),
            nonce: "b".repeat(64),
        })
        .unwrap();
        let challenge_response = handle_request(
            &state,
            request("POST", "/v1/identity/challenge", &challenge_body),
        )
        .await
        .unwrap();
        let challenge: RuntimeIdentityChallengeResponse =
            serde_json::from_slice(&challenge_response.body).unwrap();
        let expected_key = decode_hex_32(&sha256_hex(credential.as_bytes())).unwrap();
        let expected_message = [
            RUNTIME_IDENTITY_DOMAIN_SEPARATOR.as_bytes(),
            "b".repeat(64).as_bytes(),
        ]
        .concat();
        assert_eq!(
            challenge.mac,
            hmac_sha256_hex(&expected_key, &expected_message)
        );

        let session_body = serde_json::to_vec(&SessionRequest {
            client_instance_id: "instance-a".into(),
            pairing_credential: credential,
        })
        .unwrap();
        let session_response =
            handle_request(&state, request("POST", "/v1/sessions", &session_body))
                .await
                .unwrap();
        let session: SessionResponse = serde_json::from_slice(&session_response.body).unwrap();

        let mut authed = request(
            "POST",
            "/v1/spaces/open",
            &serde_json::to_vec(&OpenSpaceRequest {
                request_id: "request-a".into(),
                key: "default".into(),
                storage_class: StorageClassWire::Persistent,
                storage_category: None,
                display_name: None,
            })
            .unwrap(),
        );
        authed
            .headers
            .insert("authorization".into(), format!("Bearer {}", session.token));
        let space_response = handle_request(&state, authed).await.unwrap();
        let owned_space: SpaceWire = serde_json::from_slice(&space_response.body).unwrap();

        let second_app = state
            .storage
            .register_application(ApplicationKind::FigmaPlugin, "plugin-b", "Plugin B")
            .unwrap();
        let second_space = state
            .storage
            .open_space(&second_app.id, "default", StorageClass::Persistent, None)
            .unwrap();
        let mut cross = request(
            "POST",
            "/v1/kv/get",
            &serde_json::to_vec(&KvGetRequest {
                space_id: second_space.id,
                key: "x".into(),
            })
            .unwrap(),
        );
        cross
            .headers
            .insert("authorization".into(), format!("Bearer {}", session.token));
        let error = handle_request(&state, cross).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);

        assert!(handle
            .revoke_pairing(&approved.persisted_pairing_id)
            .unwrap());
        assert_eq!(handle.active_session_count(), 0);
        let mut after_revoke = request(
            "POST",
            "/v1/kv/get",
            &serde_json::to_vec(&KvGetRequest {
                space_id: owned_space.id,
                key: "x".into(),
            })
            .unwrap(),
        );
        after_revoke
            .headers
            .insert("authorization".into(), format!("Bearer {}", session.token));
        let error = handle_request(&state, after_revoke).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthRevoked);
    }

    #[test]
    fn desktop_preferences_default_on_and_persist_across_runtime_restart() {
        let root =
            std::env::temp_dir().join(format!("vontaqfs-runtime-preferences-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let first = Arc::new(RuntimeState::new(
            StorageEngine::open(&root).unwrap(),
            root.clone(),
            RuntimeEndpointStatus {
                selected_port: Some(RUNTIME_ENDPOINT_PORTS[0]),
                occupied_ports: vec![],
                official_ports: RUNTIME_ENDPOINT_PORTS.to_vec(),
            },
            Arc::new(HeadlessHostServices),
        ));
        let handle = RuntimeHandle {
            state: Arc::clone(&first),
        };
        assert!(handle.desktop_preferences().unwrap().automatic_update_check);
        assert!(!handle.desktop_preferences().unwrap().launch_on_login);
        handle.set_automatic_update_check(false).unwrap();
        handle.set_launch_on_login_preference(true).unwrap();
        drop(handle);
        drop(first);

        let second = Arc::new(RuntimeState::new(
            StorageEngine::open(&root).unwrap(),
            root.clone(),
            RuntimeEndpointStatus {
                selected_port: Some(RUNTIME_ENDPOINT_PORTS[0]),
                occupied_ports: vec![],
                official_ports: RUNTIME_ENDPOINT_PORTS.to_vec(),
            },
            Arc::new(HeadlessHostServices),
        ));
        let handle = RuntimeHandle { state: second };
        let persisted = handle.desktop_preferences().unwrap();
        assert!(!persisted.automatic_update_check);
        assert!(persisted.launch_on_login);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn suspend_resume_invalidates_ephemeral_sessions_without_removing_pairings() {
        let state = test_state();
        let handle = RuntimeHandle {
            state: Arc::clone(&state),
        };
        let app = state
            .storage
            .register_application(ApplicationKind::FigmaPlugin, "plugin.sleep", "Sleep Plugin")
            .unwrap();
        let pairing = state
            .storage
            .create_pairing(&app.id, "sleep-client", &"d".repeat(64))
            .unwrap();
        state.sessions.lock().unwrap().insert(
            "a".repeat(64),
            SessionState {
                pairing_id: pairing.id.clone(),
                application_id: app.id.clone(),
                expires_at_ms: now_ms() + SESSION_TTL_MS,
            },
        );
        assert_eq!(handle.active_session_count(), 1);
        handle.suspend().unwrap();
        assert_eq!(handle.lifecycle_status(), RuntimeLifecycleStatus::Suspended);
        assert_eq!(handle.active_session_count(), 0);
        assert!(state
            .storage
            .pairing_by_id(&pairing.id)
            .unwrap()
            .unwrap()
            .revoked_at_ms
            .is_none());
        handle.resume().unwrap();
        assert_eq!(handle.lifecycle_status(), RuntimeLifecycleStatus::Running);
    }

    #[test]
    fn diagnostics_export_excludes_user_payload_and_pairing_hash_material() {
        let root =
            std::env::temp_dir().join(format!("vontaqfs-runtime-diagnostics-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let state = Arc::new(RuntimeState::new(
            StorageEngine::open(&root).unwrap(),
            root.clone(),
            RuntimeEndpointStatus {
                selected_port: Some(RUNTIME_ENDPOINT_PORTS[0]),
                occupied_ports: vec![],
                official_ports: RUNTIME_ENDPOINT_PORTS.to_vec(),
            },
            Arc::new(HeadlessHostServices),
        ));
        let handle = RuntimeHandle {
            state: Arc::clone(&state),
        };
        let app = state
            .storage
            .register_application(
                ApplicationKind::FigmaPlugin,
                "plugin.diagnostics",
                "Diagnostics Plugin",
            )
            .unwrap();
        let space = state
            .storage
            .open_space(&app.id, "default", StorageClass::Persistent, None)
            .unwrap();
        let payload_marker = "private-user-file-content-marker";
        let pairing_marker = "e".repeat(64);
        state
            .storage
            .write_file(
                &space.id,
                "/secret.txt",
                payload_marker.as_bytes(),
                None,
                "diag-payload",
            )
            .unwrap();
        state
            .storage
            .create_pairing(&app.id, "diag-client", &pairing_marker)
            .unwrap();
        state.audit_log("diagnostics-fixture", "ok");
        let destination = root.join("diagnostics.json");
        handle.export_diagnostics(&destination).unwrap();
        let exported = fs::read_to_string(destination).unwrap();
        assert!(exported.contains("vontaqfs-diagnostics"));
        assert!(!exported.contains(payload_marker));
        assert!(!exported.contains(&pairing_marker));
        assert!(!exported.contains("secret.txt"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn application_operations_are_owned_and_preserve_real_progress_through_cancel() {
        let state = test_state();
        let request = OperationRequestWire {
            id: "operation-owned-progress-0001".into(),
            presentation: OperationPresentationWire::Client,
        };
        let (snapshot, cancel) = state
            .begin_application_operation(
                "application-a",
                &request,
                "write",
                "writing",
                true,
                Some(10),
                None,
            )
            .unwrap();
        assert_eq!(snapshot.status, LongOperationStatus::Queued);
        assert_eq!(snapshot.bytes_total, Some(10));
        state.update_operation_bytes(&snapshot.id, "writing", 5, Some(10));
        let running = state
            .operation_status_for_application("application-a", &snapshot.id)
            .unwrap();
        assert_eq!(running.bytes_completed, Some(5));
        assert_eq!(running.bytes_total, Some(10));
        assert!(running.throughput_bytes_per_second.is_some());
        let foreign = state
            .operation_status_for_application("application-b", &snapshot.id)
            .unwrap_err();
        assert_eq!(foreign.code, ErrorCode::OperationNotOwned);
        let cancelling = state
            .cancel_application_operation("application-a", &snapshot.id)
            .unwrap();
        assert_eq!(cancelling.status, LongOperationStatus::Cancelling);
        assert!(cancel.load(Ordering::SeqCst));
        state.cancelled_operation(&snapshot.id);
        let cancelled = state
            .operation_status_for_application("application-a", &snapshot.id)
            .unwrap();
        assert_eq!(cancelled.status, LongOperationStatus::Cancelled);
        assert_eq!(cancelled.bytes_completed, Some(5));
        assert_eq!(cancelled.bytes_total, Some(10));
    }

    #[test]
    fn vontaqfs_presented_operations_use_runtime_labels_and_require_terminal_dismissal() {
        let state = test_state();
        let handle = RuntimeHandle {
            state: Arc::clone(&state),
        };
        let app = state
            .storage
            .register_application(
                ApplicationKind::FigmaPlugin,
                "plugin.operations",
                "Operations Plugin",
            )
            .unwrap();
        let request = OperationRequestWire {
            id: "operation-desktop-window-0001".into(),
            presentation: OperationPresentationWire::Vontaqfs,
        };
        let (snapshot, _) = state
            .begin_application_operation(
                &app.id,
                &request,
                "write",
                "writing",
                true,
                Some(20),
                None,
            )
            .unwrap();
        state.update_operation_bytes(&snapshot.id, "writing", 5, Some(20));
        let rows = handle.desktop_operations().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].application_name, "Operations Plugin");
        assert_eq!(rows[0].label, "Saving local data for Operations Plugin");
        assert_eq!(rows[0].operation.bytes_completed, Some(5));
        assert!(!handle.dismiss_desktop_operation(&snapshot.id).unwrap());
        state.fail_operation_code(&snapshot.id, "TEST_FAILURE", "sanitized failure".into());
        assert_eq!(handle.desktop_operations().unwrap().len(), 1);
        assert!(handle.dismiss_desktop_operation(&snapshot.id).unwrap());
        assert!(handle.desktop_operations().unwrap().is_empty());
    }

    #[test]
    fn application_operation_does_not_invent_unknown_totals_and_respects_non_cancellable_boundary()
    {
        let state = test_state();
        let request = OperationRequestWire {
            id: "operation-unknown-total-0001".into(),
            presentation: OperationPresentationWire::Silent,
        };
        let (snapshot, _) = state
            .begin_application_operation(
                "application-a",
                &request,
                "delete",
                "deleting",
                false,
                None,
                None,
            )
            .unwrap();
        state.update_operation_items(&snapshot.id, "deleting", 3, None);
        let progress = state
            .operation_status_for_application("application-a", &snapshot.id)
            .unwrap();
        assert_eq!(progress.items_completed, Some(3));
        assert_eq!(progress.items_total, None);
        assert_eq!(progress.total, None);
        assert_eq!(progress.eta_ms, None);
        let error = state
            .cancel_application_operation("application-a", &snapshot.id)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::OperationNotCancellable);
    }

    #[test]
    fn shutdown_enters_drain_and_emits_runtime_shutting_down_event() {
        let state = test_state();
        assert_eq!(state.lifecycle_status(), RuntimeLifecycleStatus::Running);
        state.begin_shutdown();
        assert_eq!(state.lifecycle_status(), RuntimeLifecycleStatus::Draining);
        assert!(state.shutdown_requested.load(Ordering::SeqCst));
        let events = state.events.lock().unwrap();
        assert!(events.iter().any(|item| item.application_id == "*"
            && item.event.event_type == EventKindWire::RuntimeShuttingDown));
    }

    #[test]
    fn public_routes_do_not_include_admin_pairing_mutations() {
        let source = include_str!("lib.rs");
        let public_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!public_source.contains("/v1/admin"));
        assert!(!public_source.contains("/v1/pairings/approve"));
        assert!(!public_source.contains("/v1/pairings/revoke"));
    }

    #[test]
    fn foundation_runtime_does_not_claim_network_storage_capabilities_yet() {
        let health = RuntimeFoundation.health();
        assert!(health.capabilities.is_empty());
        assert_eq!(health.status, "starting");
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_MIN: u16 = 1;
pub const PROTOCOL_MAX: u16 = 1;
pub const STORAGE_FORMAT_VERSION: u32 = 1;
pub const RUNTIME_ENDPOINT_PORTS: [u16; 4] = [47_833, 47_834, 47_835, 47_836];
pub const RUNTIME_IDENTITY_DOMAIN_SEPARATOR: &str = "vontaqfs-runtime-challenge-v1";
pub const SERVICE_MARKER: &str = "vontaqfs";
pub const CONTROL_CONTENT_TYPE: &str = "application/vnd.vontaqfs+json; version=1";
pub const MAX_CONTROL_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_HEADER_BYTES: usize = 32 * 1024;
pub const DIRECT_PAYLOAD_TARGET_BYTES: usize = 256 * 1024;
pub const PAIRING_TTL_MS: i64 = 5 * 60 * 1000;
pub const SESSION_TTL_MS: i64 = 15 * 60 * 1000;
pub const MAX_PENDING_PAIRINGS: usize = 16;
pub const MAX_ACTIVE_SESSIONS: usize = 32;
pub const MAX_ACTIVE_REQUESTS: usize = 32;
pub const MAX_ACTIVE_BULK_REQUESTS: usize = 8;
pub const MAX_BULK_REQUESTS_PER_SESSION: usize = 2;
pub const MAX_STREAM_WRITES_PER_SESSION: usize = 4;
pub const MAX_STREAM_CHUNK_BYTES: usize = 512 * 1024;
pub const DEFAULT_STREAM_CHUNK_BYTES: usize = 256 * 1024;
pub const MATERIALIZATION_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_MANAGED_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const STREAM_IDLE_TIMEOUT_MS: i64 = 60 * 1000;
pub const EVENT_LONG_POLL_MAX_MS: u64 = 25 * 1000;
pub const EVENT_BUFFER_CAPACITY: usize = 2048;
pub const MAX_BATCH_OPERATIONS: usize = 64;
pub const MAX_BATCH_PAYLOAD_BYTES: usize = 512 * 1024;
pub const READ_MANY_MAX_ITEMS: usize = 64;
pub const READ_MANY_MAX_ITEM_BYTES: usize = DIRECT_PAYLOAD_TARGET_BYTES;
pub const READ_MANY_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub const CAPABILITY_POST_CHUNKS: &str = "chunk-upload-post";
pub const CAPABILITY_STREAM_STATUS: &str = "stream-status";
pub const CAPABILITY_BULK_READ: &str = "bulk-read";
pub const CAPABILITY_SPACE_CLEAR: &str = "space-clear";
pub const CAPABILITY_DESTINATION_PICKER_HINTS: &str = "destination-picker-hints";
pub const CAPABILITY_INTERNAL_EXPORT_BOOKKEEPING: &str = "internal-export-bookkeeping";
pub const CAPABILITY_TRACKED_EXPORT_PRUNE: &str = "tracked-export-prune";
pub const CAPABILITY_DIRECTORY_CONTENTS_EXPORT: &str = "directory-contents-export";

// Inert durable-operation vocabulary; no v3 capability is advertised yet.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MutationDisposition {
    Applied,
    Replayed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DurableOperationPhase {
    Admitted,
    Running,
    Reconciling,
    Committed,
    Aborted,
    PartiallyApplied,
    NeedsRepair,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DurableOperationItemPhase {
    Pending,
    Running,
    Committed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DurableReplayNegotiation {
    pub identity_version: u32,
    pub server_time_ms: i64,
    pub min_replay_window_ms: i64,
    pub max_future_skew_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableOperationItemOutcome {
    pub item_index: u32,
    pub item_fingerprint: String,
    pub phase: DurableOperationItemPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableMutationOutcome {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_fingerprint: Option<String>,
    pub phase: DurableOperationPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<MutationDisposition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<DurableOperationItemOutcome>>,
    #[serde(default)]
    pub cleanup_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolRange {
    pub min: u16,
    pub max: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub service: String,
    pub runtime_version: String,
    pub protocol: ProtocolRange,
    pub storage_format_version: u32,
    pub status: String,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_replay: Option<DurableReplayNegotiation>,
}

impl HealthResponse {
    pub fn foundation(runtime_version: impl Into<String>) -> Self {
        Self {
            service: SERVICE_MARKER.to_owned(),
            runtime_version: runtime_version.into(),
            protocol: ProtocolRange {
                min: PROTOCOL_MIN,
                max: PROTOCOL_MAX,
            },
            storage_format_version: STORAGE_FORMAT_VERSION,
            status: "starting".to_owned(),
            capabilities: Vec::new(),
            durable_replay: None,
        }
    }

    pub fn ready(runtime_version: impl Into<String>) -> Self {
        Self {
            service: SERVICE_MARKER.to_owned(),
            runtime_version: runtime_version.into(),
            protocol: ProtocolRange {
                min: PROTOCOL_MIN,
                max: PROTOCOL_MAX,
            },
            storage_format_version: STORAGE_FORMAT_VERSION,
            status: "ready".to_owned(),
            durable_replay: None,
            capabilities: vec![
                "files".into(),
                "kv".into(),
                "spaces".into(),
                "streams".into(),
                CAPABILITY_POST_CHUNKS.into(),
                CAPABILITY_STREAM_STATUS.into(),
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
                CAPABILITY_BULK_READ.into(),
                CAPABILITY_SPACE_CLEAR.into(),
                CAPABILITY_DESTINATION_PICKER_HINTS.into(),
                CAPABILITY_INTERNAL_EXPORT_BOOKKEEPING.into(),
                CAPABILITY_TRACKED_EXPORT_PRUNE.into(),
                CAPABILITY_DIRECTORY_CONTENTS_EXPORT.into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    FigmaPlugin,
    FigmaWidget,
    OtherSupportedClient,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientIdentity {
    pub kind: ClientKind,
    pub external_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub application: ClientIdentity,
    pub client_instance_id: String,
    #[serde(default)]
    pub supported_chunk_methods: Option<Vec<ChunkMethod>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChunkMethod {
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "POST")]
    Post,
}

fn default_chunk_method() -> ChunkMethod {
    ChunkMethod::Put
}

impl ChunkMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Put => "PUT",
            Self::Post => "POST",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PairingStatus {
    Pending,
    Approved,
    Denied,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequestResponse {
    pub pairing_id: String,
    pub status: PairingStatus,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingPollRequest {
    pub pairing_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingPollResponse {
    pub pairing_id: String,
    pub status: PairingStatus,
    pub expires_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityApplication {
    pub kind: ClientKind,
    pub external_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityChallengeRequest {
    pub application: RuntimeIdentityApplication,
    pub client_instance_id: String,
    pub nonce: String,
    /// Strictly read-only challenge for an already deleted application.
    #[serde(default)]
    pub history_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityChallengeResponse {
    pub mac: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRequest {
    pub client_instance_id: String,
    pub pairing_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub token: String,
    pub expires_at_ms: i64,
    pub application_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_chunk_method: Option<ChunkMethod>,
    pub capabilities: Vec<String>,
    /// Present only when every advertised mutation class has durable replay,
    /// capacity admission, authenticated result lookup and the retention floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_replay: Option<DurableReplayNegotiation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageClassWire {
    Persistent,
    Cache,
    Temporary,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StorageCategoryWire {
    UserData,
    Generated,
    Index,
    Backup,
    Snapshot,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenSpaceRequest {
    pub request_id: String,
    pub key: String,
    pub storage_class: StorageClassWire,
    #[serde(default)]
    pub storage_category: Option<StorageCategoryWire>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceWire {
    pub id: String,
    pub key: String,
    pub display_name: Option<String>,
    pub storage_class: StorageClassWire,
    pub storage_category: StorageCategoryWire,
    pub created_at_ms: i64,
    pub last_used_at_ms: i64,
    pub logical_bytes: u64,
    pub file_count: u64,
    pub format_version: u32,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListSpacesResponse {
    pub spaces: Vec<SpaceWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClearSpaceRequest {
    pub request_id: String,
    pub space_id: String,
    #[serde(default)]
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClearSpaceResponse {
    pub space_id: String,
    pub deleted_files: u64,
    pub deleted_kv_entries: u64,
    pub released_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KvGetRequest {
    pub space_id: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KvWire {
    pub key: String,
    pub value: Value,
    pub version: u64,
    pub etag: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KvGetResponse {
    pub entry: Option<KvWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KvSetRequest {
    pub request_id: String,
    pub space_id: String,
    pub key: String,
    pub value: Value,
    pub if_version: Option<u64>,
    pub if_match: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileStatRequest {
    pub space_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadataWire {
    pub content_type: Option<String>,
    pub format_id: Option<String>,
    pub opaque: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationPresentationWire {
    Silent,
    Client,
    Vontaqfs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequestWire {
    pub id: String,
    pub presentation: OperationPresentationWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationStatusRequest {
    pub operation_id: String,
    /// Optional restricted proof for an archived application after deletion.
    /// Never creates a session or authorizes file/space operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_proof: Option<HistoricalStatusProof>,
    /// Optional ordinary SQL receipt scope. When present, operation_id is the
    /// original requestId; without it, the original long-job status contract applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_scope: Option<RetainedReceiptScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetainedReceiptScope {
    pub kind: RetainedReceiptScopeKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RetainedReceiptScopeKind {
    Space,
    Application,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetainedReceiptOutcome {
    pub scope_kind: RetainedReceiptScopeKind,
    pub scope_id: String,
    pub request_id: String,
    pub operation: String,
    pub result: Value,
    pub identity_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_fingerprint: Option<String>,
    pub created_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalStatusProof {
    pub client_instance_id: String,
    pub pairing_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationCancelRequest {
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileWire {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileStatResponse {
    pub file: Option<FileWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SmallFileReadRequest {
    pub space_id: String,
    pub path: String,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SmallFileReadResponse {
    pub data_base64: String,
    pub file: FileWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadManyRequest {
    pub space_id: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadManyItemResultWire {
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BatchItemErrorWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadManyResponse {
    pub results: Vec<ReadManyItemResultWire>,
    pub completed_items: u64,
    pub failed_items: u64,
    pub total_bytes: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SmallFileWriteRequest {
    pub request_id: String,
    pub space_id: String,
    pub path: String,
    pub data_base64: String,
    pub if_match: Option<String>,
    pub metadata: Option<FileMetadataWire>,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamWriteBeginRequest {
    #[serde(default = "default_chunk_method")]
    pub chunk_method: ChunkMethod,
    pub request_id: String,
    pub space_id: String,
    pub path: String,
    pub if_match: Option<String>,
    pub declared_size: Option<u64>,
    pub metadata: Option<FileMetadataWire>,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamWriteBeginResponse {
    pub chunk_method: ChunkMethod,
    pub request_id: String,
    pub stream_id: String,
    pub max_chunk_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunkAck {
    pub stream_id: String,
    pub accepted_seq: u64,
    pub next_seq: u64,
    pub received_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamWriteCommitRequest {
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamReadBeginRequest {
    pub space_id: String,
    pub path: String,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamReadBeginResponse {
    pub stream_id: String,
    pub file: FileWire,
    pub chunk_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FsDeleteRequest {
    pub request_id: String,
    pub space_id: String,
    pub path: String,
    pub recursive: Option<bool>,
    pub if_match: Option<String>,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FsCopyRequest {
    pub request_id: String,
    pub space_id: String,
    pub from: String,
    pub to: String,
    pub overwrite: Option<bool>,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FsMoveRequest {
    pub request_id: String,
    pub space_id: String,
    pub from: String,
    pub to: String,
    pub overwrite: Option<bool>,
    pub if_match: Option<String>,
    pub operation: Option<OperationRequestWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BatchOperationWire {
    WriteFile {
        #[serde(alias = "requestId")]
        request_id: String,
        path: String,
        #[serde(alias = "dataBase64")]
        data_base64: String,
        #[serde(alias = "ifMatch")]
        if_match: Option<String>,
        metadata: Option<FileMetadataWire>,
    },
    Delete {
        #[serde(alias = "requestId")]
        request_id: String,
        path: String,
        recursive: Option<bool>,
        #[serde(alias = "ifMatch")]
        if_match: Option<String>,
    },
    Copy {
        #[serde(alias = "requestId")]
        request_id: String,
        from: String,
        to: String,
        overwrite: Option<bool>,
    },
    Move {
        #[serde(alias = "requestId")]
        request_id: String,
        from: String,
        to: String,
        overwrite: Option<bool>,
        #[serde(alias = "ifMatch")]
        if_match: Option<String>,
    },
    KvSet {
        #[serde(alias = "requestId")]
        request_id: String,
        key: String,
        value: Value,
        #[serde(alias = "ifVersion")]
        if_version: Option<u64>,
        #[serde(alias = "ifMatch")]
        if_match: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub space_id: String,
    pub operations: Vec<BatchOperationWire>,
    pub operation: OperationRequestWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemErrorWire {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemResultWire {
    pub index: usize,
    pub operation_type: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv: Option<KvWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BatchItemErrorWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub results: Vec<BatchItemResultWire>,
    pub completed_items: u64,
    pub failed_items: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormatDescriptorWire {
    pub id: String,
    pub extension: Option<String>,
    pub display_name: String,
    pub content_type: Option<String>,
    pub opaque: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterFormatRequest {
    pub request_id: String,
    pub id: String,
    pub extension: Option<String>,
    pub display_name: String,
    pub content_type: Option<String>,
    pub opaque: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFormatRequest {
    pub request_id: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListFormatsResponse {
    pub formats: Vec<FormatDescriptorWire>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DirectoryGrantCapabilityWire {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DestinationGrantWire {
    pub id: String,
    pub label: String,
    pub capability: DirectoryGrantCapabilityWire,
    pub status: String,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateDestinationGrantRequest {
    pub request_id: String,
    pub label: Option<String>,
    pub capability: DirectoryGrantCapabilityWire,
    #[serde(default)]
    pub initial_destination_id: Option<String>,
    #[serde(default)]
    pub reuse_initial_if_same: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RevokeDestinationGrantRequest {
    pub request_id: String,
    pub destination_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListDestinationGrantsResponse {
    pub destinations: Vec<DestinationGrantWire>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeExportModeWire {
    File,
    Files,
    Directory,
    Archive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportConflictPolicyWire {
    Replace,
    Skip,
    Rename,
    Ask,
    UpdateChanged,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportBookkeepingPolicyWire {
    Destination,
    Internal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportPrunePolicyWire {
    None,
    Tracked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DirectoryExportLayoutWire {
    Preserve,
    Contents,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartNativeExportRequest {
    pub request_id: String,
    pub space_id: String,
    pub source_paths: Vec<String>,
    pub mode: NativeExportModeWire,
    pub destination_id: Option<String>,
    pub conflict: ExportConflictPolicyWire,
    pub archive_name: Option<String>,
    #[serde(default)]
    pub bookkeeping: Option<ExportBookkeepingPolicyWire>,
    #[serde(default)]
    pub prune: Option<ExportPrunePolicyWire>,
    #[serde(default)]
    pub tracking_key: Option<String>,
    #[serde(default)]
    pub directory_layout: Option<DirectoryExportLayoutWire>,
    pub operation: OperationRequestWire,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImportModeWire {
    File,
    Files,
    Directory,
    Archive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ImportConflictPolicyWire {
    Replace,
    Skip,
    Rename,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartNativeImportRequest {
    pub request_id: String,
    pub space_id: String,
    pub mode: NativeImportModeWire,
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_paths: Vec<String>,
    pub target_path: String,
    pub conflict: ImportConflictPolicyWire,
    pub operation: OperationRequestWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportPresetWire {
    pub id: String,
    pub name: String,
    pub destination_id: String,
    pub mode: NativeExportModeWire,
    pub conflict: ExportConflictPolicyWire,
    pub source_path: String,
    pub archive_format: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportPresetRequest {
    pub request_id: String,
    pub id: Option<String>,
    pub name: String,
    pub destination_id: String,
    pub mode: NativeExportModeWire,
    pub conflict: ExportConflictPolicyWire,
    pub source_path: String,
    pub archive_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteExportPresetRequest {
    pub request_id: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListExportPresetsResponse {
    pub presets: Vec<ExportPresetWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotWire {
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
pub struct CreateSnapshotRequest {
    pub request_id: String,
    pub space_id: String,
    pub label: Option<String>,
    pub operation: OperationRequestWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListSnapshotsRequest {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListSnapshotsResponse {
    pub snapshots: Vec<SnapshotWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSnapshotRequest {
    pub request_id: String,
    pub space_id: String,
    pub snapshot_id: String,
    pub operation: OperationRequestWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSnapshotRequest {
    pub request_id: String,
    pub space_id: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EventKindWire {
    FileCreated,
    FileChanged,
    FileDeleted,
    FileMoved,
    KvChanged,
    PermissionChanged,
    StorageRemoved,
    RuntimeShuttingDown,
    OverflowResyncRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEventWire {
    pub sequence: u64,
    pub event_type: EventKindWire,
    pub space_id: Option<String>,
    pub path: Option<String>,
    pub from_path: Option<String>,
    pub to_path: Option<String>,
    pub key: Option<String>,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventPollRequest {
    pub after_sequence: u64,
    pub space_id: Option<String>,
    pub path_prefix: Option<String>,
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventPollResponse {
    pub events: Vec<RuntimeEventWire>,
    pub latest_sequence: u64,
    pub overflow: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    RuntimeUnreachable,
    RuntimeStarting,
    RuntimeStopped,
    PortConflict,
    RuntimeIdentityMismatch,
    RuntimeIdentityAmbiguous,
    PairingRuntimeNotFound,
    ProtocolIncompatible,
    PairingRequired,
    PairingPending,
    PairingDenied,
    PairingExpired,
    AuthInvalid,
    AuthRevoked,
    PermissionDenied,
    SpaceNotFound,
    PathInvalid,
    PathConflict,
    NotFound,
    AlreadyExists,
    Conflict,
    RequestTooLarge,
    MaterializationLimit,
    FileTooLarge,
    QuotaExceeded,
    DiskSpaceLow,
    RateLimited,
    StreamNotFound,
    StreamSequenceInvalid,
    StreamChecksumMismatch,
    StreamExpired,
    StorageUnavailable,
    StorageCorrupt,
    RuntimeUpdating,
    RuntimeShuttingDown,
    CapabilityUnavailable,
    OperationNotFound,
    OperationLost,
    OperationNotOwned,
    OperationNotCancellable,
    DestinationGrantRequired,
    DestinationGrantRevoked,
    DestinationBusy,
    DestinationUnavailable,
    DestinationReadOnly,
    UserCancelled,
    ExportCancelled,
    ExportConflict,
    ExportSourceChanged,
    ImportCancelled,
    ArchiveUnsupported,
    SnapshotNotFound,
    SnapshotRestoreConflict,
    BatchCancelled,
    InternalError,
    RequestInvalid,
    ProtocolError,
    RequestIdConflict,
    OperationIdConflict,
    ResultExpired,
    OutcomeUnknown,
    Reconciling,
    PartiallyApplied,
    NeedsRepair,
    RecoveryRequired,
    RequestClockSkew,
    RequestIdMigrationRequired,
    StorageRootBusy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_contract_is_versioned_and_fixed_port() {
        let health = HealthResponse::ready("0.1.0");
        assert_eq!(health.service, "vontaqfs");
        assert_eq!(health.protocol, ProtocolRange { min: 1, max: 1 });
        assert_eq!(RUNTIME_ENDPOINT_PORTS, [47_833, 47_834, 47_835, 47_836]);
        assert_eq!(health.status, "ready");
        assert!(health.capabilities.contains(&"files".to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_BULK_READ.to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_SPACE_CLEAR.to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_DESTINATION_PICKER_HINTS.to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_INTERNAL_EXPORT_BOOKKEEPING.to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_TRACKED_EXPORT_PRUNE.to_owned()));
        assert!(health
            .capabilities
            .contains(&CAPABILITY_DIRECTORY_CONTENTS_EXPORT.to_owned()));
    }

    #[test]
    fn error_codes_use_stable_wire_names() {
        let encoded = serde_json::to_string(&ErrorCode::AuthInvalid).unwrap();
        assert_eq!(encoded, "\"AUTH_INVALID\"");
    }

    #[test]
    fn batch_sdk_camel_case_fields_round_trip_and_legacy_snake_case_still_read() {
        // The public SDK sends camelCase inside tagged operations. The enum's
        // rename_all changes variant names only, not their field names.
        let requests = [
            serde_json::json!({
                "type": "write-file", "requestId": "req-1", "path": "/x",
                "dataBase64": "YQ==", "ifMatch": null, "metadata": null
            }),
            serde_json::json!({
                "type": "delete", "requestId": "req-2", "path": "/x",
                "recursive": false, "ifMatch": null
            }),
            serde_json::json!({
                "type": "copy", "requestId": "req-3", "from": "/x",
                "to": "/y", "overwrite": false
            }),
            serde_json::json!({
                "type": "move", "requestId": "req-4", "from": "/x",
                "to": "/y", "overwrite": false, "ifMatch": null
            }),
            serde_json::json!({
                "type": "kv-set", "requestId": "req-5", "key": "k",
                "value": {"n": 1}, "ifVersion": 2, "ifMatch": null
            }),
        ];
        for value in requests {
            let parsed: BatchOperationWire = serde_json::from_value(value).unwrap();
            let canonical = serde_json::to_value(&parsed).unwrap();
            // Existing durable child fingerprints hash serde's snake_case bytes.
            // Aliases accept the public SDK without changing that saved shape.
            assert!(canonical.get("request_id").is_some());
            assert!(canonical.get("requestId").is_none());
            let replayed: BatchOperationWire = serde_json::from_value(canonical).unwrap();
            assert_eq!(parsed, replayed);
        }

        // Pre-existing snake_case wire payloads remain readable, without
        // weakening type validation or making fields optional.
        let legacy: BatchOperationWire = serde_json::from_value(serde_json::json!({
            "type": "write-file", "request_id": "req-6", "path": "/legacy",
            "data_base64": "Yg==", "if_match": null, "metadata": null
        }))
        .unwrap();
        let encoded = serde_json::to_value(legacy).unwrap();
        assert_eq!(encoded["request_id"], "req-6");
        assert_eq!(encoded["data_base64"], "Yg==");
        assert!(encoded.get("requestId").is_none());
        assert!(encoded.get("dataBase64").is_none());

        let legacy_kv: BatchOperationWire = serde_json::from_value(serde_json::json!({
            "type": "kv-set", "request_id": "req-7", "key": "k", "value": 1,
            "if_version": 3, "if_match": null
        }))
        .unwrap();
        assert_eq!(serde_json::to_value(legacy_kv).unwrap()["if_version"], 3);

        // Parse the complete request envelope as sent by writeTree().
        let full: BatchRequest = serde_json::from_value(serde_json::json!({
            "spaceId": "space-1",
            "operations": [{
                "type": "write-file", "requestId": "req-1", "path": "/x",
                "dataBase64": "YQ==", "ifMatch": null, "metadata": null
            }],
            "operation": {"id": "operation-1", "presentation": "client"}
        }))
        .unwrap();
        assert_eq!(full.operations.len(), 1);
    }

    #[test]
    fn v02_contracts_remain_protocol_v1_and_old_export_payload_deserializes() {
        assert_eq!(PROTOCOL_MIN, 1);
        assert_eq!(PROTOCOL_MAX, 1);
        assert_eq!(STORAGE_FORMAT_VERSION, 1);
        assert_eq!(CAPABILITY_BULK_READ, "bulk-read");
        assert_eq!(CAPABILITY_SPACE_CLEAR, "space-clear");
        assert_eq!(
            CAPABILITY_DESTINATION_PICKER_HINTS,
            "destination-picker-hints"
        );
        assert_eq!(
            CAPABILITY_INTERNAL_EXPORT_BOOKKEEPING,
            "internal-export-bookkeeping"
        );
        assert_eq!(CAPABILITY_TRACKED_EXPORT_PRUNE, "tracked-export-prune");
        assert_eq!(
            CAPABILITY_DIRECTORY_CONTENTS_EXPORT,
            "directory-contents-export"
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::CapabilityUnavailable).unwrap(),
            "\"CAPABILITY_UNAVAILABLE\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::UserCancelled).unwrap(),
            "\"USER_CANCELLED\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::OperationLost).unwrap(),
            "\"OPERATION_LOST\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::DestinationBusy).unwrap(),
            "\"DESTINATION_BUSY\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::ExportSourceChanged).unwrap(),
            "\"EXPORT_SOURCE_CHANGED\""
        );

        let request: StartNativeExportRequest = serde_json::from_value(serde_json::json!({
            "requestId": "request-1",
            "spaceId": "space-1",
            "sourcePaths": ["/out"],
            "mode": "directory",
            "destinationId": null,
            "conflict": "update-changed",
            "archiveName": null,
            "operation": { "id": "operation-1", "presentation": "client" }
        }))
        .unwrap();
        assert_eq!(request.bookkeeping, None);
        assert_eq!(request.prune, None);
        assert_eq!(request.tracking_key, None);
        assert_eq!(request.directory_layout, None);
    }
}

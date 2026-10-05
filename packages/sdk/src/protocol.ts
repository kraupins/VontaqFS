export const VONTAQ_FS_PROTOCOL_MIN = 1 as const;
export const VONTAQ_FS_PROTOCOL_MAX = 1 as const;
export const VONTAQ_FS_STORAGE_FORMAT_VERSION = 1 as const;
export const VONTAQ_FS_ENDPOINT_PORTS = [47833, 47834, 47835, 47836] as const;
export const VONTAQ_FS_ENDPOINTS = VONTAQ_FS_ENDPOINT_PORTS.map((port) => `http://localhost:${port}`) as readonly string[];
/** @deprecated Use VONTAQ_FS_ENDPOINTS; retained as the canonical primary endpoint. */
export const VONTAQ_FS_ENDPOINT = VONTAQ_FS_ENDPOINTS[0] as string;
export const VONTAQ_FS_RUNTIME_IDENTITY_DOMAIN_SEPARATOR = 'vontaqfs-runtime-challenge-v1' as const;
export const VONTAQ_FS_CONTROL_CONTENT_TYPE = 'application/vnd.vontaqfs+json; version=1' as const;
export const VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES = 256 * 1024;
export const VONTAQ_FS_STREAM_CHUNK_BYTES = 256 * 1024;
export const VONTAQ_FS_STREAM_CHUNK_MAX_BYTES = 512 * 1024;
export const VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES = 64 * 1024 * 1024;
export const VONTAQ_FS_MAX_FILE_BYTES = 16 * 1024 * 1024 * 1024;
export const VONTAQ_FS_EVENT_LONG_POLL_MAX_MS = 25_000;
export const VONTAQ_FS_MAX_BATCH_OPERATIONS = 64;
export const VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES = 512 * 1024;

export type ClientKind = 'figma-plugin' | 'figma-widget' | 'other-supported-client';
export type StorageClass = 'persistent' | 'cache' | 'temporary';
export type StorageCategory = 'user-data' | 'generated' | 'index' | 'backup' | 'snapshot' | 'custom';
export type PairingStatus = 'pending' | 'approved' | 'denied' | 'expired';

export type ProgressPresentation = 'silent' | 'client' | 'vontaqfs';
export type OperationStatus = 'queued' | 'running' | 'cancelling' | 'completed' | 'failed' | 'cancelled';

export interface OperationErrorInfo {
  code: string;
  message: string;
}

export interface OperationProgress {
  id: string;
  kind: string;
  phase: string;
  presentation: ProgressPresentation;
  status: OperationStatus;
  cancellable: boolean;
  completed?: number;
  total?: number;
  bytesCompleted?: number;
  bytesTotal?: number;
  itemsCompleted?: number;
  itemsTotal?: number;
  throughputBytesPerSecond?: number;
  etaMs?: number;
  startedAtMs: number;
  updatedAtMs: number;
  error?: OperationErrorInfo;
  result?: unknown;
}

export interface OperationRequestWire {
  id: string;
  presentation: ProgressPresentation;
}


export type DirectoryGrantCapability = 'read' | 'write' | 'read-write';
export interface DestinationGrant {
  id: string;
  label: string;
  capability: DirectoryGrantCapability;
  status: 'available' | 'revoked' | 'permission-lost' | 'missing' | 'read-only' | 'needs-user-action';
  createdAtMs: number;
  lastUsedAtMs?: number;
}

export type NativeExportMode = 'file' | 'files' | 'directory' | 'archive';
export type ExportConflictPolicy = 'replace' | 'skip' | 'rename' | 'ask' | 'update-changed';
export interface NativeExportReport {
  spaceId: string;
  destinationLabel: string;
  guarantee: 'directory-swap' | 'file-atomic' | 'best-effort';
  added: number;
  changed: number;
  skipped: number;
  unchanged: number;
  conflicts: number;
  deleted: number;
  exportedBytes: number;
  manifestWritten: boolean;
}

export type NativeImportMode = 'file' | 'files' | 'directory' | 'archive';
export type ImportConflictPolicy = 'replace' | 'skip' | 'rename' | 'ask';
export interface NativeImportReport {
  spaceId: string;
  sourceLabel: string;
  importedFiles: readonly FileInfo[];
  importedBytes: number;
  skipped: number;
  conflicts: number;
  archiveExtracted: boolean;
}

export interface SnapshotInfo {
  id: string;
  spaceId: string;
  createdAtMs: number;
  label?: string;
  logicalBytes: number;
  fileCount: number;
  sourceGeneration: number;
}

export interface SnapshotRestoreReport {
  snapshotId: string;
  spaceId: string;
  restoredFiles: number;
  restoredBytes: number;
  restoredKvEntries: number;
  guarantee: 'directory-swap';
}

export interface ExportPreset {
  id: string;
  name: string;
  destinationId: string;
  mode: NativeExportMode;
  conflict: ExportConflictPolicy;
  sourcePath: string;
  archiveFormat?: string;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface ClientIdentity {
  kind: ClientKind;
  externalId: string;
  displayName: string;
}

export interface RuntimeStatus {
  service: 'vontaqfs';
  runtimeVersion: string;
  protocol: { min: number; max: number };
  storageFormatVersion: number;
  status: 'starting' | 'ready' | 'stopping' | 'error';
  capabilities: readonly string[];
}

export interface PairingRequestResponse {
  pairingId: string;
  status: PairingStatus;
  expiresAtMs: number;
}

export interface PairingPollResponse extends PairingRequestResponse {
  pairingCredential?: string;
}

export interface RuntimeIdentityChallengeResponse {
  mac: string;
}

export interface SessionResponse {
  token: string;
  expiresAtMs: number;
  applicationId: string;
  capabilities: readonly string[];
}

export interface SpaceInfo {
  id: string;
  key: string;
  displayName?: string;
  storageClass: StorageClass;
  storageCategory: StorageCategory;
  createdAtMs: number;
  lastUsedAtMs: number;
  logicalBytes: number;
  fileCount: number;
  formatVersion: number;
  state: string;
}

export interface KeyValueEntry<T = unknown> {
  key: string;
  value: T;
  version: number;
  etag: string;
  updatedAtMs: number;
}

export interface FileMetadataInput {
  contentType?: string;
  formatId?: string;
  opaque?: boolean;
}

export interface FileInfo {
  path: string;
  version: number;
  etag: string;
  size: number;
  updatedAtMs: number;
  contentType?: string;
  formatId?: string;
  opaque?: boolean;
}

export interface FormatDescriptorInput {
  id: string;
  extension?: string;
  displayName: string;
  contentType?: string;
  opaque?: boolean;
}

export interface FormatDescriptor {
  id: string;
  extension?: string;
  displayName: string;
  contentType?: string;
  opaque: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export type BatchOperation =
  | { type: 'write-file'; path: string; bytes: Uint8Array; ifMatch?: string; metadata?: FileMetadataInput }
  | { type: 'delete'; path: string; recursive?: boolean; ifMatch?: string }
  | { type: 'copy'; from: string; to: string; overwrite?: boolean }
  | { type: 'move'; from: string; to: string; overwrite?: boolean; ifMatch?: string }
  | { type: 'kv-set'; key: string; value: unknown; ifVersion?: number; ifMatch?: string };

export interface BatchItemError {
  code: string;
  message: string;
}

export interface BatchItemResult {
  index: number;
  operationType: BatchOperation['type'];
  ok: boolean;
  file?: FileInfo;
  kv?: KeyValueEntry;
  affectedFiles?: number;
  error?: BatchItemError;
}

export interface BatchReport {
  results: readonly BatchItemResult[];
  completedItems: number;
  failedItems: number;
  cancelled: boolean;
}

export interface WriteTreeEntry {
  path: string;
  bytes: Uint8Array;
  ifMatch?: string;
  metadata?: FileMetadataInput;
}

export interface WriteTreeReport {
  files: readonly FileInfo[];
  completedItems: number;
  failedItems: number;
  totalBytes: number;
}

export interface VontaqFSCapabilities {
  readonly files: boolean;
  readonly kv: boolean;
  readonly spaces: boolean;
  readonly streams: boolean;
  readonly events: boolean;
  readonly formats: boolean;
  readonly operations: boolean;
  readonly nativeExport: boolean;
  readonly nativeImport: boolean;
  readonly savedDirectories: boolean;
  readonly exportPresets: boolean;
  readonly backup: boolean;
  readonly snapshots: boolean;
  readonly batch: boolean;
  readonly storageCategory: boolean;
  readonly systemProgressWindow: boolean;
  readonly raw: readonly string[];
}

export interface SmallFileReadResponse {
  dataBase64: string;
  file: FileInfo;
}

export interface RuntimeErrorResponse {
  error?: {
    code?: string;
    message?: string;
  };
}


export interface StreamWriteBeginResponse {
  streamId: string;
  maxChunkBytes: number;
}

export interface StreamChunkAck {
  streamId: string;
  acceptedSeq: number;
  nextSeq: number;
  receivedBytes: number;
}

export interface StreamReadBeginResponse {
  streamId: string;
  file: FileInfo;
  chunkSize: number;
}

export type RuntimeEventType =
  | 'file-created' | 'file-changed' | 'file-deleted' | 'file-moved'
  | 'kv-changed' | 'permission-changed' | 'storage-removed'
  | 'runtime-shutting-down' | 'overflow-resync-required';

export interface RuntimeEvent {
  sequence: number;
  eventType: RuntimeEventType;
  spaceId?: string;
  path?: string;
  fromPath?: string;
  toPath?: string;
  key?: string;
  atMs: number;
}

export interface EventPollResponse {
  events: RuntimeEvent[];
  latestSequence: number;
  overflow: boolean;
}

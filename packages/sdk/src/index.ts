export * from './errors.js';
export * from './protocol.js';

import { isVontaqFSError, normalizeErrorCode, VontaqFSError, type VontaqFSErrorCode } from './errors.js';
import { IncrementalSha256 } from './sha256.js';
import {
  VONTAQ_FS_CONTROL_CONTENT_TYPE,
  VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES,
  VONTAQ_FS_ENDPOINTS,
  VONTAQ_FS_RUNTIME_IDENTITY_DOMAIN_SEPARATOR,
  VONTAQ_FS_EVENT_LONG_POLL_MAX_MS,
  VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES,
  VONTAQ_FS_MAX_BATCH_OPERATIONS,
  VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES,
  VONTAQ_FS_MAX_FILE_BYTES,
  VONTAQ_FS_PROTOCOL_MAX,
  VONTAQ_FS_PROTOCOL_MIN,
  VONTAQ_FS_READ_MANY_MAX_ITEMS,
  VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES,
  VONTAQ_FS_STREAM_CHUNK_BYTES,
  VONTAQ_FS_STREAM_CHUNK_MAX_BYTES,
  type BatchOperation,
  type BatchReport,
  type ClientIdentity,
  type DestinationGrant,
  type DirectoryGrantCapability,
  type DirectoryExportLayout,
  type ExportBookkeepingPolicy,
  type ExportConflictPolicy,
  type ExportPrunePolicy,
  type ExportPreset,
  type ImportConflictPolicy,
  type NativeExportMode,
  type NativeExportReport,
  type NativeImportMode,
  type NativeImportReport,
  type EventPollResponse,
  type FileInfo,
  type FileMetadataInput,
  type FormatDescriptor,
  type FormatDescriptorInput,
  type KeyValueEntry,
  type OperationProgress,
  type OperationRequestWire,
  type ProgressPresentation,
  type PairingPollResponse,
  type PairingRequestResponse,
  type ReadManyReport,
  type ReadManyReportWire,
  type RuntimeErrorResponse,
  type RuntimeEvent,
  type RuntimeIdentityChallengeResponse,
  type RuntimeStatus,
  type SessionResponse,
  type SmallFileReadResponse,
  type SnapshotInfo,
  type SnapshotRestoreReport,
  type SpaceClearReport,
  type SpaceInfo,
  type StorageCategory,
  type StorageClass,
  type StreamChunkAck,
  VONTAQ_FS_CAPABILITY_IDS,
  type VontaqFSCapabilities,
  type WriteTreeEntry,
  type WriteTreeReport,
  type StreamReadBeginResponse,
  type StreamWriteBeginResponse,
} from './protocol.js';

const CLIENT_INSTANCE_KEY = 'vontaqfs.client-instance.v1';
const PAIRING_CREDENTIAL_KEY = 'vontaqfs.pairing-credential.v1';
const DEFAULT_SPACE_KEY = 'default';
const DEFAULT_PAIRING_POLL_INTERVAL_MS = 500;
const DEFAULT_REQUEST_TIMEOUT_MS = 10_000;
const SESSION_REFRESH_SKEW_MS = 5_000;
const TEXT_ENCODER_WINDOW_CODE_UNITS = 64 * 1024;
const MAX_WRITE_TREE_ENTRIES = 100_000;

export interface ClientStateStore {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  delete?(key: string): Promise<void>;
}

/** Explicitly forget only the saved pairing credential. Client/application identity and VFS data are preserved. */
export async function resetPairingState(stateStore: ClientStateStore): Promise<void> {
  if (!stateStore || typeof stateStore.get !== 'function' || typeof stateStore.set !== 'function') {
    throw new TypeError('resetPairingState() requires a ClientStateStore.');
  }
  if (typeof stateStore.delete === 'function') await stateStore.delete(PAIRING_CREDENTIAL_KEY);
  else await stateStore.set(PAIRING_CREDENTIAL_KEY, '');
}

export interface VontaqFSHttpRequest {
  method: 'GET' | 'POST' | 'PUT';
  url: string;
  headers: Readonly<Record<string, string>>;
  body?: string | Uint8Array;
  responseType?: 'text' | 'binary';
  timeoutMs: number;
}

export interface VontaqFSHttpResponse {
  status: number;
  body: string | Uint8Array;
  headers?: Readonly<Record<string, string>>;
}

export interface VontaqFSTransport {
  request(request: VontaqFSHttpRequest): Promise<VontaqFSHttpResponse>;
}

export interface PairingRequiredEvent {
  pairingId: string;
  expiresAtMs: number;
  application: ClientIdentity;
}

export interface VontaqFSConnectOptions {
  application: ClientIdentity;
  stateStore: ClientStateStore;
  /** Development/test-only endpoint override. Published plugins should use official discovery. */
  developmentEndpoint?: string;
  transport?: VontaqFSTransport;
  pairingPollIntervalMs?: number;
  pairingTimeoutMs?: number;
  requestTimeoutMs?: number;
  retryCount?: number;
  materializationLimitBytes?: number;
  onPairingRequired?: (event: PairingRequiredEvent) => void | Promise<void>;
}

export interface OpenSpaceOptions {
  key: string;
  storageClass?: StorageClass;
  storageCategory?: StorageCategory;
  displayName?: string;
}

export interface OperationOptions {
  progress?: {
    presentation?: ProgressPresentation;
    onProgress?: (progress: OperationProgress) => void | Promise<void>;
  };
  signal?: AbortSignal;
}

export interface BatchOptions extends OperationOptions {}

export interface WriteTreeOptions extends OperationOptions {
  /** Continue after per-file failures. Default false. Successful writes are never rolled back. */
  continueOnError?: boolean;
}

export interface WorkspaceOptions {
  displayName?: string;
}

export interface CreateDestinationOptions {
  label?: string;
  capability?: DirectoryGrantCapability;
  /** Opaque prior grant used only as a native picker initial-location hint when supported. */
  initialDestinationId?: string;
  /** Allows reuse only when the user selects the same canonical directory and the prior grant remains sufficient. */
  reuseInitialIfSame?: boolean;
}

export interface NativeExportOptions extends OperationOptions {
  mode: NativeExportMode;
  destinationId?: string;
  conflict?: ExportConflictPolicy;
  format?: 'zip';
  archiveName?: string;
  /** Defaults to destination for 0.1 compatibility. */
  bookkeeping?: ExportBookkeepingPolicy;
  /** Defaults to none. tracked only removes paths proven safe by the prior authoritative tracking manifest. */
  prune?: ExportPrunePolicy;
  /** Opaque application-owned tracking scope. */
  trackingKey?: string;
  /** Defaults to preserve. contents exports a directory's children directly into the destination root. */
  directoryLayout?: DirectoryExportLayout;
}

export interface NativeImportOptions extends OperationOptions {
  mode: NativeImportMode;
  /** Saved read/read-write directory grant. Omit to use the native picker. */
  sourceId?: string;
  /** Relative paths inside sourceId. Never absolute OS paths. Directory mode may omit this to import the whole grant root. */
  sourcePaths?: readonly string[];
  /** Logical VontaqFS destination root. */
  targetPath?: string;
  conflict?: ImportConflictPolicy;
}

export interface SaveExportPresetOptions {
  id?: string;
  name: string;
  destinationId: string;
  mode: NativeExportMode;
  conflict?: ExportConflictPolicy;
  sourcePath: string;
  archiveFormat?: 'zip';
}

export interface DestinationAPI {
  create(options?: CreateDestinationOptions): Promise<DestinationGrant>;
  list(): Promise<readonly DestinationGrant[]>;
  revoke(destinationId: string): Promise<boolean>;
}

export interface ExportPresetAPI {
  save(options: SaveExportPresetOptions): Promise<ExportPreset>;
  list(): Promise<readonly ExportPreset[]>;
  delete(id: string): Promise<boolean>;
}

export interface SnapshotAPI {
  create(label?: string, options?: OperationOptions): Promise<SnapshotInfo>;
  list(): Promise<readonly SnapshotInfo[]>;
  get(snapshotId: string): Promise<SnapshotInfo | null>;
  restore(snapshotId: string, options?: OperationOptions): Promise<SnapshotRestoreReport>;
  delete(snapshotId: string): Promise<boolean>;
}

export interface FileReadOptions extends OperationOptions {}

export interface FileWriteOptions extends OperationOptions {
  ifMatch?: string;
  metadata?: FileMetadataInput;
}

export interface FileDeleteOptions extends OperationOptions {
  recursive?: boolean;
  ifMatch?: string;
}

export interface FileCopyOptions extends OperationOptions {
  overwrite?: boolean;
}

export interface FileMoveOptions extends OperationOptions {
  overwrite?: boolean;
  ifMatch?: string;
}

export interface KeyValueWriteOptions {
  ifVersion?: number;
  ifMatch?: string;
}

export interface VontaqFSWriter {
  readonly streamId: string;
  readonly bytesWritten: number;
  write(chunk: Uint8Array): Promise<void>;
  commit(): Promise<FileInfo>;
  abort(): Promise<void>;
}

export interface VontaqFSReader {
  readonly streamId: string;
  readonly file: FileInfo;
  read(): Promise<Uint8Array | null>;
  close(): Promise<void>;
}

export interface WatchOptions {
  waitMs?: number;
  onError?: (error: VontaqFSError) => void | Promise<void>;
}

export type WatchCallback = (event: RuntimeEvent) => void | Promise<void>;
export type Unsubscribe = () => void;

export interface FileAPI {
  stat(path: string): Promise<FileInfo | null>;
  exists(path: string): Promise<boolean>;
  readFile(path: string, options?: FileReadOptions): Promise<Uint8Array>;
  readMany(paths: readonly string[], options?: FileReadOptions): Promise<ReadManyReport>;
  writeFile(path: string, bytes: Uint8Array, options?: FileWriteOptions): Promise<FileInfo>;
  readText(path: string, options?: FileReadOptions): Promise<string>;
  writeText(path: string, value: string, options?: FileWriteOptions): Promise<FileInfo>;
  readJSON<T = unknown>(path: string, options?: FileReadOptions): Promise<T>;
  writeJSON(path: string, value: unknown, options?: FileWriteOptions): Promise<FileInfo>;
  delete(path: string, options?: FileDeleteOptions): Promise<{ deletedFiles: number }>;
  copy(from: string, to: string, options?: FileCopyOptions): Promise<{ copiedFiles: number }>;
  move(from: string, to: string, options?: FileMoveOptions): Promise<{ movedFiles: number }>;
  createReader(path: string, options?: FileReadOptions): Promise<VontaqFSReader>;
  createWriter(path: string, options?: FileWriteOptions & { declaredSize?: number }): Promise<VontaqFSWriter>;
}

export interface KeyValueAPI {
  get<T = unknown>(key: string): Promise<KeyValueEntry<T> | null>;
  set<T = unknown>(key: string, value: T, options?: KeyValueWriteOptions): Promise<KeyValueEntry<T>>;
}

export interface FormatAPI {
  register(descriptor: FormatDescriptorInput): Promise<FormatDescriptor>;
  list(): Promise<readonly FormatDescriptor[]>;
  delete(id: string): Promise<boolean>;
}

export class FetchTransport implements VontaqFSTransport {
  async request(request: VontaqFSHttpRequest): Promise<VontaqFSHttpResponse> {
    const fetchFn = (globalThis as { fetch?: typeof fetch }).fetch;
    if (typeof fetchFn !== 'function') {
      throw new VontaqFSError('RUNTIME_UNREACHABLE', 'This environment does not provide fetch().');
    }
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), request.timeoutMs);
    try {
      const response = await fetchFn(request.url, {
        method: request.method,
        headers: request.headers,
        body: request.body as never,
        signal: controller.signal,
      });
      return {
        status: response.status,
        body: request.responseType === 'binary'
          ? new Uint8Array(await response.arrayBuffer())
          : await response.text(),
      };
    } finally {
      clearTimeout(timeoutId);
    }
  }
}

interface ConnectionSettings {
  application: ClientIdentity;
  stateStore: ClientStateStore;
  endpoint: string;
  discoveryEndpoints: readonly string[];
  transport: VontaqFSTransport;
  pairingPollIntervalMs: number;
  pairingTimeoutMs?: number;
  requestTimeoutMs: number;
  retryCount: number;
  materializationLimitBytes: number;
  onPairingRequired?: (event: PairingRequiredEvent) => void | Promise<void>;
}

class RuntimeConnection {
  private readonly settings: ConnectionSettings;
  private readonly clientInstanceId: string;
  private pairingCredential: string;
  private session: SessionResponse;
  private closed = false;

  private constructor(settings: ConnectionSettings, clientInstanceId: string, pairingCredential: string, session: SessionResponse) {
    this.settings = settings;
    this.clientInstanceId = clientInstanceId;
    this.pairingCredential = pairingCredential;
    this.session = session;
  }

  static async connect(settings: ConnectionSettings): Promise<RuntimeConnection> {
    validateApplication(settings.application);

    let clientInstanceId = await settings.stateStore.get(CLIENT_INSTANCE_KEY);
    if (!clientInstanceId) {
      clientInstanceId = secureId('client');
      await settings.stateStore.set(CLIENT_INSTANCE_KEY, clientInstanceId);
    }

    let credential = await settings.stateStore.get(PAIRING_CREDENTIAL_KEY);
    const endpoint = credential
      ? await discoverTrustedRuntime(settings, clientInstanceId, credential)
      : await discoverFirstPairRuntime(settings);
    const connectedSettings = { ...settings, endpoint };
    const health = await fetchHealth(connectedSettings);
    validateHealth(health);

    let session: SessionResponse;
    if (credential) {
      session = await createSession(connectedSettings, clientInstanceId, credential);
    } else {
      credential = await pair(connectedSettings, clientInstanceId);
      await connectedSettings.stateStore.set(PAIRING_CREDENTIAL_KEY, credential);
      session = await createSession(connectedSettings, clientInstanceId, credential);
    }

    return new RuntimeConnection(connectedSettings, clientInstanceId, credential, session);
  }

  get materializationLimitBytes(): number { return this.settings.materializationLimitBytes; }
  get requestTimeoutMs(): number { return this.settings.requestTimeoutMs; }
  get sessionCapabilityIds(): readonly string[] { return this.session.capabilities; }

  requireCapability(capability: string): void {
    if (!this.session.capabilities.includes(capability)) {
      throw new VontaqFSError('CAPABILITY_UNAVAILABLE', `The connected VontaqFS runtime does not support ${capability}.`, { capability });
    }
  }

  async health(): Promise<RuntimeStatus> {
    this.ensureOpen();
    const health = await fetchHealth(this.settings);
    validateHealth(health);
    return health;
  }

  async close(): Promise<void> {
    this.closed = true;
    this.session = { ...this.session, token: '', expiresAtMs: 0 };
  }

  post<T>(path: string, body: unknown, mode: 'read' | 'mutation'): Promise<T> {
    return this.withSession(token => requestJson<T>(this.settings, path, body, token, mode, mode === 'mutation'));
  }

  postNonRetryingMutation<T>(path: string, body: unknown): Promise<T> {
    return this.withSession(token => requestJson<T>(this.settings, path, body, token, 'mutation', false));
  }

  putBinaryJson<T>(path: string, bytes: Uint8Array): Promise<T> {
    return this.withSession(token => requestBinaryJson<T>(this.settings, path, bytes, token));
  }

  getBinary(path: string): Promise<Uint8Array> {
    return this.withSession(token => requestBinary(this.settings, path, token));
  }

  operationStatus(operationId: string): Promise<OperationProgress> {
    return this.post('/v1/operations/status', { operationId }, 'read');
  }

  cancelOperation(operationId: string): Promise<OperationProgress> {
    return this.post('/v1/operations/cancel', { operationId }, 'mutation');
  }

  private async withSession<T>(operation: (token: string) => Promise<T>): Promise<T> {
    this.ensureOpen();
    await this.refreshSessionIfNeeded();
    try {
      return await operation(this.session.token);
    } catch (error) {
      if (isVontaqFSError(error) && error.code === 'AUTH_INVALID') {
        await this.refreshSession();
        return operation(this.session.token);
      }
      throw error;
    }
  }

  private async refreshSessionIfNeeded(): Promise<void> {
    if (this.session.expiresAtMs <= Date.now() + SESSION_REFRESH_SKEW_MS) await this.refreshSession();
  }

  private async refreshSession(): Promise<void> {
    try {
      this.session = await createSession(this.settings, this.clientInstanceId, this.pairingCredential);
    } catch (error) {
      throw error;
    }
  }

  private ensureOpen(): void {
    if (this.closed) throw new VontaqFSError('RUNTIME_STOPPED', 'This VontaqFS client has been closed.');
  }
}

class OperationObserver {
  readonly request: OperationRequestWire;
  private active = false;
  private pollPromise?: Promise<void>;
  private abortListener?: () => void;
  private lastUpdatedAt = -1;

  constructor(
    private readonly connection: RuntimeConnection,
    private readonly options: OperationOptions,
  ) {
    const presentation = options.progress?.presentation ?? (options.progress?.onProgress ? 'client' : 'silent');
    this.request = { id: secureId('operation'), presentation };
  }

  async run<T>(action: (operation: OperationRequestWire) => Promise<T>): Promise<T> {
    this.throwIfAlreadyAborted();
    this.active = true;
    this.attachAbort();
    if (this.options.progress?.onProgress) this.pollPromise = this.pollLoop();
    try {
      const result = await action(this.request);
      await this.emitFinal();
      return result;
    } catch (error) {
      await this.emitFinal();
      if (this.options.signal?.aborted) throw abortError();
      throw error;
    } finally {
      this.active = false;
      this.detachAbort();
      await this.pollPromise;
    }
  }

  async runTracked<T>(startAction: (operation: OperationRequestWire) => Promise<OperationProgress>, cancelledCode: VontaqFSErrorCode = 'EXPORT_CANCELLED', cancelledLabel = 'operation'): Promise<T> {
    this.throwIfAlreadyAborted();
    this.active = true;
    this.attachAbort();
    try {
      let snapshot = await startAction(this.request);
      await this.emit(snapshot);
      for (;;) {
        if (snapshot.status === 'completed') return snapshot.result as T;
        if (snapshot.status === 'failed') {
          throw new VontaqFSError(normalizeErrorCode(snapshot.error?.code), snapshot.error?.message || 'VontaqFS operation failed.');
        }
        if (snapshot.status === 'cancelled') throw new VontaqFSError(cancelledCode, `VontaqFS ${cancelledLabel} was cancelled.`);
        await sleep(75);
        try {
          snapshot = await this.connection.operationStatus(this.request.id);
        } catch (error) {
          if (isVontaqFSError(error) && error.code === 'OPERATION_NOT_FOUND') {
            throw new VontaqFSError('OPERATION_LOST', 'The VontaqFS runtime lost the tracked operation before its outcome was known.', { operationId: this.request.id });
          }
          throw error;
        }
        await this.emit(snapshot);
      }
    } finally {
      this.active = false;
      this.detachAbort();
    }
  }

  async finish(): Promise<void> {
    this.active = false;
    await this.emitFinal();
    this.detachAbort();
    await this.pollPromise;
  }

  start(): void {
    this.throwIfAlreadyAborted();
    this.active = true;
    this.attachAbort();
    if (this.options.progress?.onProgress) this.pollPromise = this.pollLoop();
  }

  private throwIfAlreadyAborted(): void {
    if (this.options.signal?.aborted) throw abortError();
  }

  private attachAbort(): void {
    const signal = this.options.signal;
    if (!signal || this.abortListener) return;
    this.abortListener = () => { void this.requestCancel(); };
    signal.addEventListener('abort', this.abortListener, { once: true });
  }

  private detachAbort(): void {
    const signal = this.options.signal;
    if (signal && this.abortListener) signal.removeEventListener('abort', this.abortListener);
    this.abortListener = undefined;
  }

  private async requestCancel(): Promise<void> {
    for (let attempt = 0; attempt < 6; attempt += 1) {
      try {
        const snapshot = await this.connection.cancelOperation(this.request.id);
        await this.emit(snapshot);
        return;
      } catch (error) {
        if (isVontaqFSError(error) && error.code === 'OPERATION_NOT_FOUND' && attempt < 5) {
          await sleep(20);
          continue;
        }
        if (isVontaqFSError(error) && ['OPERATION_NOT_CANCELLABLE', 'OPERATION_NOT_FOUND'].includes(error.code)) return;
        return;
      }
    }
  }

  private async pollLoop(): Promise<void> {
    while (this.active) {
      await this.pollOnce();
      if (this.active) await sleep(75);
    }
  }

  private async pollOnce(): Promise<void> {
    try {
      const snapshot = await this.connection.operationStatus(this.request.id);
      await this.emit(snapshot);
    } catch (error) {
      if (isVontaqFSError(error) && error.code === 'OPERATION_NOT_FOUND') return;
      if (isVontaqFSError(error) && ['RUNTIME_SHUTTING_DOWN', 'RUNTIME_STOPPED', 'RUNTIME_UNREACHABLE'].includes(error.code)) return;
    }
  }

  private async emitFinal(): Promise<void> {
    if (!this.options.progress?.onProgress) return;
    await this.pollOnce();
  }

  private async emit(snapshot: OperationProgress): Promise<void> {
    const callback = this.options.progress?.onProgress;
    if (!callback) return;
    if (snapshot.updatedAtMs === this.lastUpdatedAt && !['completed', 'failed', 'cancelled'].includes(snapshot.status)) return;
    this.lastUpdatedAt = snapshot.updatedAtMs;
    await callback(snapshot);
  }
}

class StreamWriter implements VontaqFSWriter {
  private readonly hasher = new IncrementalSha256();
  private sequence = 0;
  private state: 'open' | 'committed' | 'aborted' = 'open';
  private committedFile?: FileInfo;
  private checksum?: string;
  private written = 0;

  constructor(
    private readonly connection: RuntimeConnection,
    readonly streamId: string,
    private readonly maxChunkBytes: number,
    private readonly operation?: OperationObserver,
  ) {}

  get bytesWritten(): number { return this.written; }

  async write(chunk: Uint8Array): Promise<void> {
    if (this.state !== 'open' || this.checksum) throw new VontaqFSError('STREAM_EXPIRED', 'Cannot write after stream commit has started.');
    if (!(chunk instanceof Uint8Array)) throw new TypeError('writer.write() requires Uint8Array data.');
    for (let offset = 0; offset < chunk.byteLength; offset += this.maxChunkBytes) {
      const view = chunk.subarray(offset, Math.min(chunk.byteLength, offset + this.maxChunkBytes));
      await this.connection.putBinaryJson<StreamChunkAck>(`/v1/streams/write/${encodeURIComponent(this.streamId)}/${this.sequence}`, view);
      this.hasher.update(view);
      this.written += view.byteLength;
      this.sequence += 1;
    }
  }

  async commit(): Promise<FileInfo> {
    if (this.state === 'committed' && this.committedFile) return this.committedFile;
    if (this.state !== 'open') throw new VontaqFSError('STREAM_EXPIRED', 'Cannot commit an aborted stream.');
    this.checksum ??= this.hasher.digestHex();
    const file = await this.connection.post<FileInfo>(`/v1/streams/write/${encodeURIComponent(this.streamId)}/commit`, { sha256: this.checksum }, 'mutation');
    this.state = 'committed';
    this.committedFile = file;
    await this.operation?.finish();
    return file;
  }

  async abort(): Promise<void> {
    if (this.state !== 'open') return;
    this.state = 'aborted';
    try {
      await this.connection.post<void>(`/v1/streams/write/${encodeURIComponent(this.streamId)}/abort`, {}, 'mutation');
    } finally {
      await this.operation?.finish();
    }
  }
}

class StreamReader implements VontaqFSReader {
  private readonly hasher = new IncrementalSha256();
  private sequence = 0;
  private bytesRead = 0;
  private integrityVerified = false;
  private closed = false;

  constructor(
    private readonly connection: RuntimeConnection,
    readonly streamId: string,
    readonly file: FileInfo,
    private readonly operation?: OperationObserver,
  ) {}

  async read(): Promise<Uint8Array | null> {
    if (this.closed || this.bytesRead >= this.file.size) return null;
    const chunk = await this.connection.getBinary(`/v1/streams/read/${encodeURIComponent(this.streamId)}/${this.sequence}`);
    if (chunk.byteLength === 0 && this.bytesRead < this.file.size) {
      throw new VontaqFSError('STORAGE_CORRUPT', 'Read stream ended before the recorded file size.');
    }
    this.hasher.update(chunk);
    this.sequence += 1;
    this.bytesRead += chunk.byteLength;
    if (this.bytesRead > this.file.size) throw new VontaqFSError('STORAGE_CORRUPT', 'Read stream exceeded the recorded file size.');
    if (this.bytesRead === this.file.size && !this.integrityVerified) {
      this.integrityVerified = true;
      const actual = this.hasher.digestHex();
      if (actual !== this.file.etag.toLowerCase()) {
        throw new VontaqFSError('STREAM_CHECKSUM_MISMATCH', 'Read stream checksum did not match runtime metadata.', {
          expectedSha256: this.file.etag, actualSha256: actual,
        });
      }
      await this.operation?.finish();
    }
    return chunk;
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
      await this.connection.post<void>(`/v1/streams/read/${encodeURIComponent(this.streamId)}/close`, {}, 'mutation');
    } catch (error) {
      if (!isVontaqFSError(error) || error.code !== 'STREAM_NOT_FOUND') throw error;
    } finally {
      await this.operation?.finish();
    }
  }
}

class SpaceFiles implements FileAPI {
  constructor(private readonly connection: RuntimeConnection, private readonly spaceId: string) {}

  async stat(path: string): Promise<FileInfo | null> {
    const response = await this.connection.post<{ file: FileInfo | null }>('/v1/fs/stat', { spaceId: this.spaceId, path }, 'read');
    return response.file;
  }

  async exists(path: string): Promise<boolean> { return (await this.stat(path)) !== null; }

  async readFile(path: string, options: FileReadOptions = {}): Promise<Uint8Array> {
    const info = await this.requireFile(path);
    return this.readFileWithInfo(path, info, options);
  }

  async readMany(paths: readonly string[], options: FileReadOptions = {}): Promise<ReadManyReport> {
    if (!Array.isArray(paths) || paths.length === 0 || paths.length > VONTAQ_FS_READ_MANY_MAX_ITEMS) {
      throw new RangeError(`files.readMany() requires 1-${VONTAQ_FS_READ_MANY_MAX_ITEMS} paths.`);
    }
    for (const path of paths) {
      if (typeof path !== 'string') throw new TypeError('files.readMany() paths must be strings.');
    }
    this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.bulkRead);
    const observer = new OperationObserver(this.connection, options);
    const wire = await observer.run(operation => this.connection.post<ReadManyReportWire>('/v1/fs/read-many', {
      spaceId: this.spaceId, paths: [...paths], operation,
    }, 'read'));
    if (wire.cancelled) {
      if (options.signal?.aborted) throw abortError();
      throw new VontaqFSError('CONFLICT', 'VontaqFS readMany() was cancelled before all requested paths were processed.');
    }
    if (!Number.isSafeInteger(wire.totalBytes) || wire.totalBytes < 0 || wire.totalBytes > VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES) {
      throw new VontaqFSError('STORAGE_CORRUPT', 'VontaqFS returned an invalid readMany() aggregate byte count.');
    }
    return {
      completedItems: wire.completedItems,
      failedItems: wire.failedItems,
      totalBytes: wire.totalBytes,
      cancelled: false,
      results: wire.results.map((item) => {
        if (item.ok) {
          if (typeof item.dataBase64 !== 'string' || !item.file) {
            throw new VontaqFSError('STORAGE_CORRUPT', `VontaqFS returned an incomplete readMany() success result for ${item.path}.`);
          }
          return { path: item.path, ok: true as const, bytes: decodeBase64(item.dataBase64), file: item.file };
        }
        if (!item.error || typeof item.error.code !== 'string' || typeof item.error.message !== 'string') {
          throw new VontaqFSError('STORAGE_CORRUPT', `VontaqFS returned an incomplete readMany() failure result for ${item.path}.`);
        }
        return { path: item.path, ok: false as const, error: item.error };
      }),
    };
  }

  async writeFile(path: string, bytes: Uint8Array, options: FileWriteOptions = {}): Promise<FileInfo> {
    if (!(bytes instanceof Uint8Array)) throw new TypeError('writeFile() requires Uint8Array data.');
    if (bytes.byteLength > VONTAQ_FS_MAX_FILE_BYTES) throw new VontaqFSError('FILE_TOO_LARGE', 'File exceeds the 16 GiB safety ceiling.');
    if (bytes.byteLength <= VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
      const observer = new OperationObserver(this.connection, options);
      return observer.run(operation => this.connection.post<FileInfo>('/v1/fs/write-small', {
        requestId: secureId('request'), spaceId: this.spaceId, path,
        dataBase64: encodeBase64(bytes), ifMatch: options.ifMatch ?? null,
        metadata: normalizeFileMetadata(options.metadata), operation,
      }, 'mutation'));
    }
    const writer = await this.createWriter(path, { ...options, declaredSize: bytes.byteLength });
    try {
      for (let offset = 0; offset < bytes.byteLength; offset += VONTAQ_FS_STREAM_CHUNK_BYTES) {
        await writer.write(bytes.subarray(offset, Math.min(bytes.byteLength, offset + VONTAQ_FS_STREAM_CHUNK_BYTES)));
      }
      return await writer.commit();
    } catch (error) {
      await bestEffortAbort(writer);
      throw error;
    }
  }

  async readText(path: string, options: FileReadOptions = {}): Promise<string> {
    const info = await this.requireFile(path);
    this.assertMaterializable(info);
    if (info.size <= VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
      return new TextDecoder('utf-8', { fatal: true }).decode(await this.readFileWithInfo(path, info, options));
    }
    const reader = await this.createReader(path, options);
    const decoder = new TextDecoder('utf-8', { fatal: true });
    const parts: string[] = [];
    try {
      for (;;) {
        const chunk = await reader.read();
        if (!chunk) break;
        parts.push(decoder.decode(chunk, { stream: true }));
      }
      parts.push(decoder.decode());
      return parts.join('');
    } finally {
      await reader.close();
    }
  }

  async writeText(path: string, value: string, options: FileWriteOptions = {}): Promise<FileInfo> {
    if (typeof value !== 'string') throw new TypeError('writeText() requires a string.');
    if (value.length <= VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES / 4) {
      return this.writeFile(path, new TextEncoder().encode(value), options);
    }
    const writer = await this.createWriter(path, options);
    const encoder = new TextEncoder();
    try {
      for (const part of textSlices(value, TEXT_ENCODER_WINDOW_CODE_UNITS)) await writer.write(encoder.encode(part));
      return await writer.commit();
    } catch (error) {
      await bestEffortAbort(writer);
      throw error;
    }
  }

  async readJSON<T = unknown>(path: string, options: FileReadOptions = {}): Promise<T> {
    return JSON.parse(await this.readText(path, options)) as T;
  }

  async writeJSON(path: string, value: unknown, options: FileWriteOptions = {}): Promise<FileInfo> {
    const encoder = new TextEncoder();
    const buffered: Uint8Array[] = [];
    let bufferedBytes = 0;
    let writer: VontaqFSWriter | undefined;
    try {
      for (const fragment of jsonFragments(value)) {
        const bytes = encoder.encode(fragment);
        if (!writer && bufferedBytes + bytes.byteLength <= VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
          buffered.push(bytes);
          bufferedBytes += bytes.byteLength;
          continue;
        }
        if (!writer) {
          writer = await this.createWriter(path, options);
          for (const bufferedChunk of buffered) await writer.write(bufferedChunk);
        }
        await writer.write(bytes);
      }
      if (!writer) return this.writeFile(path, concatenate(buffered, bufferedBytes), options);
      return await writer.commit();
    } catch (error) {
      if (writer) await bestEffortAbort(writer);
      throw error;
    }
  }

  delete(path: string, options: FileDeleteOptions = {}): Promise<{ deletedFiles: number }> {
    const observer = new OperationObserver(this.connection, options);
    return observer.run(operation => this.connection.post('/v1/fs/delete', {
      requestId: secureId('request'), spaceId: this.spaceId, path,
      recursive: options.recursive ?? false, ifMatch: options.ifMatch ?? null, operation,
    }, 'mutation'));
  }

  copy(from: string, to: string, options: FileCopyOptions = {}): Promise<{ copiedFiles: number }> {
    const observer = new OperationObserver(this.connection, options);
    return observer.run(operation => this.connection.post('/v1/fs/copy', {
      requestId: secureId('request'), spaceId: this.spaceId, from, to, overwrite: options.overwrite ?? false, operation,
    }, 'mutation'));
  }

  move(from: string, to: string, options: FileMoveOptions = {}): Promise<{ movedFiles: number }> {
    const observer = new OperationObserver(this.connection, options);
    return observer.run(operation => this.connection.post('/v1/fs/move', {
      requestId: secureId('request'), spaceId: this.spaceId, from, to,
      overwrite: options.overwrite ?? false, ifMatch: options.ifMatch ?? null, operation,
    }, 'mutation'));
  }

  async createWriter(path: string, options: FileWriteOptions & { declaredSize?: number } = {}): Promise<VontaqFSWriter> {
    if (options.declaredSize !== undefined && (!Number.isSafeInteger(options.declaredSize) || options.declaredSize < 0 || options.declaredSize > VONTAQ_FS_MAX_FILE_BYTES)) {
      throw new RangeError(`declaredSize must be between 0 and ${VONTAQ_FS_MAX_FILE_BYTES}.`);
    }
    const observer = new OperationObserver(this.connection, options);
    observer.start();
    try {
      const response = await this.connection.post<StreamWriteBeginResponse>('/v1/streams/write/begin', {
        requestId: secureId('request'), spaceId: this.spaceId, path,
        ifMatch: options.ifMatch ?? null, declaredSize: options.declaredSize ?? null,
        metadata: normalizeFileMetadata(options.metadata), operation: observer.request,
      }, 'mutation');
      const maxChunkBytes = Math.min(VONTAQ_FS_STREAM_CHUNK_MAX_BYTES, Math.max(1, response.maxChunkBytes));
      return new StreamWriter(this.connection, response.streamId, maxChunkBytes, observer);
    } catch (error) {
      await observer.finish();
      throw error;
    }
  }

  async createReader(path: string, options: FileReadOptions = {}): Promise<VontaqFSReader> {
    const observer = new OperationObserver(this.connection, options);
    observer.start();
    try {
      const response = await this.connection.post<StreamReadBeginResponse>('/v1/streams/read/begin', { spaceId: this.spaceId, path, operation: observer.request }, 'read');
      return new StreamReader(this.connection, response.streamId, response.file, observer);
    } catch (error) {
      await observer.finish();
      throw error;
    }
  }

  private async readFileWithInfo(path: string, info: FileInfo, options: FileReadOptions): Promise<Uint8Array> {
    this.assertMaterializable(info);
    if (info.size <= VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
      const observer = new OperationObserver(this.connection, options);
      const response = await observer.run(operation => this.connection.post<SmallFileReadResponse>('/v1/fs/read-small', { spaceId: this.spaceId, path, operation }, 'read'));
      return decodeBase64(response.dataBase64);
    }
    const reader = await this.createReader(path, options);
    const output = new Uint8Array(info.size);
    let offset = 0;
    try {
      for (;;) {
        const chunk = await reader.read();
        if (!chunk) break;
        output.set(chunk, offset);
        offset += chunk.byteLength;
      }
    } finally {
      await reader.close();
    }
    if (offset !== info.size) throw new VontaqFSError('STORAGE_CORRUPT', 'Materialized file size did not match runtime metadata.');
    return output;
  }

  private async requireFile(path: string): Promise<FileInfo> {
    const info = await this.stat(path);
    if (!info) throw new VontaqFSError('NOT_FOUND', `File not found: ${path}`);
    return info;
  }

  private assertMaterializable(info: FileInfo): void {
    if (info.size > this.connection.materializationLimitBytes) {
      throw new VontaqFSError('MATERIALIZATION_LIMIT', `File is too large for a high-level materialized read. Use createReader().`, {
        size: info.size, materializationLimit: this.connection.materializationLimitBytes,
      });
    }
  }
}

class SpaceKeyValue implements KeyValueAPI {
  constructor(private readonly connection: RuntimeConnection, private readonly spaceId: string) {}

  async get<T = unknown>(key: string): Promise<KeyValueEntry<T> | null> {
    const response = await this.connection.post<{ entry: KeyValueEntry<T> | null }>('/v1/kv/get', { spaceId: this.spaceId, key }, 'read');
    return response.entry;
  }

  set<T = unknown>(key: string, value: T, options: KeyValueWriteOptions = {}): Promise<KeyValueEntry<T>> {
    return this.connection.post('/v1/kv/set', {
      requestId: secureId('request'), spaceId: this.spaceId, key, value,
      ifVersion: options.ifVersion ?? null, ifMatch: options.ifMatch ?? null,
    }, 'mutation');
  }
}

class ApplicationFormats implements FormatAPI {
  constructor(private readonly connection: RuntimeConnection) {}

  register(descriptor: FormatDescriptorInput): Promise<FormatDescriptor> {
    const normalized = normalizeFormatDescriptor(descriptor);
    return this.connection.post('/v1/formats/register', { requestId: secureId('request'), ...normalized }, 'mutation');
  }

  async list(): Promise<readonly FormatDescriptor[]> {
    const response = await this.connection.post<{ formats: FormatDescriptor[] }>('/v1/formats/list', {}, 'read');
    return response.formats;
  }

  async delete(id: string): Promise<boolean> {
    validateFormatId(id);
    const response = await this.connection.post<{ deleted: boolean }>('/v1/formats/delete', { requestId: secureId('request'), id }, 'mutation');
    return response.deleted;
  }
}

class ApplicationDestinations implements DestinationAPI {
  constructor(private readonly connection: RuntimeConnection) {}
  create(options: CreateDestinationOptions = {}): Promise<DestinationGrant> {
    const capability = options.capability ?? 'write';
    if (!['read', 'write', 'read-write'].includes(capability)) throw new TypeError('destination capability is invalid.');
    if (options.label !== undefined && (!options.label.trim() || options.label.length > 128)) throw new TypeError('destination label must be 1-128 characters.');
    if (options.initialDestinationId !== undefined && !/^dst_[a-f0-9]{32}$/i.test(options.initialDestinationId)) throw new TypeError('initialDestinationId is invalid.');
    if (options.reuseInitialIfSame && !options.initialDestinationId) throw new TypeError('reuseInitialIfSame requires initialDestinationId.');
    if (options.initialDestinationId !== undefined || options.reuseInitialIfSame !== undefined) {
      this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.destinationPickerHints);
    }
    return this.connection.postNonRetryingMutation('/v1/destinations/create', {
      requestId: secureId('request'), label: options.label ?? null, capability,
      initialDestinationId: options.initialDestinationId ?? null,
      reuseInitialIfSame: options.reuseInitialIfSame ?? false,
    });
  }
  async list(): Promise<readonly DestinationGrant[]> {
    const response = await this.connection.post<{ destinations: DestinationGrant[] }>('/v1/destinations/list', {}, 'read');
    return response.destinations;
  }
  async revoke(destinationId: string): Promise<boolean> {
    if (!/^dst_[a-f0-9]{32}$/i.test(destinationId)) throw new TypeError('destinationId is invalid.');
    const response = await this.connection.post<{ revoked: boolean }>('/v1/destinations/revoke', { requestId: secureId('request'), destinationId }, 'mutation');
    return response.revoked;
  }
}

class SpaceSnapshots implements SnapshotAPI {
  constructor(private readonly connection: RuntimeConnection, private readonly spaceId: string) {}

  create(label?: string, options: OperationOptions = {}): Promise<SnapshotInfo> {
    if (label !== undefined && (!label.trim() || label.length > 128 || /[\u0000-\u001F\u007F]/.test(label))) {
      throw new TypeError('snapshot label must be 1-128 characters without control characters.');
    }
    const observer = new OperationObserver(this.connection, options);
    return observer.runTracked<SnapshotInfo>(operation => this.connection.postNonRetryingMutation<OperationProgress>('/v1/snapshots/create', {
      requestId: secureId('request'), spaceId: this.spaceId, label: label ?? null, operation,
    }), 'CONFLICT', 'snapshot creation');
  }

  async list(): Promise<readonly SnapshotInfo[]> {
    const response = await this.connection.post<{ snapshots: SnapshotInfo[] }>('/v1/snapshots/list', { spaceId: this.spaceId }, 'read');
    return response.snapshots;
  }

  async get(snapshotId: string): Promise<SnapshotInfo | null> {
    validateSnapshotId(snapshotId);
    const snapshots = await this.list();
    return snapshots.find(snapshot => snapshot.id === snapshotId) ?? null;
  }

  restore(snapshotId: string, options: OperationOptions = {}): Promise<SnapshotRestoreReport> {
    validateSnapshotId(snapshotId);
    const observer = new OperationObserver(this.connection, options);
    return observer.runTracked<SnapshotRestoreReport>(operation => this.connection.postNonRetryingMutation<OperationProgress>('/v1/snapshots/restore', {
      requestId: secureId('request'), spaceId: this.spaceId, snapshotId, operation,
    }), 'CONFLICT', 'snapshot restore');
  }

  async delete(snapshotId: string): Promise<boolean> {
    validateSnapshotId(snapshotId);
    const response = await this.connection.post<{ deleted: boolean }>('/v1/snapshots/delete', {
      requestId: secureId('request'), spaceId: this.spaceId, snapshotId,
    }, 'mutation');
    return response.deleted;
  }
}

class ApplicationExportPresets implements ExportPresetAPI {
  constructor(private readonly connection: RuntimeConnection) {}
  save(options: SaveExportPresetOptions): Promise<ExportPreset> {
    if (!options?.name?.trim() || options.name.length > 128) throw new TypeError('export preset name must be 1-128 characters.');
    if (!/^dst_[a-f0-9]{32}$/i.test(options.destinationId)) throw new TypeError('destinationId is invalid.');
    if (!['file', 'files', 'directory', 'archive'].includes(options.mode)) throw new TypeError('export preset mode is invalid.');
    if (options.archiveFormat && (options.mode !== 'archive' || options.archiveFormat !== 'zip')) throw new TypeError('ZIP is the only supported archive preset format.');
    return this.connection.postNonRetryingMutation('/v1/export-presets/save', {
      requestId: secureId('request'), id: options.id ?? null, name: options.name,
      destinationId: options.destinationId, mode: options.mode, conflict: options.conflict ?? 'ask',
      sourcePath: options.sourcePath, archiveFormat: options.archiveFormat ?? null,
    });
  }
  async list(): Promise<readonly ExportPreset[]> {
    const response = await this.connection.post<{ presets: ExportPreset[] }>('/v1/export-presets/list', {}, 'read');
    return response.presets;
  }
  async delete(id: string): Promise<boolean> {
    const response = await this.connection.post<{ deleted: boolean }>('/v1/export-presets/delete', { requestId: secureId('request'), id }, 'mutation');
    return response.deleted;
  }
}

export class VontaqFSSpace {
  readonly files: FileAPI;
  readonly kv: KeyValueAPI;
  readonly snapshots: SnapshotAPI;

  constructor(private readonly connection: RuntimeConnection, readonly info: SpaceInfo) {
    this.files = new SpaceFiles(connection, info.id);
    this.kv = new SpaceKeyValue(connection, info.id);
    this.snapshots = new SpaceSnapshots(connection, info.id);
  }

  get id(): string { return this.info.id; }
  get key(): string { return this.info.key; }
  get storageClass(): StorageClass { return this.info.storageClass; }
  get storageCategory(): StorageCategory { return this.info.storageCategory ?? 'user-data'; }

  async clear(options: OperationOptions = {}): Promise<SpaceClearReport> {
    if (this.storageClass === 'persistent') {
      throw new VontaqFSError('REQUEST_INVALID', 'Persistent spaces cannot be cleared with space.clear().');
    }
    this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.spaceClear);
    const observer = new OperationObserver(this.connection, options);
    return observer.run(operation => this.connection.postNonRetryingMutation<SpaceClearReport>('/v1/spaces/clear', {
      requestId: secureId('request'), spaceId: this.id, operation,
    }));
  }

  async batch(operations: readonly BatchOperation[], options: BatchOptions = {}): Promise<BatchReport> {
    if (!Array.isArray(operations) || operations.length === 0 || operations.length > VONTAQ_FS_MAX_BATCH_OPERATIONS) {
      throw new RangeError(`space.batch() requires 1-${VONTAQ_FS_MAX_BATCH_OPERATIONS} operations.`);
    }
    let decodedBytes = 0;
    const wireOperations = operations.map((operation) => {
      const wire = batchOperationWire(operation);
      if (operation.type === 'write-file') decodedBytes += operation.bytes.byteLength;
      return wire;
    });
    if (decodedBytes > VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES) {
      throw new RangeError(`space.batch() write payloads exceed ${VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES} bytes; use writeTree()/writeFile() so streaming can be selected automatically.`);
    }
    const observer = new OperationObserver(this.connection, options);
    const report = await observer.run(operation => this.connection.postNonRetryingMutation<BatchReport>('/v1/batch', {
      spaceId: this.id, operations: wireOperations, operation,
    }));
    if (report.cancelled) {
      throw new VontaqFSError('BATCH_CANCELLED', 'VontaqFS batch was cancelled after applying a bounded prefix of operations.', {
        completedItems: report.completedItems, failedItems: report.failedItems,
      });
    }
    return report;
  }

  async writeTree(entries: readonly WriteTreeEntry[], options: WriteTreeOptions = {}): Promise<WriteTreeReport> {
    if (!Array.isArray(entries) || entries.length === 0 || entries.length > MAX_WRITE_TREE_ENTRIES) {
      throw new RangeError(`writeTree() requires 1-${MAX_WRITE_TREE_ENTRIES} entries.`);
    }
    const normalized = entries.map((entry) => {
      if (!entry || typeof entry.path !== 'string' || !(entry.bytes instanceof Uint8Array)) throw new TypeError('writeTree() entries require path and Uint8Array bytes.');
      if (entry.bytes.byteLength > VONTAQ_FS_MAX_FILE_BYTES) throw new RangeError(`writeTree() entry exceeds ${VONTAQ_FS_MAX_FILE_BYTES} bytes.`);
      return entry;
    });
    const totalBytes = normalized.reduce((sum, entry) => {
      const next = sum + entry.bytes.byteLength;
      if (!Number.isSafeInteger(next)) throw new RangeError('writeTree() aggregate byte count exceeds JavaScript safe integer range.');
      return next;
    }, 0);
    const presentation = options.progress?.presentation ?? (options.progress?.onProgress ? 'client' : 'silent');
    const aggregateId = secureId('operation');
    const aggregateStartedAt = Date.now();
    let completedItems = 0;
    let failedItems = 0;
    let processedBytes = 0;
    const files: FileInfo[] = [];

    const childOptions = (itemCount: number, byteCount: number): OperationOptions => ({
      signal: options.signal,
      progress: {
        presentation,
        ...(options.progress?.onProgress ? {
          onProgress: async (progress: OperationProgress) => {
            const childItems = progress.itemsCompleted ?? (progress.status === 'completed' ? itemCount : 0);
            const childBytes = progress.bytesCompleted ?? (progress.status === 'completed' ? byteCount : 0);
            await options.progress!.onProgress!({
              ...progress,
              id: aggregateId,
              kind: 'tree-write',
              presentation,
              completed: completedItems + childItems,
              total: normalized.length,
              itemsCompleted: completedItems + childItems,
              itemsTotal: normalized.length,
              bytesCompleted: Math.min(totalBytes, processedBytes + childBytes),
              bytesTotal: totalBytes,
              startedAtMs: aggregateStartedAt,
            });
          },
        } : {}),
      },
    });

    let index = 0;
    while (index < normalized.length) {
      if (options.signal?.aborted) throw abortError();
      const entry = normalized[index];
      if (entry.bytes.byteLength > VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
        try {
          const file = await this.files.writeFile(entry.path, entry.bytes, { ...childOptions(1, entry.bytes.byteLength), ifMatch: entry.ifMatch, metadata: entry.metadata });
          files.push(file);
          processedBytes += entry.bytes.byteLength;
        } catch (error) {
          failedItems += 1;
          if (!options.continueOnError || options.signal?.aborted) throw error;
        }
        completedItems += 1;
        index += 1;
        continue;
      }

      const group: WriteTreeEntry[] = [];
      let groupBytes = 0;
      while (index < normalized.length && group.length < VONTAQ_FS_MAX_BATCH_OPERATIONS) {
        const candidate = normalized[index];
        if (candidate.bytes.byteLength > VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) break;
        if (group.length > 0 && groupBytes + candidate.bytes.byteLength > VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES) break;
        group.push(candidate);
        groupBytes += candidate.bytes.byteLength;
        index += 1;
      }
      if (group.length === 0) continue;
      const report = await this.batch(group.map((item) => ({ type: 'write-file' as const, path: item.path, bytes: item.bytes, ifMatch: item.ifMatch, metadata: item.metadata })), childOptions(group.length, groupBytes));
      for (const result of report.results) {
        if (result.ok && result.file) files.push(result.file);
        if (!result.ok) failedItems += 1;
      }
      completedItems += report.completedItems;
      processedBytes += groupBytes;
      if (report.failedItems > 0 && !options.continueOnError) {
        const first = report.results.find((result) => !result.ok)?.error;
        throw new VontaqFSError(normalizeErrorCode(first?.code), first?.message || 'writeTree() batch failed after applying a bounded prefix of the tree.', { completedItems, failedItems });
      }
    }

    return { files, completedItems, failedItems, totalBytes };
  }

  async export(source: string | readonly string[], options: NativeExportOptions): Promise<NativeExportReport> {
    if (!options || !['file', 'files', 'directory', 'archive'].includes(options.mode)) throw new TypeError('space.export() requires a valid export mode.');
    const sourcePaths = typeof source === 'string' ? [source] : [...source];
    if (sourcePaths.length === 0) throw new TypeError('space.export() requires at least one source path.');
    if (options.destinationId && !/^dst_[a-f0-9]{32}$/i.test(options.destinationId)) throw new TypeError('destinationId is invalid.');
    if (options.format && options.format !== 'zip') throw new VontaqFSError('ARCHIVE_UNSUPPORTED', 'Only ZIP archive export is supported.');
    if ((options.format || options.archiveName) && options.mode !== 'archive') throw new TypeError('archive options require mode=archive.');
    const bookkeeping = options.bookkeeping ?? 'destination';
    const prune = options.prune ?? 'none';
    const directoryLayout = options.directoryLayout ?? 'preserve';
    if (!['destination', 'internal'].includes(bookkeeping)) throw new TypeError('bookkeeping is invalid.');
    if (!['none', 'tracked'].includes(prune)) throw new TypeError('prune is invalid.');
    if (!['preserve', 'contents'].includes(directoryLayout)) throw new TypeError('directoryLayout is invalid.');
    if (directoryLayout === 'contents' && options.mode !== 'directory') throw new TypeError('directoryLayout=contents requires mode=directory.');
    if (directoryLayout === 'contents' && sourcePaths.length !== 1) throw new TypeError('directoryLayout=contents requires exactly one directory source.');
    if (options.trackingKey !== undefined && (!options.trackingKey.trim() || options.trackingKey.length > 256 || /[\u0000-\u001F\u007F]/.test(options.trackingKey))) {
      throw new TypeError('trackingKey must be 1-256 characters without control characters.');
    }
    if (bookkeeping === 'internal') this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.internalExportBookkeeping);
    if (prune === 'tracked' || options.trackingKey !== undefined) this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.trackedExportPrune);
    if (directoryLayout === 'contents') this.connection.requireCapability(VONTAQ_FS_CAPABILITY_IDS.directoryContentsExport);
    const observer = new OperationObserver(this.connection, options);
    return observer.runTracked<NativeExportReport>(operation => this.connection.postNonRetryingMutation<OperationProgress>('/v1/exports/start', {
      requestId: secureId('request'), spaceId: this.id, sourcePaths, mode: options.mode,
      destinationId: options.destinationId ?? null, conflict: options.conflict ?? 'ask',
      archiveName: options.archiveName ?? null, bookkeeping, prune,
      trackingKey: options.trackingKey ?? null, directoryLayout, operation,
    }));
  }

  exportPreset(preset: ExportPreset, options: OperationOptions = {}): Promise<NativeExportReport> {
    if (!preset?.id || !preset.destinationId) throw new TypeError('exportPreset() requires a preset returned by exportPresets.list/save.');
    return this.export(preset.sourcePath, {
      mode: preset.mode,
      destinationId: preset.destinationId,
      conflict: preset.conflict,
      ...(preset.archiveFormat === 'zip' ? { format: 'zip' as const } : {}),
      ...options,
    });
  }

  async import(options: NativeImportOptions): Promise<NativeImportReport> {
    if (!options || !['file', 'files', 'directory', 'archive'].includes(options.mode)) throw new TypeError('space.import() requires a valid import mode.');
    if (options.sourceId && !/^dst_[a-f0-9]{32}$/i.test(options.sourceId)) throw new TypeError('sourceId is invalid.');
    const sourcePaths = [...(options.sourcePaths ?? [])];
    if (!options.sourceId && sourcePaths.length) throw new TypeError('sourcePaths require a saved sourceId; one-off import uses the native picker.');
    for (const source of sourcePaths) {
      if (!source || source.startsWith('/') || source.includes('\\') || source.split('/').some(segment => !segment || segment === '.' || segment === '..')) {
        throw new TypeError('sourcePaths must be safe relative POSIX-style paths inside the saved directory grant.');
      }
    }
    if (options.sourceId && ['file', 'archive'].includes(options.mode) && sourcePaths.length !== 1) throw new TypeError(`${options.mode} import from a saved directory requires exactly one sourcePath.`);
    if (options.sourceId && options.mode === 'files' && sourcePaths.length === 0) throw new TypeError('files import from a saved directory requires sourcePaths.');
    const targetPath = options.targetPath ?? '/';
    if (!targetPath.startsWith('/')) throw new TypeError('targetPath must be a logical absolute VontaqFS path.');
    const conflict = options.conflict ?? 'ask';
    if (!['replace', 'skip', 'rename', 'ask'].includes(conflict)) throw new TypeError('import conflict policy is invalid.');
    const observer = new OperationObserver(this.connection, options);
    return observer.runTracked<NativeImportReport>(operation => this.connection.postNonRetryingMutation<OperationProgress>('/v1/imports/start', {
      requestId: secureId('request'), spaceId: this.id, mode: options.mode,
      sourceId: options.sourceId ?? null, sourcePaths, targetPath, conflict, operation,
    }), 'IMPORT_CANCELLED', 'import');
  }

  async watch(path: string, callback: WatchCallback, options: WatchOptions = {}): Promise<Unsubscribe> {
    if (typeof callback !== 'function') throw new TypeError('watch() requires a callback.');
    const maxSafeWaitMs = Math.max(0, Math.min(VONTAQ_FS_EVENT_LONG_POLL_MAX_MS, this.connection.requestTimeoutMs - 250));
    const waitMs = boundedInteger(options.waitMs ?? maxSafeWaitMs, 0, maxSafeWaitMs, 'waitMs');
    let active = true;
    const baseline = await this.connection.post<EventPollResponse>('/v1/events/poll', {
      afterSequence: 0, spaceId: this.id, pathPrefix: path, waitMs: 0,
    }, 'read');
    let cursor = baseline.latestSequence;
    const run = async (): Promise<void> => {
      while (active) {
        try {
          const response = await this.connection.post<EventPollResponse>('/v1/events/poll', {
            afterSequence: cursor, spaceId: this.id, pathPrefix: path, waitMs,
          }, 'read');
          if (!active) break;
          const sequenceReset = response.latestSequence < cursor;
          if (response.overflow || sequenceReset) {
            await callback({
              sequence: response.latestSequence,
              eventType: 'overflow-resync-required',
              spaceId: this.id,
              path,
              atMs: Date.now(),
            });
          }
          for (const event of response.events) {
            if (!active) break;
            await callback(event);
          }
          cursor = response.latestSequence;
        } catch (error) {
          if (!active) break;
          const normalized = isVontaqFSError(error)
            ? error
            : new VontaqFSError('RUNTIME_UNREACHABLE', 'VontaqFS watch polling failed.', { cause: safeCause(error) });
          if (options.onError) await options.onError(normalized);
          else throw normalized;
          await sleep(100);
        }
      }
    };
    void run().catch((error) => {
      active = false;
      // Do not silently swallow a background polling failure when no explicit
      // error handler is supplied. Surface it as an unhandled rejection.
      queueMicrotask(() => { void Promise.reject(error); });
    });
    return () => { active = false; };
  }

  async close(): Promise<void> { /* app-owned spaces do not require a server-side release operation */ }
}

export class VontaqFS {
  readonly files: FileAPI;
  readonly kv: KeyValueAPI;
  readonly defaultSpace: VontaqFSSpace;
  readonly formats: FormatAPI;
  readonly destinations: DestinationAPI;
  readonly exportPresets: ExportPresetAPI;

  private constructor(private readonly connection: RuntimeConnection, defaultSpace: VontaqFSSpace) {
    this.defaultSpace = defaultSpace;
    this.files = defaultSpace.files;
    this.kv = defaultSpace.kv;
    this.formats = new ApplicationFormats(connection);
    this.destinations = new ApplicationDestinations(connection);
    this.exportPresets = new ApplicationExportPresets(connection);
  }

  static async connect(options: VontaqFSConnectOptions): Promise<VontaqFS> {
    if (!options || !options.application || !options.stateStore) {
      throw new TypeError('VontaqFS.connect() requires application identity and a client-local stateStore. Figma plugins can use createFigmaConnectOptions().');
    }
    const settings: ConnectionSettings = {
      application: options.application,
      stateStore: options.stateStore,
      endpoint: '',
      discoveryEndpoints: options.developmentEndpoint ? [normalizeEndpoint(options.developmentEndpoint)] : [...VONTAQ_FS_ENDPOINTS],
      transport: options.transport ?? new FetchTransport(),
      pairingPollIntervalMs: boundedInteger(options.pairingPollIntervalMs ?? DEFAULT_PAIRING_POLL_INTERVAL_MS, 0, 10_000, 'pairingPollIntervalMs'),
      pairingTimeoutMs: options.pairingTimeoutMs,
      requestTimeoutMs: boundedInteger(options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS, 100, 120_000, 'requestTimeoutMs'),
      retryCount: boundedInteger(options.retryCount ?? 1, 0, 3, 'retryCount'),
      materializationLimitBytes: boundedInteger(options.materializationLimitBytes ?? VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES, 1, VONTAQ_FS_MAX_FILE_BYTES, 'materializationLimitBytes'),
      onPairingRequired: options.onPairingRequired,
    };
    const connection = await RuntimeConnection.connect(settings);
    const defaultInfo = await openSpace(connection, { key: DEFAULT_SPACE_KEY, storageClass: 'persistent' });
    return new VontaqFS(connection, new VontaqFSSpace(connection, defaultInfo));
  }

  status(): Promise<RuntimeStatus> { return this.connection.health(); }

  async capabilities(): Promise<VontaqFSCapabilities> {
    return capabilityMap(this.connection.sessionCapabilityIds);
  }

  async openSpace(options: OpenSpaceOptions): Promise<VontaqFSSpace> {
    return new VontaqFSSpace(this.connection, await openSpace(this.connection, options));
  }

  async listSpaces(): Promise<readonly SpaceInfo[]> {
    const response = await this.connection.post<{ spaces: SpaceInfo[] }>('/v1/spaces/list', {}, 'read');
    return response.spaces.map(normalizeSpaceInfo);
  }

  async workspace(key: string, options: WorkspaceOptions = {}): Promise<VontaqFSWorkspace> {
    validateWorkspaceKey(key);
    const storage = await this.openSpace({
      key: workspaceSpaceKey(key, 'persistent'),
      storageClass: 'persistent',
      storageCategory: 'user-data',
      displayName: options.displayName ?? key,
    });
    return new VontaqFSWorkspace(this, key, storage, options.displayName ?? key);
  }

  watch(path: string, callback: WatchCallback, options?: WatchOptions): Promise<Unsubscribe> {
    return this.defaultSpace.watch(path, callback, options);
  }

  close(): Promise<void> { return this.connection.close(); }

  readFile(path: string, options?: FileReadOptions): Promise<Uint8Array> { return this.files.readFile(path, options); }
  writeFile(path: string, bytes: Uint8Array, options?: FileWriteOptions): Promise<FileInfo> { return this.files.writeFile(path, bytes, options); }
  readText(path: string, options?: FileReadOptions): Promise<string> { return this.files.readText(path, options); }
  writeText(path: string, value: string, options?: FileWriteOptions): Promise<FileInfo> { return this.files.writeText(path, value, options); }
  readJSON<T = unknown>(path: string, options?: FileReadOptions): Promise<T> { return this.files.readJSON<T>(path, options); }
  writeJSON(path: string, value: unknown, options?: FileWriteOptions): Promise<FileInfo> { return this.files.writeJSON(path, value, options); }
}

export class VontaqFSWorkspace {
  readonly files: FileAPI;
  readonly kv: KeyValueAPI;
  readonly snapshots: SnapshotAPI;
  readonly destinations: DestinationAPI;
  readonly exportPresets: ExportPresetAPI;

  constructor(
    private readonly client: VontaqFS,
    readonly key: string,
    readonly storage: VontaqFSSpace,
    readonly displayName: string,
  ) {
    this.files = storage.files;
    this.kv = storage.kv;
    this.snapshots = storage.snapshots;
    this.destinations = client.destinations;
    this.exportPresets = client.exportPresets;
  }

  get persistent(): VontaqFSSpace { return this.storage; }

  cache(): Promise<VontaqFSSpace> {
    return this.client.openSpace({
      key: workspaceSpaceKey(this.key, 'cache'), storageClass: 'cache', storageCategory: 'custom', displayName: `${this.displayName} Cache`,
    });
  }

  generated(): Promise<VontaqFSSpace> {
    return this.client.openSpace({
      key: workspaceSpaceKey(this.key, 'generated'), storageClass: 'persistent', storageCategory: 'generated', displayName: `${this.displayName} Generated`,
    });
  }

  index(): Promise<VontaqFSSpace> {
    return this.client.openSpace({
      key: workspaceSpaceKey(this.key, 'index'), storageClass: 'persistent', storageCategory: 'index', displayName: `${this.displayName} Index`,
    });
  }
}

function validateSnapshotId(snapshotId: string): void {
  if (typeof snapshotId !== 'string' || !/^snp_[a-f0-9]{32}$/i.test(snapshotId)) throw new TypeError('snapshotId is invalid.');
}

function batchOperationWire(operation: BatchOperation): Record<string, unknown> {
  if (!operation || typeof operation !== 'object') throw new TypeError('batch operation is invalid.');
  switch (operation.type) {
    case 'write-file': {
      if (!(operation.bytes instanceof Uint8Array)) throw new TypeError('batch write-file bytes must be a Uint8Array.');
      if (operation.bytes.byteLength > VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES) {
        throw new RangeError('batch write-file payload exceeds the direct-transfer threshold; use writeTree()/writeFile() for automatic streaming.');
      }
      return { type: operation.type, requestId: secureId('request'), path: operation.path, dataBase64: encodeBase64(operation.bytes), ifMatch: operation.ifMatch ?? null, metadata: normalizeFileMetadata(operation.metadata) };
    }
    case 'delete':
      return { type: operation.type, requestId: secureId('request'), path: operation.path, recursive: operation.recursive ?? false, ifMatch: operation.ifMatch ?? null };
    case 'copy':
      return { type: operation.type, requestId: secureId('request'), from: operation.from, to: operation.to, overwrite: operation.overwrite ?? false };
    case 'move':
      return { type: operation.type, requestId: secureId('request'), from: operation.from, to: operation.to, overwrite: operation.overwrite ?? false, ifMatch: operation.ifMatch ?? null };
    case 'kv-set': {
      if (operation.ifVersion !== undefined && (!Number.isSafeInteger(operation.ifVersion) || operation.ifVersion < 0)) throw new TypeError('batch kv-set ifVersion must be a non-negative safe integer.');
      let encoded: string;
      try { encoded = JSON.stringify(operation.value); } catch { throw new TypeError('batch kv-set value must be JSON-serializable.'); }
      if (encoded === undefined) throw new TypeError('batch kv-set value must be JSON-serializable.');
      if (new TextEncoder().encode(encoded).byteLength > 512 * 1024) throw new RangeError('batch kv-set value exceeds 512 KiB.');
      return { type: operation.type, requestId: secureId('request'), key: operation.key, value: operation.value, ifVersion: operation.ifVersion ?? null, ifMatch: operation.ifMatch ?? null };
    }
    default:
      throw new TypeError('batch operation type is unsupported.');
  }
}

function normalizeSpaceInfo(info: SpaceInfo): SpaceInfo {
  return { ...info, storageCategory: info.storageCategory ?? 'user-data' };
}

function capabilityMap(rawCapabilities: readonly string[]): VontaqFSCapabilities {
  const raw = [...new Set(rawCapabilities.filter((value): value is string => typeof value === 'string'))];
  const has = (id: string): boolean => raw.includes(id);
  return {
    files: has('files'), kv: has('kv'), spaces: has('spaces'), streams: has('streams'), events: has('events'), formats: has('formats'), operations: has('operations'),
    nativeExport: has('native-export'), nativeImport: has('native-import'), savedDirectories: has('saved-directories'), exportPresets: has('export-presets'),
    backup: has('backup'), snapshots: has('snapshots'), batch: has('batch'), storageCategory: has('storage-category'), systemProgressWindow: has('system-progress-window'),
    bulkRead: has(VONTAQ_FS_CAPABILITY_IDS.bulkRead), spaceClear: has(VONTAQ_FS_CAPABILITY_IDS.spaceClear),
    destinationPickerHints: has(VONTAQ_FS_CAPABILITY_IDS.destinationPickerHints), internalExportBookkeeping: has(VONTAQ_FS_CAPABILITY_IDS.internalExportBookkeeping),
    trackedExportPrune: has(VONTAQ_FS_CAPABILITY_IDS.trackedExportPrune), directoryContentsExport: has(VONTAQ_FS_CAPABILITY_IDS.directoryContentsExport), raw,
  };
}

function validateWorkspaceKey(key: string): void {
  if (typeof key !== 'string' || !key.trim() || key.length > 256 || /[\u0000-\u001F\u007F]/.test(key)) {
    throw new TypeError('workspace key must be 1-256 characters without control characters.');
  }
}

function workspaceSpaceKey(key: string, role: 'persistent' | 'cache' | 'generated' | 'index'): string {
  return `workspace:${key}:${role}`;
}

function normalizeFileMetadata(metadata: FileMetadataInput | undefined): FileMetadataInput | null {
  if (!metadata) return null;
  const contentType = normalizeOptionalMetadataText(metadata.contentType, 128, 'contentType');
  const formatId = normalizeOptionalMetadataText(metadata.formatId, 64, 'formatId');
  if (formatId) validateFormatId(formatId);
  if (metadata.opaque !== undefined && typeof metadata.opaque !== 'boolean') throw new TypeError('metadata.opaque must be a boolean when provided.');
  return { ...(contentType ? { contentType } : {}), ...(formatId ? { formatId } : {}), ...(metadata.opaque !== undefined ? { opaque: metadata.opaque } : {}) };
}

function normalizeFormatDescriptor(descriptor: FormatDescriptorInput): { id: string; extension: string | null; displayName: string; contentType: string | null; opaque: boolean } {
  if (!descriptor || typeof descriptor !== 'object') throw new TypeError('formats.register() requires a descriptor.');
  validateFormatId(descriptor.id);
  const displayName = normalizeOptionalMetadataText(descriptor.displayName, 128, 'displayName');
  if (!displayName) throw new TypeError('displayName is required.');
  const contentType = normalizeOptionalMetadataText(descriptor.contentType, 128, 'contentType');
  let extension: string | null = null;
  if (descriptor.extension !== undefined) {
    if (typeof descriptor.extension !== 'string' || descriptor.extension.length < 2 || descriptor.extension.length > 32 || !/^\.[A-Za-z0-9_-]+$/.test(descriptor.extension)) {
      throw new TypeError("extension must start with '.' and contain only portable ASCII extension characters.");
    }
    extension = descriptor.extension;
  }
  if (descriptor.opaque !== undefined && typeof descriptor.opaque !== 'boolean') throw new TypeError('opaque must be a boolean when provided.');
  return { id: descriptor.id, extension, displayName, contentType, opaque: descriptor.opaque ?? false };
}

function validateFormatId(id: string): void {
  if (typeof id !== 'string' || !/^[A-Za-z0-9._-]{1,64}$/.test(id)) throw new TypeError("format id must be 1-64 ASCII letters, digits, '.', '_' or '-'.");
}

function normalizeOptionalMetadataText(value: string | undefined, maxLength: number, field: string): string | null {
  if (value === undefined) return null;
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > maxLength || /[\u0000-\u001F\u007F]/.test(value)) {
    throw new TypeError(`${field} is empty, too long, or contains control characters.`);
  }
  return value;
}

async function openSpace(connection: RuntimeConnection, options: OpenSpaceOptions): Promise<SpaceInfo> {
  if (!options.key || !options.key.trim()) throw new TypeError('Space key must not be empty.');
  if (options.storageCategory !== undefined && !['user-data', 'generated', 'index', 'backup', 'snapshot', 'custom'].includes(options.storageCategory)) {
    throw new TypeError('storageCategory is invalid.');
  }
  const info = await connection.post<SpaceInfo>('/v1/spaces/open', {
    requestId: secureId('request'), key: options.key,
    storageClass: options.storageClass ?? 'persistent', storageCategory: options.storageCategory ?? null, displayName: options.displayName ?? null,
  }, 'mutation');
  return normalizeSpaceInfo(info);
}

async function fetchHealth(settings: ConnectionSettings): Promise<RuntimeStatus> {
  let response: VontaqFSHttpResponse;
  try {
    response = await settings.transport.request({
      method: 'GET', url: `${settings.endpoint}/v1/health`, headers: {}, timeoutMs: settings.requestTimeoutMs,
    });
  } catch (error) {
    if (isVontaqFSError(error)) throw error;
    throw new VontaqFSError('RUNTIME_UNREACHABLE', 'VontaqFS is not reachable. Install or start VontaqFS, then try again.', { cause: safeCause(error), endpoint: settings.endpoint });
  }
  if (response.status < 200 || response.status >= 300) throw decodeRuntimeError(response);
  try { return JSON.parse(responseText(response)) as RuntimeStatus; }
  catch { throw new VontaqFSError('PORT_CONFLICT', 'An official VontaqFS endpoint responded with an invalid health payload.', { endpoint: settings.endpoint }); }
}

async function probeHealth(settings: ConnectionSettings, endpoint: string): Promise<RuntimeStatus | null> {
  try {
    const candidate = { ...settings, endpoint };
    const response = await candidate.transport.request({ method: 'GET', url: `${endpoint}/v1/health`, headers: {}, timeoutMs: candidate.requestTimeoutMs });
    if (response.status < 200 || response.status >= 300) return null;
    const health = JSON.parse(responseText(response)) as RuntimeStatus;
    if (!health || health.service !== 'vontaqfs') return null;
    if (!health.protocol || health.protocol.min > VONTAQ_FS_PROTOCOL_MAX || health.protocol.max < VONTAQ_FS_PROTOCOL_MIN) {
      throw new VontaqFSError('PROTOCOL_INCOMPATIBLE', 'A VontaqFS runtime was found, but its protocol is incompatible.', { endpoint, runtimeProtocol: health.protocol });
    }
    return health;
  } catch (error) {
    if (isVontaqFSError(error)) throw error;
    return null;
  }
}

async function discoverFirstPairRuntime(settings: ConnectionSettings): Promise<string> {
  const candidates: string[] = [];
  let pendingStateError: VontaqFSError | null = null;
  for (const endpoint of settings.discoveryEndpoints) {
    let health: RuntimeStatus | null = null;
    try { health = await probeHealth(settings, endpoint); }
    catch (error) { if (isVontaqFSError(error)) { pendingStateError ??= error; continue; } throw error; }
    if (!health) continue;
    try { validateHealth(health); candidates.push(endpoint); }
    catch (error) { if (isVontaqFSError(error)) pendingStateError ??= error; else throw error; }
  }
  if (candidates.length > 1) {
    throw new VontaqFSError('RUNTIME_IDENTITY_AMBIGUOUS', 'More than one compatible VontaqFS runtime is visible. Close unexpected instances before pairing.', { endpoints: candidates });
  }
  if (candidates.length === 1) return candidates[0]!;
  if (pendingStateError) throw pendingStateError;
  throw new VontaqFSError('RUNTIME_UNREACHABLE', 'VontaqFS is not reachable on any official endpoint. Install or start VontaqFS, then try again.');
}

async function discoverTrustedRuntime(settings: ConnectionSettings, clientInstanceId: string, pairingCredential: string): Promise<string> {
  const compatible: string[] = [];
  let protocolError: VontaqFSError | null = null;
  for (const endpoint of settings.discoveryEndpoints) {
    let health: RuntimeStatus | null = null;
    try { health = await probeHealth(settings, endpoint); }
    catch (error) { if (isVontaqFSError(error)) { protocolError ??= error; continue; } throw error; }
    if (!health) continue;
    try { validateHealth(health); compatible.push(endpoint); }
    catch (error) { if (isVontaqFSError(error)) { protocolError ??= error; continue; } throw error; }
  }
  const trusted: string[] = [];
  for (const endpoint of compatible) {
    if (await verifyRuntimeIdentity({ ...settings, endpoint }, clientInstanceId, pairingCredential)) trusted.push(endpoint);
  }
  if (trusted.length === 1) return trusted[0]!;
  if (trusted.length > 1) {
    throw new VontaqFSError('RUNTIME_IDENTITY_AMBIGUOUS', 'Multiple endpoints proved possession of the same pairing verifier. Refusing to choose automatically.', { endpoints: trusted });
  }
  if (compatible.length > 0) {
    throw new VontaqFSError('RUNTIME_IDENTITY_MISMATCH', 'Compatible localhost services were found, but none proved the saved VontaqFS pairing identity.', { endpoints: compatible });
  }
  if (protocolError) throw protocolError;
  throw new VontaqFSError('PAIRING_RUNTIME_NOT_FOUND', 'The Runtime associated with the saved pairing credential was not found on the official endpoint set. Re-pair only through an explicit user-approved recovery flow.');
}

async function verifyRuntimeIdentity(settings: ConnectionSettings, clientInstanceId: string, pairingCredential: string): Promise<boolean> {
  const nonce = secureHex(32);
  let response: RuntimeIdentityChallengeResponse;
  try {
    response = await requestJson<RuntimeIdentityChallengeResponse>(settings, '/v1/identity/challenge', {
      application: { kind: settings.application.kind, externalId: settings.application.externalId },
      clientInstanceId,
      nonce,
    }, undefined, 'read');
  } catch (error) {
    if (isVontaqFSError(error) && ['PAIRING_RUNTIME_NOT_FOUND', 'NOT_FOUND', 'AUTH_INVALID'].includes(error.code)) return false;
    if (isVontaqFSError(error)) return false;
    return false;
  }
  const credentialHash = sha256Bytes(new TextEncoder().encode(pairingCredential));
  const message = new TextEncoder().encode(`${VONTAQ_FS_RUNTIME_IDENTITY_DOMAIN_SEPARATOR}${nonce}`);
  const expected = hmacSha256Hex(credentialHash, message);
  return constantTimeHexEqual(expected, response.mac);
}

function validateHealth(health: RuntimeStatus): void {
  if (!health || health.service !== 'vontaqfs') throw new VontaqFSError('PORT_CONFLICT', 'The endpoint is occupied by an incompatible service.');
  if (!health.protocol || health.protocol.min > VONTAQ_FS_PROTOCOL_MAX || health.protocol.max < VONTAQ_FS_PROTOCOL_MIN) {
    throw new VontaqFSError('PROTOCOL_INCOMPATIBLE', 'The installed VontaqFS runtime does not support this SDK protocol version.', {
      runtimeProtocol: health.protocol, sdkProtocol: { min: VONTAQ_FS_PROTOCOL_MIN, max: VONTAQ_FS_PROTOCOL_MAX },
    });
  }
  if (health.status === 'starting') throw new VontaqFSError('RUNTIME_STARTING', 'VontaqFS is still starting.');
  if (health.status === 'stopping') throw new VontaqFSError('RUNTIME_SHUTTING_DOWN', 'VontaqFS is shutting down.');
  if (health.status !== 'ready') throw new VontaqFSError('RUNTIME_STOPPED', 'VontaqFS is not ready.');
}

async function pair(settings: ConnectionSettings, clientInstanceId: string): Promise<string> {
  const initial = await requestJson<PairingRequestResponse>(settings, '/v1/pairings/request', {
    application: settings.application, clientInstanceId,
  }, undefined, 'mutation');
  await settings.onPairingRequired?.({ pairingId: initial.pairingId, expiresAtMs: initial.expiresAtMs, application: settings.application });
  const timeoutAt = Math.min(initial.expiresAtMs, settings.pairingTimeoutMs === undefined ? Number.POSITIVE_INFINITY : Date.now() + Math.max(0, settings.pairingTimeoutMs));
  let current: PairingPollResponse = { ...initial };
  while (current.status === 'pending') {
    if (Date.now() >= timeoutAt) throw new VontaqFSError('PAIRING_EXPIRED', 'VontaqFS pairing approval timed out.');
    if (settings.pairingPollIntervalMs > 0) await sleep(settings.pairingPollIntervalMs);
    current = await requestJson<PairingPollResponse>(settings, '/v1/pairings/poll', { pairingId: initial.pairingId }, undefined, 'read');
  }
  if (current.status === 'denied') throw new VontaqFSError('PAIRING_DENIED', 'VontaqFS pairing was denied.');
  if (current.status === 'expired') throw new VontaqFSError('PAIRING_EXPIRED', 'VontaqFS pairing request expired.');
  if (current.status !== 'approved' || !current.pairingCredential) throw new VontaqFSError('INTERNAL_ERROR', 'VontaqFS approved pairing without returning a credential.');
  return current.pairingCredential;
}

function createSession(settings: ConnectionSettings, clientInstanceId: string, pairingCredential: string): Promise<SessionResponse> {
  return requestJson<SessionResponse>(settings, '/v1/sessions', { clientInstanceId, pairingCredential }, undefined, 'mutation');
}

async function requestJson<T>(settings: ConnectionSettings, path: string, body: unknown, token: string | undefined, mode: 'read' | 'mutation', retrySafe = mode === 'read'): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': VONTAQ_FS_CONTROL_CONTENT_TYPE };
  if (token) headers.Authorization = `Bearer ${token}`;
  const bodyText = JSON.stringify(body);
  return retryTransport(settings, path, retrySafe, async () => {
    const response = await settings.transport.request({
      method: 'POST', url: `${settings.endpoint}${path}`, headers, body: bodyText, timeoutMs: settings.requestTimeoutMs,
    });
    if (response.status < 200 || response.status >= 300) throw decodeRuntimeError(response);
    const text = responseText(response);
    return text ? JSON.parse(text) as T : undefined as T;
  });
}

async function requestBinaryJson<T>(settings: ConnectionSettings, path: string, bytes: Uint8Array, token: string): Promise<T> {
  return retryTransport(settings, path, true, async () => {
    const response = await settings.transport.request({
      method: 'PUT', url: `${settings.endpoint}${path}`,
      headers: { 'Content-Type': 'application/octet-stream', Authorization: `Bearer ${token}` },
      body: bytes, timeoutMs: settings.requestTimeoutMs,
    });
    if (response.status < 200 || response.status >= 300) throw decodeRuntimeError(response);
    return JSON.parse(responseText(response)) as T;
  });
}

async function requestBinary(settings: ConnectionSettings, path: string, token: string): Promise<Uint8Array> {
  return retryTransport(settings, path, true, async () => {
    const response = await settings.transport.request({
      method: 'GET', url: `${settings.endpoint}${path}`, headers: { Authorization: `Bearer ${token}` },
      responseType: 'binary', timeoutMs: settings.requestTimeoutMs,
    });
    if (response.status < 200 || response.status >= 300) throw decodeRuntimeError(response);
    return response.body instanceof Uint8Array ? response.body : new TextEncoder().encode(response.body);
  });
}

async function retryTransport<T>(settings: ConnectionSettings, path: string, retrySafe: boolean, operation: () => Promise<T>): Promise<T> {
  let lastTransportError: unknown;
  for (let attempt = 0; attempt <= settings.retryCount; attempt += 1) {
    try { return await operation(); }
    catch (error) {
      if (isVontaqFSError(error)) throw error;
      lastTransportError = error;
      if (attempt >= settings.retryCount) break;
      if (!retrySafe) break;
      await sleep(Math.min(25 * (attempt + 1), 100));
    }
  }
  throw new VontaqFSError('RUNTIME_UNREACHABLE', 'VontaqFS became unreachable while processing the request.', { cause: safeCause(lastTransportError), path });
}

function decodeRuntimeError(response: VontaqFSHttpResponse): VontaqFSError {
  let parsed: RuntimeErrorResponse | undefined;
  try { parsed = JSON.parse(responseText(response)) as RuntimeErrorResponse; } catch { /* normalize below */ }
  const runtimeCode = parsed?.error?.code;
  const code = normalizeErrorCode(runtimeCode);
  return new VontaqFSError(code, parsed?.error?.message || `VontaqFS request failed with HTTP ${response.status}.`, {
    httpStatus: response.status, ...(runtimeCode && code === 'INTERNAL_ERROR' ? { runtimeCode } : {}),
  });
}

function responseText(response: VontaqFSHttpResponse): string {
  return typeof response.body === 'string' ? response.body : new TextDecoder().decode(response.body);
}

function validateApplication(application: ClientIdentity): void {
  if (!application.externalId?.trim()) throw new TypeError('application.externalId must not be empty.');
  if (!application.displayName?.trim()) throw new TypeError('application.displayName must not be empty.');
  if (!['figma-plugin', 'figma-widget', 'other-supported-client'].includes(application.kind)) throw new TypeError('application.kind is not supported.');
}

function normalizeEndpoint(value: string): string {
  const trimmed = value.trim().replace(/\/+$/, '');
  if (!/^https?:\/\//i.test(trimmed)) throw new TypeError('endpoint must be an HTTP(S) URL.');
  return trimmed;
}

function boundedInteger(value: number, min: number, max: number, name: string): number {
  if (!Number.isSafeInteger(value) || value < min || value > max) throw new RangeError(`${name} must be an integer between ${min} and ${max}.`);
  return value;
}

function secureHex(byteLength: number): string {
  const cryptoApi = globalThis.crypto;
  if (!cryptoApi || typeof cryptoApi.getRandomValues !== 'function') throw new VontaqFSError('INTERNAL_ERROR', 'A cryptographically secure random source is required by VontaqFS.');
  const bytes = new Uint8Array(byteLength);
  cryptoApi.getRandomValues(bytes);
  return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
}

function sha256Bytes(data: Uint8Array): Uint8Array {
  const hasher = new IncrementalSha256();
  hasher.update(data);
  return hexToBytes(hasher.digestHex());
}

function hmacSha256Hex(key: Uint8Array, message: Uint8Array): string {
  const block = new Uint8Array(64);
  const normalized = key.byteLength > 64 ? sha256Bytes(key) : key;
  block.set(normalized.subarray(0, 64));
  const innerPad = new Uint8Array(64);
  const outerPad = new Uint8Array(64);
  for (let index = 0; index < 64; index += 1) {
    innerPad[index] = (block[index] ?? 0) ^ 0x36;
    outerPad[index] = (block[index] ?? 0) ^ 0x5c;
  }
  const inner = new IncrementalSha256();
  inner.update(innerPad); inner.update(message);
  const innerDigest = hexToBytes(inner.digestHex());
  const outer = new IncrementalSha256();
  outer.update(outerPad); outer.update(innerDigest);
  return outer.digestHex();
}

function hexToBytes(value: string): Uint8Array {
  if (value.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(value)) throw new VontaqFSError('INTERNAL_ERROR', 'Invalid hexadecimal value.');
  const out = new Uint8Array(value.length / 2);
  for (let index = 0; index < out.length; index += 1) out[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  return out;
}

function constantTimeHexEqual(expected: string, actual: string): boolean {
  const a = expected.toLowerCase(); const b = String(actual ?? '').toLowerCase();
  let diff = a.length ^ b.length;
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) diff |= (a.charCodeAt(index) || 0) ^ (b.charCodeAt(index) || 0);
  return diff === 0;
}

function secureId(prefix: string): string {
  const cryptoApi = globalThis.crypto;
  if (!cryptoApi || typeof cryptoApi.getRandomValues !== 'function') throw new VontaqFSError('INTERNAL_ERROR', 'A cryptographically secure random source is required by VontaqFS.');
  const bytes = new Uint8Array(16);
  cryptoApi.getRandomValues(bytes);
  return `${prefix}-${Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')}`;
}

function encodeBase64(bytes: Uint8Array): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let output = '';
  for (let index = 0; index < bytes.length; index += 3) {
    const a = bytes[index] ?? 0; const b = bytes[index + 1] ?? 0; const c = bytes[index + 2] ?? 0;
    output += alphabet[a >> 2];
    output += alphabet[((a & 3) << 4) | (b >> 4)];
    output += index + 1 < bytes.length ? alphabet[((b & 15) << 2) | (c >> 6)] : '=';
    output += index + 2 < bytes.length ? alphabet[c & 63] : '=';
  }
  return output;
}

function decodeBase64(value: string): Uint8Array {
  if (value.length % 4 !== 0) throw new VontaqFSError('INTERNAL_ERROR', 'Runtime returned malformed Base64 file data.');
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const output: number[] = [];
  for (let index = 0; index < value.length; index += 4) {
    const chunk = value.slice(index, index + 4);
    const a = alphabet.indexOf(chunk[0] ?? ''); const b = alphabet.indexOf(chunk[1] ?? '');
    const c = chunk[2] === '=' ? 0 : alphabet.indexOf(chunk[2] ?? '');
    const d = chunk[3] === '=' ? 0 : alphabet.indexOf(chunk[3] ?? '');
    if (a < 0 || b < 0 || c < 0 || d < 0 || (chunk[2] === '=' && chunk[3] !== '=')) throw new VontaqFSError('INTERNAL_ERROR', 'Runtime returned malformed Base64 file data.');
    output.push((a << 2) | (b >> 4));
    if (chunk[2] !== '=') output.push(((b & 15) << 4) | (c >> 2));
    if (chunk[3] !== '=') output.push(((c & 3) << 6) | d);
  }
  return Uint8Array.from(output);
}

function concatenate(chunks: readonly Uint8Array[], total: number): Uint8Array {
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { out.set(chunk, offset); offset += chunk.byteLength; }
  return out;
}

function* textSlices(value: string, maxCodeUnits: number): Generator<string> {
  for (let offset = 0; offset < value.length;) {
    let end = Math.min(value.length, offset + maxCodeUnits);
    if (end < value.length) {
      const code = value.charCodeAt(end - 1);
      if (code >= 0xd800 && code <= 0xdbff) end -= 1;
    }
    if (end === offset) end += 1;
    yield value.slice(offset, end);
    offset = end;
  }
}

function* jsonFragments(root: unknown): Generator<string> {
  const seen = new Set<object>();
  let emittedRoot = false;
  function* encode(value: unknown, inArray: boolean): Generator<string> {
    if (value && typeof value === 'object' && typeof (value as { toJSON?: () => unknown }).toJSON === 'function') {
      value = (value as { toJSON: () => unknown }).toJSON();
    }
    if (value === null) { yield 'null'; return; }
    switch (typeof value) {
      case 'string': yield* jsonStringFragments(value); return;
      case 'number': yield Number.isFinite(value) ? String(value) : 'null'; return;
      case 'boolean': yield value ? 'true' : 'false'; return;
      case 'bigint': throw new TypeError('Do not know how to serialize a BigInt');
      case 'undefined':
      case 'function':
      case 'symbol':
        if (inArray) yield 'null';
        return;
      case 'object': break;
      default: return;
    }
    const objectValue = value as object;
    if (seen.has(objectValue)) throw new TypeError('Converting circular structure to JSON');
    seen.add(objectValue);
    try {
      if (Array.isArray(value)) {
        yield '[';
        for (let index = 0; index < value.length; index += 1) {
          if (index > 0) yield ',';
          yield* encode(value[index], true);
        }
        yield ']';
      } else {
        yield '{';
        let first = true;
        for (const key of Object.keys(value as Record<string, unknown>)) {
          const item = (value as Record<string, unknown>)[key];
          if (item === undefined || typeof item === 'function' || typeof item === 'symbol') continue;
          if (!first) yield ',';
          first = false;
          yield* jsonStringFragments(key);
          yield ':';
          yield* encode(item, false);
        }
        yield '}';
      }
    } finally {
      seen.delete(objectValue);
    }
  }
  for (const fragment of encode(root, false)) { emittedRoot = true; yield fragment; }
  if (!emittedRoot) throw new TypeError('writeJSON() requires a JSON-compatible root value.');
}

function* jsonStringFragments(value: string): Generator<string> {
  yield '"';
  let buffer = '';
  const flush = function* (): Generator<string> { if (buffer) { const current = buffer; buffer = ''; yield current; } };
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    let escaped: string | undefined;
    if (code === 0x22) escaped = '\\"';
    else if (code === 0x5c) escaped = '\\\\';
    else if (code === 0x08) escaped = '\\b';
    else if (code === 0x0c) escaped = '\\f';
    else if (code === 0x0a) escaped = '\\n';
    else if (code === 0x0d) escaped = '\\r';
    else if (code === 0x09) escaped = '\\t';
    else if (code < 0x20) escaped = `\\u${code.toString(16).padStart(4, '0')}`;
    else if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) { buffer += value[index] + value[index + 1]; index += 1; }
      else escaped = `\\u${code.toString(16).padStart(4, '0')}`;
    } else if (code >= 0xdc00 && code <= 0xdfff) escaped = `\\u${code.toString(16).padStart(4, '0')}`;
    else buffer += value[index];
    if (escaped !== undefined) { yield* flush(); yield escaped; }
    if (buffer.length >= 16 * 1024) yield* flush();
  }
  yield* flush();
  yield '"';
}

async function bestEffortAbort(writer: VontaqFSWriter): Promise<void> {
  try { await writer.abort(); } catch { /* original operation error wins */ }
}

async function deleteState(store: ClientStateStore, key: string): Promise<void> {
  if (store.delete) await store.delete(key);
  else await store.set(key, '');
}

function abortError(): Error {
  const DomException = (globalThis as { DOMException?: typeof DOMException }).DOMException;
  return DomException ? new DomException('The VontaqFS operation was aborted.', 'AbortError') : Object.assign(new Error('The VontaqFS operation was aborted.'), { name: 'AbortError' });
}

function safeCause(error: unknown): string { return error instanceof Error ? error.message : String(error ?? 'unknown error'); }
function sleep(milliseconds: number): Promise<void> { return new Promise(resolve => setTimeout(resolve, milliseconds)); }

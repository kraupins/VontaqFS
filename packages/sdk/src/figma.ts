import type {
  ClientStateStore,
  VontaqFSConnectOptions,
  VontaqFSHttpRequest,
  VontaqFSHttpResponse,
  VontaqFSSecureRandomSource,
  VontaqFSTransport,
} from './index.js';
import type { ClientIdentity } from './protocol.js';
import { VontaqFSError, normalizeErrorCode } from './errors.js';
import { VONTAQ_FS_ENDPOINTS } from './protocol.js';

const DOCUMENT_BINDING_KEY = 'vontaqfs.document-binding.v1';
const FIGMA_MESSAGE_NAMESPACE = 'vontaq-fs:figma:v1';
const ENTROPY_BATCH_BYTES = 32 * 1024;
const ENTROPY_LOW_WATER_BYTES = 4 * 1024;
const ENTROPY_READY_TIMEOUT_MS = 10_000;

/** Ready-to-copy Figma manifest networkAccess fragment for published VontaqFS clients. */
export const VONTAQ_FS_FIGMA_NETWORK_ACCESS = Object.freeze({
  allowedDomains: Object.freeze([...VONTAQ_FS_ENDPOINTS]),
  reasoning: 'Connects to the locally installed VontaqFS runtime for local persistent storage.',
});

export function createVontaqFSFigmaNetworkAccess(): { allowedDomains: string[]; reasoning: string } {
  return { allowedDomains: [...VONTAQ_FS_FIGMA_NETWORK_ACCESS.allowedDomains], reasoning: VONTAQ_FS_FIGMA_NETWORK_ACCESS.reasoning };
}

export interface FigmaClientStorageLike {
  getAsync(key: string): Promise<unknown>;
  setAsync(key: string, value: unknown): Promise<void>;
  deleteAsync?(key: string): Promise<void>;
}

export interface FigmaDocumentNodeLike {
  name?: string;
  getPluginData(key: string): string;
  setPluginData(key: string, value: string): void;
}

export interface FigmaApiLike {
  pluginId?: string;
  widgetId?: string;
  clientStorage: FigmaClientStorageLike;
  root: FigmaDocumentNodeLike;
}

export interface FigmaDocumentBinding {
  id: string;
  version: 1;
  displayName?: string;
}

export interface FigmaMainHostAdapter {
  readonly transport: VontaqFSTransport;
  readonly secureRandom: VontaqFSSecureRandomSource;
  ready(): Promise<void>;
  handleUiMessage(message: unknown): boolean;
  close(): void;
}

export interface FigmaMainHostAdapterOptions {
  postMessage(message: unknown): void;
}

export interface FigmaUiHostOptions {
  postMessage(message: unknown): void;
}

export interface FigmaConnectOptions {
  displayName?: string;
  host?: FigmaMainHostAdapter;
}

export interface FigmaDocumentBindingOptions {
  secureRandom?: VontaqFSSecureRandomSource;
}

interface HostPending<T> {
  resolve(value: T): void;
  reject(error: unknown): void;
  timeout?: ReturnType<typeof setTimeout>;
}

type FigmaHostMessage =
  | { namespace: typeof FIGMA_MESSAGE_NAMESPACE; type: 'entropy-request'; requestId: string; byteLength: number }
  | { namespace: typeof FIGMA_MESSAGE_NAMESPACE; type: 'entropy-response'; requestId: string; bytesBase64?: string; error?: SerializedTransportError }
  | { namespace: typeof FIGMA_MESSAGE_NAMESPACE; type: 'http-request'; requestId: string; request: SerializedHttpRequest }
  | { namespace: typeof FIGMA_MESSAGE_NAMESPACE; type: 'http-response'; requestId: string; response?: SerializedHttpResponse; error?: SerializedTransportError };

interface SerializedHttpRequest {
  method: VontaqFSHttpRequest['method'];
  url: string;
  headers: Readonly<Record<string, string>>;
  body?: { kind: 'text'; value: string } | { kind: 'binary'; base64: string };
  responseType?: 'text' | 'binary';
  timeoutMs: number;
  purpose?: VontaqFSHttpRequest['purpose'];
  timeoutMode?: VontaqFSHttpRequest['timeoutMode'];
}

interface SerializedHttpResponse {
  status: number;
  body: { kind: 'text'; value: string } | { kind: 'binary'; base64: string };
  headers?: Readonly<Record<string, string>>;
}

interface SerializedTransportError {
  code: string;
  message: string;
}

export class FigmaClientStateStore implements ClientStateStore {
  constructor(private readonly storage: FigmaClientStorageLike) {}

  async get(key: string): Promise<string | null> {
    const value = await this.storage.getAsync(key);
    return typeof value === 'string' && value.length > 0 ? value : null;
  }

  set(key: string, value: string): Promise<void> {
    return this.storage.setAsync(key, value);
  }

  async delete(key: string): Promise<void> {
    if (this.storage.deleteAsync) await this.storage.deleteAsync(key);
    else await this.storage.setAsync(key, '');
  }
}

export function getFigmaClientIdentity(figma: FigmaApiLike, options: FigmaConnectOptions = {}): ClientIdentity {
  const pluginId = typeof figma.pluginId === 'string' && figma.pluginId.trim() ? figma.pluginId.trim() : undefined;
  const widgetId = typeof figma.widgetId === 'string' && figma.widgetId.trim() ? figma.widgetId.trim() : undefined;
  if (!pluginId && !widgetId) {
    throw new TypeError('Figma did not expose pluginId or widgetId for this integration.');
  }
  const externalId = pluginId ?? widgetId!;
  return {
    kind: pluginId ? 'figma-plugin' : 'figma-widget',
    externalId,
    displayName: options.displayName?.trim() || externalId,
  };
}

export function createFigmaConnectOptions(
  figma: FigmaApiLike,
  options: FigmaConnectOptions = {},
): Pick<VontaqFSConnectOptions, 'application' | 'stateStore'> & Partial<Pick<VontaqFSConnectOptions, 'transport' | 'secureRandom'>> {
  return {
    application: getFigmaClientIdentity(figma, options),
    stateStore: new FigmaClientStateStore(figma.clientStorage),
    ...(options.host ? { transport: options.host.transport, secureRandom: options.host.secureRandom } : {}),
  };
}

/**
 * Creates the Figma-main side of the supported VontaqFS host bridge.
 * The adapter is intentionally composable: callers retain ownership of figma.ui.onmessage.
 */
export function createFigmaMainHostAdapter(options: FigmaMainHostAdapterOptions): FigmaMainHostAdapter {
  let closed = false;
  let nextRequestId = 1;
  let entropy = new Uint8Array(0);
  let entropyOffset = 0;
  let entropyRefill: Promise<void> | null = null;
  const entropyPending = new Map<string, HostPending<Uint8Array>>();
  const httpPending = new Map<string, HostPending<VontaqFSHttpResponse>>();

  const requestId = (kind: string): string => `vfs-${kind}-${nextRequestId++}`;
  const ensureOpen = (): void => {
    if (closed) throw new VontaqFSError('TRANSPORT_CANCELLED', 'The VontaqFS Figma host adapter is closed.');
  };

  const post = (message: FigmaHostMessage): void => {
    ensureOpen();
    try {
      options.postMessage(message);
    } catch (error) {
      throw new VontaqFSError('TRANSPORT_ERROR', 'Failed to post a VontaqFS host message to the Figma UI.', { cause: safeErrorMessage(error) });
    }
  };

  const requestEntropy = async (): Promise<void> => {
    ensureOpen();
    if (entropyRefill) return entropyRefill;
    const id = requestId('entropy');
    entropyRefill = new Promise<void>((resolve, reject) => {
      const pending: HostPending<Uint8Array> = {
        resolve(bytes) {
          if (bytes.byteLength === 0) {
            reject(new VontaqFSError('INTERNAL_ERROR', 'Figma UI returned an empty secure entropy batch.'));
            return;
          }
          const remaining = entropy.subarray(entropyOffset);
          const merged = new Uint8Array(remaining.byteLength + bytes.byteLength);
          merged.set(remaining, 0);
          merged.set(bytes, remaining.byteLength);
          entropy = merged;
          entropyOffset = 0;
          resolve();
        },
        reject,
      };
      pending.timeout = setTimeout(() => {
        entropyPending.delete(id);
        reject(new VontaqFSError('TRANSPORT_TIMEOUT', 'Timed out waiting for secure entropy from the Figma UI.'));
      }, ENTROPY_READY_TIMEOUT_MS);
      entropyPending.set(id, pending);
      try {
        post({ namespace: FIGMA_MESSAGE_NAMESPACE, type: 'entropy-request', requestId: id, byteLength: ENTROPY_BATCH_BYTES });
      } catch (error) {
        entropyPending.delete(id);
        if (pending.timeout) clearTimeout(pending.timeout);
        reject(error);
      }
    }).finally(() => {
      entropyRefill = null;
    });
    return entropyRefill;
  };

  const secureRandom: VontaqFSSecureRandomSource = {
    getRandomValues(target: Uint8Array): Uint8Array {
      ensureOpen();
      const available = entropy.byteLength - entropyOffset;
      if (target.byteLength > available) {
        if (!entropyRefill) void requestEntropy().catch(() => undefined);
        throw new VontaqFSError('INTERNAL_ERROR', 'Secure entropy is not ready in the Figma main host adapter. Call await host.ready() before connecting.');
      }
      target.set(entropy.subarray(entropyOffset, entropyOffset + target.byteLength));
      entropyOffset += target.byteLength;
      if (entropy.byteLength - entropyOffset < ENTROPY_LOW_WATER_BYTES && !entropyRefill) {
        void requestEntropy().catch(() => undefined);
      }
      return target;
    },
  };

  const transport: VontaqFSTransport = {
    request(request): Promise<VontaqFSHttpResponse> {
      ensureOpen();
      assertAllowedVfsUrl(request.url);
      const id = requestId('http');
      return new Promise<VontaqFSHttpResponse>((resolve, reject) => {
        const pending: HostPending<VontaqFSHttpResponse> = { resolve, reject };
        if ((request.timeoutMode ?? 'hard') === 'hard') {
          pending.timeout = setTimeout(() => {
            httpPending.delete(id);
            reject(new VontaqFSError('TRANSPORT_TIMEOUT', 'VontaqFS Figma relay deadline expired.', { purpose: request.purpose }));
          }, request.timeoutMs);
        }
        httpPending.set(id, pending);
        try {
          post({ namespace: FIGMA_MESSAGE_NAMESPACE, type: 'http-request', requestId: id, request: serializeHttpRequest(request) });
        } catch (error) {
          httpPending.delete(id);
          if (pending.timeout) clearTimeout(pending.timeout);
          reject(error);
        }
      });
    },
  };

  return {
    transport,
    secureRandom,
    async ready(): Promise<void> {
      ensureOpen();
      if (entropy.byteLength - entropyOffset < ENTROPY_LOW_WATER_BYTES) await requestEntropy();
    },
    handleUiMessage(message: unknown): boolean {
      if (!isHostMessage(message)) return false;
      if (message.type === 'entropy-response') {
        const pending = entropyPending.get(message.requestId);
        if (!pending) return true;
        entropyPending.delete(message.requestId);
        if (pending.timeout) clearTimeout(pending.timeout);
        if (message.error) pending.reject(deserializeError(message.error));
        else {
          try {
            pending.resolve(decodeBase64(message.bytesBase64 ?? ''));
          } catch (error) {
            pending.reject(error);
          }
        }
        return true;
      }
      if (message.type === 'http-response') {
        const pending = httpPending.get(message.requestId);
        if (!pending) return true;
        httpPending.delete(message.requestId);
        if (pending.timeout) clearTimeout(pending.timeout);
        if (message.error) pending.reject(deserializeError(message.error));
        else if (message.response) {
          try {
            pending.resolve(deserializeHttpResponse(message.response));
          } catch (error) {
            pending.reject(error);
          }
        } else {
          pending.reject(new VontaqFSError('TRANSPORT_ERROR', 'Figma UI returned an empty VontaqFS HTTP response.'));
        }
        return true;
      }
      return false;
    },
    close(): void {
      if (closed) return;
      closed = true;
      const error = new VontaqFSError('TRANSPORT_CANCELLED', 'The VontaqFS Figma host adapter was closed.');
      for (const pending of entropyPending.values()) {
        if (pending.timeout) clearTimeout(pending.timeout);
        pending.reject(error);
      }
      for (const pending of httpPending.values()) {
        if (pending.timeout) clearTimeout(pending.timeout);
        pending.reject(error);
      }
      entropyPending.clear();
      httpPending.clear();
      entropy = new Uint8Array(0);
      entropyOffset = 0;
    },
  };
}

/**
 * Handles one VontaqFS host message in the Figma UI/browser realm.
 * Returns false for unrelated application messages so the caller can continue its own dispatch.
 */
export async function handleVontaqFSFigmaUiMessage(message: unknown, options: FigmaUiHostOptions): Promise<boolean> {
  if (!isHostMessage(message)) return false;

  if (message.type === 'entropy-request') {
    const response: FigmaHostMessage = { namespace: FIGMA_MESSAGE_NAMESPACE, type: 'entropy-response', requestId: message.requestId };
    try {
      if (!Number.isInteger(message.byteLength) || message.byteLength <= 0 || message.byteLength > 65_536) {
        throw new VontaqFSError('REQUEST_INVALID', 'Invalid Figma entropy request size.');
      }
      const cryptoApi = globalThis.crypto;
      if (!cryptoApi || typeof cryptoApi.getRandomValues !== 'function') {
        throw new VontaqFSError('INTERNAL_ERROR', 'Figma UI does not provide Web Crypto secure random.');
      }
      const bytes = new Uint8Array(message.byteLength);
      cryptoApi.getRandomValues(bytes);
      response.bytesBase64 = encodeBase64(bytes);
    } catch (error) {
      response.error = serializeError(error);
    }
    options.postMessage(response);
    return true;
  }

  if (message.type === 'http-request') {
    const responseMessage: FigmaHostMessage = { namespace: FIGMA_MESSAGE_NAMESPACE, type: 'http-response', requestId: message.requestId };
    let timedOut = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | undefined;
    try {
      const request = message.request;
      assertAllowedVfsUrl(request.url);
      const fetchFn = (globalThis as { fetch?: typeof fetch }).fetch;
      if (typeof fetchFn !== 'function') throw new VontaqFSError('TRANSPORT_ERROR', 'Figma UI does not provide fetch().');
      const timeoutMode = request.timeoutMode ?? 'hard';
      if (timeoutMode === 'hard') {
        const AbortControllerCtor = (globalThis as { AbortController?: typeof AbortController }).AbortController;
        if (typeof AbortControllerCtor !== 'function') {
          throw new VontaqFSError('TRANSPORT_ERROR', 'Figma UI does not provide AbortController required for a hard transport timeout.');
        }
        controller = new AbortControllerCtor();
        timeout = setTimeout(() => {
          timedOut = true;
          controller?.abort();
        }, request.timeoutMs);
      }
      const body = request.body?.kind === 'binary'
        ? materializeArrayBuffer(decodeBase64(request.body.base64))
        : request.body?.value;
      const fetched = await fetchFn(request.url, {
        method: request.method,
        headers: request.headers,
        ...(body === undefined ? {} : { body }),
        ...(controller ? { signal: controller.signal } : {}),
        cache: 'no-store',
      });
      responseMessage.response = {
        status: fetched.status,
        body: request.responseType === 'binary'
          ? { kind: 'binary', base64: encodeBase64(new Uint8Array(await fetched.arrayBuffer())) }
          : { kind: 'text', value: await fetched.text() },
        headers: collectHeaders(fetched.headers),
      };
    } catch (error) {
      if (timedOut) responseMessage.error = serializeError(new VontaqFSError('TRANSPORT_TIMEOUT', 'VontaqFS Figma UI fetch deadline expired.'));
      else if (isAbortLikeError(error)) responseMessage.error = serializeError(new VontaqFSError('TRANSPORT_CANCELLED', 'VontaqFS Figma UI fetch was cancelled.'));
      else responseMessage.error = serializeError(error instanceof VontaqFSError ? error : new VontaqFSError('TRANSPORT_ERROR', 'VontaqFS Figma UI fetch failed.', { cause: safeErrorMessage(error) }));
    } finally {
      if (timeout !== undefined) clearTimeout(timeout);
    }
    options.postMessage(responseMessage);
    return true;
  }

  return false;
}

export async function bindFigmaDocument(figma: FigmaApiLike, options: FigmaDocumentBindingOptions = {}): Promise<FigmaDocumentBinding> {
  const existing = figma.root.getPluginData(DOCUMENT_BINDING_KEY);
  if (existing) {
    const parsed = parseBinding(existing);
    if (parsed.kind === 'current') {
      return { id: parsed.id, version: 1, displayName: normalizedDisplayName(figma.root.name) };
    }
    if (parsed.kind === 'legacy') {
      figma.root.setPluginData(DOCUMENT_BINDING_KEY, JSON.stringify({ version: 1, id: parsed.id }));
      return { id: parsed.id, version: 1, displayName: normalizedDisplayName(figma.root.name) };
    }
    if (parsed.kind === 'unsupported') {
      throw new VontaqFSError('PROTOCOL_INCOMPATIBLE', 'This document uses an unsupported VontaqFS binding version.', { bindingVersion: parsed.version });
    }
    throw new VontaqFSError('STORAGE_CORRUPT', 'The VontaqFS document binding is malformed.');
  }

  const binding: FigmaDocumentBinding = {
    id: secureBindingId(options.secureRandom),
    version: 1,
    displayName: normalizedDisplayName(figma.root.name),
  };
  figma.root.setPluginData(DOCUMENT_BINDING_KEY, JSON.stringify({ version: 1, id: binding.id }));
  return binding;
}

function parseBinding(value: string):
  | { kind: 'current'; id: string }
  | { kind: 'legacy'; id: string }
  | { kind: 'unsupported'; version: unknown }
  | { kind: 'invalid' } {
  try {
    const parsed = JSON.parse(value) as { version?: unknown; id?: unknown };
    if (parsed.version === 1 && typeof parsed.id === 'string' && parsed.id.length > 0) {
      return { kind: 'current', id: parsed.id };
    }
    if (parsed.version !== undefined && parsed.version !== 1) return { kind: 'unsupported', version: parsed.version };
    return { kind: 'invalid' };
  } catch {
    // Pre-v1 development builds may have stored a raw non-secret ID. Preserve it while upgrading the envelope.
    if (/^[A-Za-z0-9._:-]{8,256}$/.test(value)) return { kind: 'legacy', id: value };
    return { kind: 'invalid' };
  }
}

function secureBindingId(source?: VontaqFSSecureRandomSource): string {
  const cryptoApi = globalThis.crypto;
  const provider = source ?? (cryptoApi && typeof cryptoApi.getRandomValues === 'function'
    ? { getRandomValues: (target: Uint8Array) => cryptoApi.getRandomValues(target) }
    : undefined);
  if (!provider) {
    throw new VontaqFSError('INTERNAL_ERROR', 'A cryptographically secure random source is required to bind a Figma document.');
  }
  const bytes = new Uint8Array(16);
  provider.getRandomValues(bytes);
  return `doc-${Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')}`;
}

function serializeHttpRequest(request: VontaqFSHttpRequest): SerializedHttpRequest {
  return {
    method: request.method,
    url: request.url,
    headers: request.headers,
    ...(request.body === undefined ? {} : {
      body: typeof request.body === 'string'
        ? { kind: 'text' as const, value: request.body }
        : { kind: 'binary' as const, base64: encodeBase64(request.body) },
    }),
    responseType: request.responseType,
    timeoutMs: request.timeoutMs,
    purpose: request.purpose,
    timeoutMode: request.timeoutMode,
  };
}

function deserializeHttpResponse(response: SerializedHttpResponse): VontaqFSHttpResponse {
  if (!Number.isInteger(response.status) || response.status < 100 || response.status > 599) {
    throw new VontaqFSError('TRANSPORT_ERROR', 'Figma UI returned an invalid HTTP status.');
  }
  return {
    status: response.status,
    body: response.body.kind === 'binary' ? decodeBase64(response.body.base64) : response.body.value,
    headers: response.headers,
  };
}

function serializeError(error: unknown): SerializedTransportError {
  if (error instanceof VontaqFSError) return { code: error.code, message: error.message };
  return { code: 'TRANSPORT_ERROR', message: safeErrorMessage(error) || 'VontaqFS host transport failed.' };
}

function deserializeError(error: SerializedTransportError): VontaqFSError {
  return new VontaqFSError(normalizeErrorCode(error.code), error.message || 'VontaqFS host transport failed.');
}

function isHostMessage(message: unknown): message is FigmaHostMessage {
  if (!message || typeof message !== 'object') return false;
  const candidate = message as Record<string, unknown>;
  if (candidate.namespace !== FIGMA_MESSAGE_NAMESPACE || typeof candidate.type !== 'string' || typeof candidate.requestId !== 'string') return false;
  return candidate.type === 'entropy-request'
    || candidate.type === 'entropy-response'
    || candidate.type === 'http-request'
    || candidate.type === 'http-response';
}

function assertAllowedVfsUrl(url: string): void {
  const allowed = VONTAQ_FS_ENDPOINTS.some(endpoint => {
    if (!url.startsWith(endpoint)) return false;
    const suffix = url.slice(endpoint.length);
    return suffix === '/v1' || suffix.startsWith('/v1/') || suffix.startsWith('/v1?');
  });
  if (!allowed) {
    throw new VontaqFSError('TRANSPORT_ERROR', 'Figma VontaqFS relay refused a URL outside the official localhost VFS endpoints.', { url });
  }
}

function materializeArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  return buffer;
}

function collectHeaders(headers: Headers): Record<string, string> {
  const result: Record<string, string> = {};
  headers.forEach((value, key) => { result[key] = value; });
  return result;
}

const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

function encodeBase64(bytes: Uint8Array): string {
  let result = '';
  for (let offset = 0; offset < bytes.length; offset += 3) {
    const a = bytes[offset] ?? 0;
    const b = bytes[offset + 1] ?? 0;
    const c = bytes[offset + 2] ?? 0;
    const triplet = (a << 16) | (b << 8) | c;
    result += BASE64_ALPHABET[(triplet >> 18) & 63];
    result += BASE64_ALPHABET[(triplet >> 12) & 63];
    result += offset + 1 < bytes.length ? BASE64_ALPHABET[(triplet >> 6) & 63] : '=';
    result += offset + 2 < bytes.length ? BASE64_ALPHABET[triplet & 63] : '=';
  }
  return result;
}

function decodeBase64(value: string): Uint8Array {
  if (value.length % 4 !== 0 || /[^A-Za-z0-9+/=]/.test(value)) throw new VontaqFSError('TRANSPORT_ERROR', 'Invalid base64 data in Figma VontaqFS relay.');
  const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0;
  const output = new Uint8Array((value.length / 4) * 3 - padding);
  let out = 0;
  for (let offset = 0; offset < value.length; offset += 4) {
    let bits = 0;
    for (let index = 0; index < 4; index += 1) {
      const ch = value[offset + index];
      const code = ch === '=' ? 0 : BASE64_ALPHABET.indexOf(ch);
      if (code < 0) throw new VontaqFSError('TRANSPORT_ERROR', 'Invalid base64 data in Figma VontaqFS relay.');
      bits = (bits << 6) | code;
    }
    if (out < output.length) output[out++] = (bits >> 16) & 0xff;
    if (out < output.length) output[out++] = (bits >> 8) & 0xff;
    if (out < output.length) output[out++] = bits & 0xff;
  }
  return output;
}

function isAbortLikeError(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && (error as { name?: unknown }).name === 'AbortError');
}

function safeErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error ?? '');
}

function normalizedDisplayName(value: string | undefined): string | undefined {
  const normalized = value?.trim();
  return normalized || undefined;
}

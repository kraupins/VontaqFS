import type { ClientStateStore, VontaqFSConnectOptions } from './index.js';
import type { ClientIdentity } from './protocol.js';
import { VontaqFSError } from './errors.js';
import { VONTAQ_FS_ENDPOINTS } from './protocol.js';

const DOCUMENT_BINDING_KEY = 'vontaqfs.document-binding.v1';

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

export interface FigmaConnectOptions {
  displayName?: string;
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
): Pick<VontaqFSConnectOptions, 'application' | 'stateStore'> {
  return {
    application: getFigmaClientIdentity(figma, options),
    stateStore: new FigmaClientStateStore(figma.clientStorage),
  };
}

export async function bindFigmaDocument(figma: FigmaApiLike): Promise<FigmaDocumentBinding> {
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
    id: secureBindingId(),
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

function secureBindingId(): string {
  const cryptoApi = globalThis.crypto;
  if (!cryptoApi || typeof cryptoApi.getRandomValues !== 'function') {
    throw new VontaqFSError('INTERNAL_ERROR', 'A cryptographically secure random source is required to bind a Figma document.');
  }
  const bytes = new Uint8Array(16);
  cryptoApi.getRandomValues(bytes);
  return `doc-${Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')}`;
}

function normalizedDisplayName(value: string | undefined): string | undefined {
  const normalized = value?.trim();
  return normalized || undefined;
}

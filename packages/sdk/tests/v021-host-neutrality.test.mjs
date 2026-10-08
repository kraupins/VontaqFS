import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { TextDecoder as NodeTextDecoder, TextEncoder as NodeTextEncoder } from 'node:util';
import {
  FetchTransport,
  VONTAQ_FS_ERROR_CODES,
  VontaqFS,
  VontaqFSError,
} from '../dist/index.js';
import { PortableUtf8Decoder, utf8Decode, utf8Encode } from '../dist/utf8.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class MemoryStateStore {
  constructor(entries = {}) { this.values = new Map(Object.entries(entries)); }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
  async delete(key) { this.values.delete(key); }
}

class HostNeutralRuntime {
  constructor() {
    this.calls = [];
    this.files = new Map();
    this.failStat = false;
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    this.calls.push({ ...request, path });
    if (path === '/v1/health') return ok({
      service: 'vontaqfs', runtimeVersion: '0.2.1', protocol: { min: 1, max: 1 },
      storageFormatVersion: 1, status: 'ready', capabilities: ['files', 'kv', 'spaces', 'events'],
    });
    const body = request.body && typeof request.body === 'string' ? JSON.parse(request.body) : {};
    if (path === '/v1/pairings/request') {
      return { status: 202, body: JSON.stringify({ pairingId: 'pair-1', status: 'pending', expiresAtMs: Date.now() + 10_000 }) };
    }
    if (path === '/v1/pairings/poll') return ok({ pairingId: 'pair-1', status: 'approved', expiresAtMs: Date.now() + 10_000, pairingCredential: CREDENTIAL });
    if (path === '/v1/sessions') return ok({ token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: ['files', 'kv', 'spaces', 'events'] });
    if (path === '/v1/spaces/open') return ok(space(body.key));
    if (path === '/v1/fs/write-small') {
      const bytes = Buffer.from(body.dataBase64, 'base64');
      const info = { path: body.path, version: 1, etag: 'etag-1', size: bytes.byteLength, updatedAtMs: Date.now() };
      this.files.set(body.path, { bytes, info });
      return ok(info);
    }
    if (path === '/v1/fs/stat') {
      if (this.failStat) throw new Error('socket reset');
      return ok({ file: this.files.get(body.path)?.info ?? null });
    }
    if (path === '/v1/fs/read-small') {
      const record = this.files.get(body.path);
      if (!record) return error(404, 'NOT_FOUND', 'missing');
      return ok({ dataBase64: record.bytes.toString('base64'), file: record.info });
    }
    if (path === '/v1/events/poll') {
      if ((body.waitMs ?? 0) > 0) return new Promise(() => {});
      return ok({ events: [], latestSequence: 1, overflow: false });
    }
    throw new Error(`Unhandled fake path: ${path}`);
  }
}

function ok(body) { return { status: 200, body: JSON.stringify(body) }; }
function error(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function space(key = 'default') {
  return { id: `space-${key}`, key, displayName: null, storageClass: 'persistent', storageCategory: 'user-data', createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy' };
}
function secureRandom() {
  let next = 1;
  return {
    getRandomValues(target) {
      for (let index = 0; index < target.length; index += 1) target[index] = next++ & 0xff;
      return target;
    },
  };
}
function options(runtime, extra = {}) {
  return {
    application: { kind: 'figma-plugin', externalId: 'host-neutral-test', displayName: 'Host Neutral Test' },
    stateStore: new MemoryStateStore(),
    developmentEndpoint: 'http://localhost:47833',
    transport: runtime,
    pairingPollIntervalMs: 0,
    secureRandom: secureRandom(),
    retryCount: 0,
    ...extra,
  };
}

function replaceGlobal(name, value) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  return () => {
    if (descriptor) Object.defineProperty(globalThis, name, descriptor);
    else delete globalThis[name];
  };
}

test('portable UTF-8 matches platform encoding for valid Unicode and handles stream boundaries', () => {
  const nativeEncoder = new NodeTextEncoder();
  const nativeDecoder = new NodeTextDecoder('utf-8', { fatal: true });
  for (const value of ['', 'ASCII', 'Zażółć gęślą', '日本語', 'emoji 😀🧪', 'lone \ud800 surrogate']) {
    assert.deepEqual([...utf8Encode(value)], [...nativeEncoder.encode(value)]);
    assert.equal(utf8Decode(utf8Encode(value), { fatal: true }), nativeDecoder.decode(nativeEncoder.encode(value)));
  }

  const decoder = new PortableUtf8Decoder({ fatal: true });
  const emoji = utf8Encode('A😀B');
  assert.equal(decoder.decode(emoji.subarray(0, 3), { stream: true }), 'A');
  assert.equal(decoder.decode(emoji.subarray(3, 5), { stream: true }), '😀');
  assert.equal(decoder.decode(emoji.subarray(5)), 'B');
});

test('portable UTF-8 fatal decode rejects malformed and incomplete sequences', () => {
  for (const bytes of [
    Uint8Array.of(0xc0, 0x80),
    Uint8Array.of(0xe0, 0x80, 0x80),
    Uint8Array.of(0xed, 0xa0, 0x80),
    Uint8Array.of(0xf4, 0x90, 0x80, 0x80),
    Uint8Array.of(0xf0, 0x9f),
  ]) assert.throws(() => utf8Decode(bytes, { fatal: true }), /UTF-8/);
});

test('core connects and reads/writes text without global crypto/TextEncoder/TextDecoder when secureRandom is injected', async () => {
  const restores = [
    replaceGlobal('crypto', undefined),
    replaceGlobal('TextEncoder', undefined),
    replaceGlobal('TextDecoder', undefined),
  ];
  try {
    const runtime = new HostNeutralRuntime();
    const fs = await VontaqFS.connect(options(runtime));
    await fs.writeText('/unicode.txt', 'Vontaq 😀 日本語');
    assert.equal(await fs.readText('/unicode.txt'), 'Vontaq 😀 日本語');
    const requestIds = runtime.calls
      .filter(call => typeof call.body === 'string')
      .map(call => JSON.parse(call.body).requestId)
      .filter(Boolean);
    assert.ok(requestIds.length > 0);
    assert.ok(requestIds.every(id => /^request-[0-9a-f]{32}$/.test(id)));
  } finally {
    for (const restore of restores.reverse()) restore();
  }
});

test('core fails closed when neither injected nor native secure random is available', async () => {
  const restore = replaceGlobal('crypto', undefined);
  try {
    await assert.rejects(
      () => VontaqFS.connect({ ...options(new HostNeutralRuntime()), secureRandom: undefined }),
      errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'INTERNAL_ERROR',
    );
  } finally {
    restore();
  }
});

test('discovery timeout is hard and separate while implicit operational requests have no wall-clock timeout', async () => {
  const runtime = new HostNeutralRuntime();
  const fs = await VontaqFS.connect(options(runtime, { discoveryTimeoutMs: 777 }));
  const health = runtime.calls.filter(call => call.path === '/v1/health');
  assert.ok(health.length >= 2);
  assert.ok(health.every(call => call.purpose === 'discovery' && call.timeoutMode === 'hard' && call.timeoutMs === 777));
  const open = runtime.calls.find(call => call.path === '/v1/spaces/open');
  assert.equal(open.purpose, 'operational');
  assert.equal(open.timeoutMode, 'none');
  assert.equal(open.timeoutMs, 10_000);
  await fs.close();
});

test('explicit requestTimeoutMs preserves legacy hard operational timeout intent', async () => {
  const runtime = new HostNeutralRuntime();
  await VontaqFS.connect(options(runtime, { requestTimeoutMs: 4321 }));
  const open = runtime.calls.find(call => call.path === '/v1/spaces/open');
  assert.equal(open.purpose, 'operational');
  assert.equal(open.timeoutMode, 'hard');
  assert.equal(open.timeoutMs, 4321);
});

test('watch long-poll timing is independent from explicit operational timeout', async () => {
  const runtime = new HostNeutralRuntime();
  const fs = await VontaqFS.connect(options(runtime, { requestTimeoutMs: 500 }));
  const unsubscribe = await fs.watch('/', () => {}, { waitMs: 25_000 });
  const poll = runtime.calls.find(call => call.path === '/v1/events/poll');
  assert.equal(poll.purpose, 'long-poll');
  assert.equal(poll.timeoutMode, 'hard');
  assert.equal(poll.timeoutMs, 30_000);
  unsubscribe();
});

test('operational transport exhaustion is TRANSPORT_ERROR rather than RUNTIME_UNREACHABLE', async () => {
  const runtime = new HostNeutralRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  runtime.failStat = true;
  await assert.rejects(
    () => fs.files.exists('/missing'),
    errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'TRANSPORT_ERROR',
  );
});

test('new transport taxonomy is public and FetchTransport classifies missing fetch', async () => {
  for (const code of ['TRANSPORT_ERROR', 'TRANSPORT_TIMEOUT', 'TRANSPORT_CANCELLED', 'RUNTIME_DISCONNECTED']) {
    assert.ok(VONTAQ_FS_ERROR_CODES.includes(code));
  }
  const restore = replaceGlobal('fetch', undefined);
  try {
    await assert.rejects(
      () => new FetchTransport().request({ method: 'GET', url: 'http://localhost/', headers: {}, timeoutMs: 100, purpose: 'operational', timeoutMode: 'none' }),
      errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'TRANSPORT_ERROR',
    );
  } finally {
    restore();
  }
});

test('emitted SDK JavaScript is scanner-safe for import method syntax', async () => {
  const source = await readFile(new URL('../dist/index.js', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /\bimport\s*\(/);
  assert.match(source, /async\s+\["import"\]\s*\(/);
});

test('FetchTransport distinguishes hard deadline timeout from cancellation and supports timeoutMode none without AbortController', async () => {
  const originalFetch = globalThis.fetch;
  const originalAbortController = globalThis.AbortController;
  try {
    globalThis.fetch = async (_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener('abort', () => {
        const errorValue = new Error('aborted');
        errorValue.name = 'AbortError';
        reject(errorValue);
      }, { once: true });
    });
    await assert.rejects(
      () => new FetchTransport().request({ method: 'GET', url: 'http://localhost/', headers: {}, timeoutMs: 5, purpose: 'discovery', timeoutMode: 'hard' }),
      errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'TRANSPORT_TIMEOUT',
    );

    globalThis.fetch = async () => { const errorValue = new Error('cancelled'); errorValue.name = 'AbortError'; throw errorValue; };
    await assert.rejects(
      () => new FetchTransport().request({ method: 'GET', url: 'http://localhost/', headers: {}, timeoutMs: 5, purpose: 'operational', timeoutMode: 'none' }),
      errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'TRANSPORT_CANCELLED',
    );

    globalThis.AbortController = undefined;
    globalThis.fetch = async () => ({ status: 200, text: async () => 'ok', arrayBuffer: async () => new ArrayBuffer(0) });
    assert.equal((await new FetchTransport().request({ method: 'GET', url: 'http://localhost/', headers: {}, timeoutMs: 5, purpose: 'operational', timeoutMode: 'none' })).body, 'ok');
  } finally {
    globalThis.fetch = originalFetch;
    globalThis.AbortController = originalAbortController;
  }
});

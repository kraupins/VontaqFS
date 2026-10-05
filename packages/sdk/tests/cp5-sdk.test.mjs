import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS, VontaqFSError } from '../dist/index.js';
import { bindFigmaDocument, createFigmaConnectOptions } from '../dist/figma.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);
const TOKEN_2 = 'c'.repeat(64);

class MemoryStateStore {
  constructor(entries = {}) { this.values = new Map(Object.entries(entries)); }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
  async delete(key) { this.values.delete(key); }
}

class FakeRuntime {
  constructor() {
    this.calls = [];
    this.pairRequested = 0;
    this.sessionCount = 0;
    this.revoked = false;
    this.authInvalidOnce = false;
    this.failKvSetTransportOnce = false;
    this.failedKvSetBodies = [];
    this.kv = new Map();
    this.files = new Map();
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    this.calls.push({ path, ...request });
    if (path === '/v1/health') return ok({
      service: 'vontaqfs', runtimeVersion: '0.1.0', protocol: { min: 1, max: 1 },
      storageFormatVersion: 1, status: 'ready', capabilities: ['files', 'kv', 'spaces'],
    });

    const body = request.body ? JSON.parse(request.body) : {};
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      const mac = createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex');
      return ok({ mac });
    }
    if (path === '/v1/pairings/request') {
      this.pairRequested += 1;
      return { status: 202, body: JSON.stringify({ pairingId: 'pair-1', status: 'pending', expiresAtMs: Date.now() + 10_000 }) };
    }
    if (path === '/v1/pairings/poll') {
      return ok({ pairingId: body.pairingId, status: 'approved', expiresAtMs: Date.now() + 10_000, pairingCredential: CREDENTIAL });
    }
    if (path === '/v1/sessions') {
      this.sessionCount += 1;
      if (body.pairingCredential !== CREDENTIAL) return error(401, 'AUTH_INVALID', 'bad credential');
      return ok({ token: this.sessionCount === 1 ? TOKEN : TOKEN_2, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: ['files', 'kv', 'spaces'] });
    }

    if (this.revoked) return error(401, 'AUTH_REVOKED', 'revoked');
    if (this.authInvalidOnce) {
      this.authInvalidOnce = false;
      return error(401, 'AUTH_INVALID', 'expired');
    }
    if (!String(request.headers.Authorization ?? '').startsWith('Bearer ')) return error(401, 'AUTH_INVALID', 'missing');

    if (path === '/v1/spaces/open') return ok(space(body.key, body.storageClass, body.displayName));
    if (path === '/v1/spaces/list') return ok({ spaces: [space('default', 'persistent', null)] });
    if (path === '/v1/kv/get') return ok({ entry: this.kv.get(body.key) ?? null });
    if (path === '/v1/kv/set') {
      if (this.failKvSetTransportOnce) {
        this.failKvSetTransportOnce = false;
        this.failedKvSetBodies.push(request.body);
        throw new Error('socket reset after send');
      }
      this.failedKvSetBodies.push(request.body);
      const previous = this.kv.get(body.key);
      if (body.ifVersion != null && previous?.version !== body.ifVersion) return error(409, 'CONFLICT', 'stale version');
      const entry = { key: body.key, value: body.value, version: (previous?.version ?? 0) + 1, etag: `etag-${(previous?.version ?? 0) + 1}`, updatedAtMs: Date.now() };
      this.kv.set(body.key, entry);
      return ok(entry);
    }
    if (path === '/v1/fs/stat') return ok({ file: this.files.get(body.path)?.info ?? null });
    if (path === '/v1/fs/write-small') {
      const bytes = Buffer.from(body.dataBase64, 'base64');
      const info = { path: body.path, version: 1, etag: 'file-etag-1', size: bytes.length, updatedAtMs: Date.now() };
      this.files.set(body.path, { bytes, info });
      return ok(info);
    }
    if (path === '/v1/fs/read-small') {
      const record = this.files.get(body.path);
      if (!record) return error(404, 'NOT_FOUND', 'missing');
      return ok({ dataBase64: record.bytes.toString('base64'), file: record.info });
    }
    throw new Error(`Unhandled fake path: ${path}`);
  }
}

function ok(body) { return { status: 200, body: JSON.stringify(body) }; }
function error(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function space(key, storageClass = 'persistent', displayName = null) {
  return { id: `space-${key}`, key, displayName, storageClass, createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy' };
}
function options(runtime, stateStore = new MemoryStateStore({
  'vontaqfs.client-instance.v1': 'client-existing',
  'vontaqfs.pairing-credential.v1': CREDENTIAL,
})) {
  return {
    application: { kind: 'figma-plugin', externalId: '12345', displayName: 'SDK test plugin' },
    stateStore,
    developmentEndpoint: 'http://localhost:47833',
    transport: runtime,
    pairingPollIntervalMs: 0,
    requestTimeoutMs: 1000,
    retryCount: 1,
  };
}

test('connect reuses persisted credential and exposes default Files/KV APIs', async () => {
  const runtime = new FakeRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  assert.equal(runtime.pairRequested, 0);
  assert.equal(fs.defaultSpace.key, 'default');

  const saved = await fs.kv.set('resume', { revision: 1 });
  assert.equal(saved.version, 1);
  assert.deepEqual((await fs.kv.get('resume')).value, { revision: 1 });

  const file = await fs.writeJSON('/project.json', { hello: 'world' });
  assert.equal(file.path, '/project.json');
  assert.deepEqual(await fs.readJSON('/project.json'), { hello: 'world' });
  assert.equal(await fs.files.exists('/project.json'), true);
});

test('missing credential runs pairing flow once and persists approved credential', async () => {
  const runtime = new FakeRuntime();
  const store = new MemoryStateStore();
  let pairingEvent;
  const fs = await VontaqFS.connect({ ...options(runtime, store), onPairingRequired: event => { pairingEvent = event; } });
  assert.equal(runtime.pairRequested, 1);
  assert.equal(pairingEvent.pairingId, 'pair-1');
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), CREDENTIAL);
  assert.match(await store.get('vontaqfs.client-instance.v1'), /^client-[0-9a-f]{32}$/);
  await fs.close();
});

test('session AUTH_INVALID refreshes once without re-pairing', async () => {
  const runtime = new FakeRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  runtime.authInvalidOnce = true;
  assert.equal(await fs.files.exists('/missing'), false);
  assert.equal(runtime.sessionCount, 2);
  assert.equal(runtime.pairRequested, 0);
});

test('revoked pairing is surfaced without silently deleting the trusted credential', async () => {
  const runtime = new FakeRuntime();
  const store = new MemoryStateStore({
    'vontaqfs.client-instance.v1': 'client-existing',
    'vontaqfs.pairing-credential.v1': CREDENTIAL,
  });
  const fs = await VontaqFS.connect(options(runtime, store));
  runtime.revoked = true;
  await assert.rejects(() => fs.kv.get('x'), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'AUTH_REVOKED');
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), CREDENTIAL);
});

test('idempotent mutation retry reuses exactly the same request envelope', async () => {
  const runtime = new FakeRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  runtime.failKvSetTransportOnce = true;
  await fs.kv.set('retry', { ok: true });
  assert.equal(runtime.failedKvSetBodies.length, 2);
  assert.equal(runtime.failedKvSetBodies[0], runtime.failedKvSetBodies[1]);
  assert.match(JSON.parse(runtime.failedKvSetBodies[0]).requestId, /^request-[0-9a-f]{32}$/);
});

test('CAS conflict is normalized to typed CONFLICT', async () => {
  const runtime = new FakeRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  await fs.kv.set('revisioned', 1);
  await assert.rejects(() => fs.kv.set('revisioned', 2, { ifVersion: 0 }), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'CONFLICT');
});

test('unreachable and incompatible runtime states use stable typed errors', async () => {
  const unreachable = { async request() { throw new Error('ECONNREFUSED'); } };
  await assert.rejects(() => VontaqFS.connect(options(unreachable, new MemoryStateStore())), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'RUNTIME_UNREACHABLE');
  await assert.rejects(() => VontaqFS.connect(options(unreachable)), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'PAIRING_RUNTIME_NOT_FOUND');

  const incompatible = { async request(request) {
    if (new URL(request.url).pathname === '/v1/health') return ok({ service: 'vontaqfs', runtimeVersion: '9.0.0', protocol: { min: 9, max: 9 }, storageFormatVersion: 9, status: 'ready', capabilities: [] });
    throw new Error('unexpected');
  } };
  await assert.rejects(() => VontaqFS.connect(options(incompatible)), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'PROTOCOL_INCOMPATIBLE');
});

test('Figma helper derives official plugin identity and clientStorage adapter', async () => {
  const storage = new Map();
  const figma = {
    pluginId: 'figma-plugin-1',
    clientStorage: {
      async getAsync(key) { return storage.get(key); },
      async setAsync(key, value) { storage.set(key, value); },
      async deleteAsync(key) { storage.delete(key); },
    },
    root: { name: 'Document', getPluginData() { return ''; }, setPluginData() {} },
  };
  const connectOptions = createFigmaConnectOptions(figma, { displayName: 'Example plugin' });
  assert.deepEqual(connectOptions.application, { kind: 'figma-plugin', externalId: 'figma-plugin-1', displayName: 'Example plugin' });
  await connectOptions.stateStore.set('x', 'y');
  assert.equal(await connectOptions.stateStore.get('x'), 'y');
});

test('Figma document binding is stable, versioned, and migrates legacy raw ids without changing identity', async () => {
  let stored = '';
  const root = {
    name: 'Design file',
    getPluginData() { return stored; },
    setPluginData(_key, value) { stored = value; },
  };
  const figma = { pluginId: 'figma-plugin-1', clientStorage: { async getAsync() {}, async setAsync() {} }, root };

  const first = await bindFigmaDocument(figma);
  const second = await bindFigmaDocument(figma);
  assert.equal(first.id, second.id);
  assert.equal(first.version, 1);
  assert.equal(first.displayName, 'Design file');
  assert.equal(JSON.parse(stored).version, 1);

  stored = 'legacy-binding-12345';
  const migrated = await bindFigmaDocument(figma);
  assert.equal(migrated.id, 'legacy-binding-12345');
  assert.deepEqual(JSON.parse(stored), { version: 1, id: 'legacy-binding-12345' });
});

test('future Figma binding versions fail closed instead of overwriting linkage', async () => {
  let stored = JSON.stringify({ version: 2, id: 'future-id' });
  const figma = {
    pluginId: 'figma-plugin-1',
    clientStorage: { async getAsync() {}, async setAsync() {} },
    root: { getPluginData() { return stored; }, setPluginData(_key, value) { stored = value; } },
  };
  await assert.rejects(() => bindFigmaDocument(figma), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'PROTOCOL_INCOMPATIBLE');
  assert.equal(JSON.parse(stored).version, 2);
});

class EndpointPoolRuntime {
  constructor({ activePorts, trustedPort = null, wrongMac = false }) {
    this.activePorts = new Set(activePorts);
    this.trustedPort = trustedPort;
    this.wrongMac = wrongMac;
    this.sessionPorts = [];
    this.pairRequestPorts = [];
  }

  async request(request) {
    const url = new URL(request.url);
    const port = Number(url.port);
    const path = url.pathname;
    if (!this.activePorts.has(port)) throw new Error('ECONNREFUSED');
    if (path === '/v1/health') return ok({
      service: 'vontaqfs', runtimeVersion: '0.1.0', protocol: { min: 1, max: 1 },
      storageFormatVersion: 1, status: 'ready', capabilities: ['files', 'kv', 'spaces'],
    });
    const body = request.body ? JSON.parse(request.body) : {};
    if (path === '/v1/identity/challenge') {
      if (port !== this.trustedPort) return error(404, 'PAIRING_RUNTIME_NOT_FOUND', 'not paired here');
      const key = createHash('sha256').update(CREDENTIAL).digest();
      const mac = this.wrongMac
        ? '0'.repeat(64)
        : createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex');
      return ok({ mac });
    }
    if (path === '/v1/sessions') {
      this.sessionPorts.push(port);
      return ok({ token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: ['files', 'kv', 'spaces'] });
    }
    if (path === '/v1/spaces/open') return ok(space(body.key, body.storageClass, body.displayName));
    if (path === '/v1/pairings/request') {
      this.pairRequestPorts.push(port);
      return { status: 202, body: JSON.stringify({ pairingId: 'ambiguous', status: 'pending', expiresAtMs: Date.now() + 10_000 }) };
    }
    throw new Error(`Unhandled endpoint-pool path: ${port} ${path}`);
  }
}

test('saved credential selects only the fallback Runtime that proves pairing-bound identity before session credential reuse', async () => {
  const runtime = new EndpointPoolRuntime({ activePorts: [47833, 47835], trustedPort: 47835 });
  const store = new MemoryStateStore({
    'vontaqfs.client-instance.v1': 'client-existing',
    'vontaqfs.pairing-credential.v1': CREDENTIAL,
  });
  const fs = await VontaqFS.connect({
    application: { kind: 'figma-plugin', externalId: '12345', displayName: 'SDK test plugin' },
    stateStore: store, transport: runtime, pairingPollIntervalMs: 0, requestTimeoutMs: 1000, retryCount: 0,
  });
  assert.deepEqual(runtime.sessionPorts, [47835]);
  assert.equal(runtime.pairRequestPorts.length, 0);
  await fs.close();
});

test('spoofed compatible health cannot receive a saved pairing credential when challenge MAC is wrong', async () => {
  const runtime = new EndpointPoolRuntime({ activePorts: [47834], trustedPort: 47834, wrongMac: true });
  const store = new MemoryStateStore({
    'vontaqfs.client-instance.v1': 'client-existing',
    'vontaqfs.pairing-credential.v1': CREDENTIAL,
  });
  await assert.rejects(() => VontaqFS.connect({
    application: { kind: 'figma-plugin', externalId: '12345', displayName: 'SDK test plugin' },
    stateStore: store, transport: runtime, requestTimeoutMs: 1000, retryCount: 0,
  }), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'RUNTIME_IDENTITY_MISMATCH');
  assert.deepEqual(runtime.sessionPorts, []);
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), CREDENTIAL);
});

test('first pairing fails closed when multiple compatible Runtime candidates are visible', async () => {
  const runtime = new EndpointPoolRuntime({ activePorts: [47833, 47834] });
  const store = new MemoryStateStore();
  await assert.rejects(() => VontaqFS.connect({
    application: { kind: 'figma-plugin', externalId: '12345', displayName: 'SDK test plugin' },
    stateStore: store, transport: runtime, requestTimeoutMs: 1000, retryCount: 0,
  }), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'RUNTIME_IDENTITY_AMBIGUOUS');
  assert.deepEqual(runtime.pairRequestPorts, []);
});

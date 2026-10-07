import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';

import {
  VontaqFS,
  VontaqFSError,
  resetPairingState,
  VONTAQ_FS_READ_MANY_MAX_ITEMS,
} from '../dist/index.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class Store {
  constructor(entries = {}, withDelete = true) {
    this.values = new Map(Object.entries(entries));
    if (!withDelete) this.delete = undefined;
  }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
  async delete(key) { this.values.delete(key); }
}

class PrimitiveRuntime {
  constructor(capabilities = ['files', 'kv', 'spaces', 'bulk-read', 'space-clear']) {
    this.capabilities = capabilities;
    this.calls = [];
    this.pairClientInstanceIds = [];
    this.statCounts = new Map();
    this.files = new Map([
      ['/a.txt', Buffer.from('alpha')],
      ['/data.json', Buffer.from('{"ok":true}')],
    ]);
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    const body = request.body ? JSON.parse(request.body) : {};
    this.calls.push({ path, body });

    if (path === '/v1/health') return ok({
      service: 'vontaqfs', runtimeVersion: '0.1.0', protocol: { min: 1, max: 1 },
      storageFormatVersion: 1, status: 'ready', capabilities: this.capabilities,
    });
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      const mac = createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex');
      return ok({ mac });
    }
    if (path === '/v1/pairings/request') {
      this.pairClientInstanceIds.push(body.clientInstanceId);
      return ok({ pairingId: 'pair-1', status: 'pending', expiresAtMs: Date.now() + 10_000 });
    }
    if (path === '/v1/pairings/poll') return ok({
      pairingId: body.pairingId, status: 'approved', expiresAtMs: Date.now() + 10_000,
      pairingCredential: CREDENTIAL,
    });
    if (path === '/v1/sessions') return ok({
      token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: this.capabilities,
    });
    if (path === '/v1/spaces/open') return ok(space(body.key, body.storageClass ?? 'persistent'));

    if (path === '/v1/fs/stat') {
      this.statCounts.set(body.path, (this.statCounts.get(body.path) ?? 0) + 1);
      const bytes = this.files.get(body.path);
      return ok({ file: bytes ? fileInfo(body.path, bytes) : null });
    }
    if (path === '/v1/fs/read-small') {
      const bytes = this.files.get(body.path);
      if (!bytes) return runtimeError(404, 'NOT_FOUND', 'missing');
      return ok({ dataBase64: bytes.toString('base64'), file: fileInfo(body.path, bytes) });
    }
    if (path === '/v1/fs/read-many') {
      const results = body.paths.map((itemPath) => {
        const bytes = this.files.get(itemPath);
        if (!bytes) return { path: itemPath, ok: false, error: { code: 'NOT_FOUND', message: 'file not found' } };
        return { path: itemPath, ok: true, dataBase64: bytes.toString('base64'), file: fileInfo(itemPath, bytes) };
      });
      const totalBytes = results.reduce((sum, item) => sum + (item.ok ? Buffer.from(item.dataBase64, 'base64').length : 0), 0);
      return ok({
        results,
        completedItems: results.length,
        failedItems: results.filter(item => !item.ok).length,
        totalBytes,
        cancelled: false,
      });
    }
    if (path === '/v1/spaces/clear') return ok({
      spaceId: body.spaceId, deletedFiles: 2, deletedKvEntries: 1, releasedBytes: 123,
    });

    throw new Error(`Unhandled path ${path}`);
  }
}

function ok(body) { return { status: 200, body: JSON.stringify(body) }; }
function runtimeError(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function space(key, storageClass) {
  return {
    id: `space-${key}`, key, storageClass, storageCategory: 'custom', createdAtMs: 1, lastUsedAtMs: 1,
    logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy',
  };
}
function fileInfo(path, bytes) {
  return {
    path, version: 1, etag: createHash('sha256').update(bytes).digest('hex'), size: bytes.length, updatedAtMs: 1,
  };
}
function connectOptions(runtime, stateStore = new Store({
  'vontaqfs.client-instance.v1': 'client-existing',
  'vontaqfs.pairing-credential.v1': CREDENTIAL,
})) {
  return {
    application: { kind: 'figma-plugin', externalId: 'cp2-plugin', displayName: 'CP2 Plugin' },
    stateStore,
    developmentEndpoint: 'http://localhost:47833',
    transport: runtime,
    pairingPollIntervalMs: 0,
    retryCount: 0,
  };
}

test('resetPairingState deletes only pairing credential and preserves client/application state', async () => {
  const store = new Store({
    'vontaqfs.client-instance.v1': 'client-stable',
    'vontaqfs.pairing-credential.v1': CREDENTIAL,
    'application.preference': 'keep-me',
  });
  await resetPairingState(store);
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), null);
  assert.equal(await store.get('vontaqfs.client-instance.v1'), 'client-stable');
  assert.equal(await store.get('application.preference'), 'keep-me');
});

test('resetPairingState supports stores without delete and next connect re-pairs with stable client identity', async () => {
  const store = new Store({
    'vontaqfs.client-instance.v1': 'client-stable',
    'vontaqfs.pairing-credential.v1': CREDENTIAL,
  }, false);
  await resetPairingState(store);
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), '');
  const runtime = new PrimitiveRuntime();
  const fs = await VontaqFS.connect(connectOptions(runtime, store));
  assert.deepEqual(runtime.pairClientInstanceIds, ['client-stable']);
  assert.equal(await store.get('vontaqfs.pairing-credential.v1'), CREDENTIAL);
  await fs.close();
});

test('small readText/readJSON reuse known FileInfo and perform one stat each', async () => {
  const runtime = new PrimitiveRuntime();
  const fs = await VontaqFS.connect(connectOptions(runtime));
  assert.equal(await fs.files.readText('/a.txt'), 'alpha');
  assert.deepEqual(await fs.files.readJSON('/data.json'), { ok: true });
  assert.equal(runtime.statCounts.get('/a.txt'), 1);
  assert.equal(runtime.statCounts.get('/data.json'), 1);
});

test('readMany is one bounded capability-gated request with per-path results', async () => {
  const runtime = new PrimitiveRuntime();
  const fs = await VontaqFS.connect(connectOptions(runtime));
  const report = await fs.files.readMany(['/a.txt', '/missing.txt']);
  assert.equal(report.completedItems, 2);
  assert.equal(report.failedItems, 1);
  assert.equal(new TextDecoder().decode(report.results[0].bytes), 'alpha');
  assert.equal(report.results[1].ok, false);
  assert.equal(report.results[1].error.code, 'NOT_FOUND');
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/read-many').length, 1);
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/read-small').length, 0);
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/stat').length, 0);

  await assert.rejects(
    () => fs.files.readMany(Array.from({ length: VONTAQ_FS_READ_MANY_MAX_ITEMS + 1 }, (_, index) => `/x-${index}`)),
    RangeError,
  );
});

test('readMany and space.clear fail CAPABILITY_UNAVAILABLE against compatible older runtime', async () => {
  const runtime = new PrimitiveRuntime(['files', 'kv', 'spaces']);
  const fs = await VontaqFS.connect(connectOptions(runtime));
  await assert.rejects(
    () => fs.files.readMany(['/a.txt']),
    error => error instanceof VontaqFSError && error.code === 'CAPABILITY_UNAVAILABLE',
  );
  const cache = await fs.openSpace({ key: 'cache', storageClass: 'cache' });
  await assert.rejects(
    () => cache.clear(),
    error => error instanceof VontaqFSError && error.code === 'CAPABILITY_UNAVAILABLE',
  );
  assert.equal(runtime.calls.some(call => call.path === '/v1/fs/read-many'), false);
  assert.equal(runtime.calls.some(call => call.path === '/v1/spaces/clear'), false);
});

test('space.clear is disposable-only and forwards cache/temporary clear to runtime', async () => {
  const runtime = new PrimitiveRuntime();
  const fs = await VontaqFS.connect(connectOptions(runtime));
  await assert.rejects(
    () => fs.defaultSpace.clear(),
    error => error instanceof VontaqFSError && error.code === 'REQUEST_INVALID',
  );
  const cache = await fs.openSpace({ key: 'cache', storageClass: 'cache' });
  const report = await cache.clear();
  assert.deepEqual(report, { spaceId: 'space-cache', deletedFiles: 2, deletedKvEntries: 1, releasedBytes: 123 });
  const clearCalls = runtime.calls.filter(call => call.path === '/v1/spaces/clear');
  assert.equal(clearCalls.length, 1);
  assert.equal(clearCalls[0].body.spaceId, 'space-cache');
  assert.match(clearCalls[0].body.requestId, /^request-[0-9a-f]{32}$/);
});

test('readMany respects an already-aborted signal without issuing the bulk request', async () => {
  const runtime = new PrimitiveRuntime();
  const fs = await VontaqFS.connect(connectOptions(runtime));
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(() => fs.files.readMany(['/a.txt'], { signal: controller.signal }), error => error?.name === 'AbortError');
  assert.equal(runtime.calls.some(call => call.path === '/v1/fs/read-many'), false);
});

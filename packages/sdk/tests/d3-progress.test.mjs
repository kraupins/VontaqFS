import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class Store {
  constructor() { this.map = new Map([['vontaqfs.client-instance.v1', 'client-d3'], ['vontaqfs.pairing-credential.v1', CREDENTIAL]]); }
  async get(key) { return this.map.get(key) ?? null; }
  async set(key, value) { this.map.set(key, value); }
}

class OperationRuntime {
  constructor() {
    this.operations = new Map();
    this.streams = new Map();
    this.calls = [];
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    const body = typeof request.body === 'string' && request.body ? JSON.parse(request.body) : {};
    this.calls.push({ path, method: request.method, body });
    if (path === '/v1/health') return json({ service: 'vontaqfs', runtimeVersion: '0.1.0', protocol: { min: 1, max: 1 }, storageFormatVersion: 1, status: 'ready', capabilities: ['files','kv','spaces','streams','events','formats','operations'] });
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      return json({ mac: createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex') });
    }
    if (path === '/v1/sessions') return json({ token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-d3', capabilities: ['files','spaces','streams','operations'] });
    if (path === '/v1/spaces/open') return json(space(body.key));
    if (path === '/v1/fs/stat') return json({ file: null });

    if (path === '/v1/operations/status') {
      const op = this.operations.get(body.operationId);
      return op ? json(op.snapshot) : error(404, 'OPERATION_NOT_FOUND', 'not found');
    }
    if (path === '/v1/operations/cancel') {
      const op = this.operations.get(body.operationId);
      if (!op) return error(404, 'OPERATION_NOT_FOUND', 'not found');
      op.cancelled = true;
      op.snapshot = { ...op.snapshot, status: 'cancelling', phase: 'cancelling', updatedAtMs: Date.now() };
      return json(op.snapshot);
    }

    if (path === '/v1/fs/write-small') {
      const bytes = Buffer.from(body.dataBase64, 'base64');
      const op = this.register(body.operation, 'write', bytes.length);
      this.updateBytes(op, bytes.length, bytes.length, 'completed');
      return json(file(body.path, bytes.length));
    }

    if (path === '/v1/streams/write/begin') {
      const op = this.register(body.operation, 'write', body.declaredSize ?? undefined);
      const streamId = `stream-${this.streams.size + 1}`;
      this.streams.set(streamId, { op, size: 0, path: body.path });
      return json({ streamId, maxChunkBytes: 512 * 1024 });
    }
    if (path.startsWith('/v1/streams/write/') && request.method === 'PUT') {
      const parts = path.split('/');
      const stream = this.streams.get(parts[4]);
      await delay(30);
      if (stream.op.cancelled) return error(409, 'CONFLICT', 'operation was cancelled');
      stream.size += request.body.byteLength;
      this.updateBytes(stream.op, stream.size, stream.op.snapshot.bytesTotal, 'running');
      return json({ streamId: parts[4], acceptedSeq: Number(parts[5]), nextSeq: Number(parts[5]) + 1, receivedBytes: stream.size });
    }
    if (path.endsWith('/commit') && path.startsWith('/v1/streams/write/')) {
      const stream = this.streams.get(path.split('/')[4]);
      this.updateBytes(stream.op, stream.size, stream.op.snapshot.bytesTotal ?? stream.size, 'completed');
      return json(file(stream.path, stream.size));
    }
    if (path.endsWith('/abort') && path.startsWith('/v1/streams/write/')) {
      const stream = this.streams.get(path.split('/')[4]);
      if (stream) stream.op.snapshot = { ...stream.op.snapshot, status: 'cancelled', phase: 'cancelled', cancellable: false, updatedAtMs: Date.now() };
      return empty();
    }

    if (path === '/v1/fs/copy') {
      const op = this.register(body.operation, 'copy', undefined, 8);
      for (let index = 1; index <= 8; index += 1) {
        await delay(25);
        if (op.cancelled) {
          op.snapshot = { ...op.snapshot, status: 'cancelled', phase: 'cancelled', cancellable: false, updatedAtMs: Date.now() };
          return error(409, 'CONFLICT', 'operation was cancelled');
        }
        op.snapshot = { ...op.snapshot, status: 'running', phase: 'copying', itemsCompleted: index, itemsTotal: 8, completed: index, total: 8, updatedAtMs: Date.now() };
      }
      op.snapshot = { ...op.snapshot, status: 'completed', phase: 'complete', cancellable: false, updatedAtMs: Date.now() };
      return json({ copiedFiles: 8 });
    }

    throw new Error(`Unhandled D3 fake path ${request.method} ${path}`);
  }

  register(request, kind, bytesTotal, itemsTotal) {
    let op = this.operations.get(request.id);
    if (op) return op;
    const now = Date.now();
    op = {
      cancelled: false,
      snapshot: {
        id: request.id, kind, phase: kind === 'write' ? 'writing' : kind === 'copy' ? 'copying' : kind, presentation: request.presentation,
        status: 'running', cancellable: true,
        completed: bytesTotal != null ? 0 : itemsTotal != null ? 0 : undefined,
        total: bytesTotal ?? itemsTotal,
        bytesCompleted: bytesTotal != null ? 0 : undefined, bytesTotal,
        itemsCompleted: itemsTotal != null ? 0 : undefined, itemsTotal,
        startedAtMs: now, updatedAtMs: now,
      },
    };
    this.operations.set(request.id, op);
    return op;
  }

  updateBytes(op, done, total, status) {
    op.snapshot = {
      ...op.snapshot, status, phase: status === 'completed' ? 'complete' : 'writing', cancellable: status !== 'completed',
      completed: done, total, bytesCompleted: done, bytesTotal: total, updatedAtMs: Date.now(),
    };
  }
}

function options(runtime) {
  return {
    application: { kind: 'figma-plugin', externalId: 'd3-plugin', displayName: 'D3 Plugin' },
    stateStore: new Store(), developmentEndpoint: 'http://localhost:47833', transport: runtime,
    pairingPollIntervalMs: 0, requestTimeoutMs: 2000, retryCount: 0,
  };
}
function json(value) { return { status: 200, body: JSON.stringify(value) }; }
function empty() { return { status: 204, body: '' }; }
function error(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
function space(key) { return { id: `space-${key}`, key, storageClass: 'persistent', createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy' }; }
function file(path, size) { return { path, version: 1, etag: '0'.repeat(64), size, updatedAtMs: Date.now() }; }

test('large high-level write emits monotonic Runtime-owned byte progress without caller chunk metadata', async () => {
  const runtime = new OperationRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  const seen = [];
  const bytes = new Uint8Array(2 * 1024 * 1024 + 13);
  await fs.files.writeFile('/large.vui', bytes, { progress: { onProgress: snapshot => seen.push(snapshot) } });
  assert.ok(seen.length >= 2);
  const completed = seen.map(item => item.bytesCompleted).filter(Number.isFinite);
  assert.deepEqual([...completed].sort((a,b) => a-b), completed);
  const final = seen.at(-1);
  assert.equal(final.status, 'completed');
  assert.equal(final.bytesCompleted, bytes.byteLength);
  assert.equal(final.bytesTotal, bytes.byteLength);
  const begin = runtime.calls.find(call => call.path === '/v1/streams/write/begin');
  assert.equal(begin.body.operation.presentation, 'client');
  assert.equal('chunkCount' in begin.body, false);
});

test('presentation defaults are safe: no callback is silent and vontaqfs is always explicit', async () => {
  const runtime = new OperationRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  await fs.files.writeFile('/silent.bin', new Uint8Array([1,2,3]));
  await fs.files.writeFile('/native.bin', new Uint8Array([4,5,6]), { progress: { presentation: 'vontaqfs' } });
  const writes = runtime.calls.filter(call => call.path === '/v1/fs/write-small');
  assert.equal(writes[0].body.operation.presentation, 'silent');
  assert.equal(writes[1].body.operation.presentation, 'vontaqfs');
  assert.equal(runtime.calls.some(call => call.path === '/v1/operations/status'), false);
});

test('AbortSignal requests backend cancellation and does not report cancellation before backend acknowledgement', async () => {
  const runtime = new OperationRuntime();
  const fs = await VontaqFS.connect(options(runtime));
  const controller = new AbortController();
  const states = [];
  const pending = fs.files.copy('/tree', '/tree-copy', {
    signal: controller.signal,
    progress: { onProgress: snapshot => states.push(snapshot.status) },
  });
  setTimeout(() => controller.abort(), 65);
  await assert.rejects(pending, error => error?.name === 'AbortError');
  assert.ok(runtime.calls.some(call => call.path === '/v1/operations/cancel'));
  assert.ok(states.includes('cancelling') || states.includes('cancelled'));
  const firstCancel = states.findIndex(state => state === 'cancelling' || state === 'cancelled');
  if (firstCancel >= 0) assert.equal(states.slice(0, firstCancel).includes('cancelled'), false);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS, VontaqFSError } from '../dist/index.js';
import { IncrementalSha256 } from '../dist/sha256.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class MemoryStateStore {
  constructor() { this.values = new Map([['vontaqfs.client-instance.v1', 'client-existing'], ['vontaqfs.pairing-credential.v1', CREDENTIAL]]); }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
  async delete(key) { this.values.delete(key); }
}

class StreamingRuntime {
  constructor() {
    this.calls = [];
    this.files = new Map();
    this.streams = new Map();
    this.readStreams = new Map();
    this.chunkSharesExpectedBuffer = [];
    this.expectedBuffer = null;
    this.eventPolls = 0;
    this.eventResponses = null;
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    this.calls.push({ method: request.method, path });
    if (path === '/v1/health') return json({ service: 'vontaqfs', runtimeVersion: '0.1.0', protocol: { min: 1, max: 1 }, storageFormatVersion: 1, status: 'ready', capabilities: ['files', 'kv', 'spaces', 'streams', 'events'] });
    const body = typeof request.body === 'string' && request.body ? JSON.parse(request.body) : {};
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      const mac = createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex');
      return json({ mac });
    }
    if (path === '/v1/sessions') return json({ token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: ['files', 'kv', 'spaces', 'streams', 'events'] });
    if (path === '/v1/spaces/open') return json(space(body.key));
    if (path === '/v1/spaces/list') return json({ spaces: [space('default')] });
    if (path === '/v1/fs/stat') return json({ file: this.files.get(body.path)?.info ?? null });
    if (path === '/v1/fs/read-small') {
      const record = this.files.get(body.path);
      if (!record?.bytes) return error(404, 'NOT_FOUND', 'missing');
      return json({ dataBase64: Buffer.from(record.bytes).toString('base64'), file: record.info });
    }
    if (path === '/v1/fs/write-small') {
      const bytes = new Uint8Array(Buffer.from(body.dataBase64, 'base64'));
      const info = fileInfo(body.path, bytes.byteLength, sha(bytes));
      this.files.set(body.path, { bytes, info });
      return json(info);
    }
    if (path === '/v1/streams/write/begin') {
      const id = `write-${this.streams.size + 1}`;
      this.streams.set(id, { path: body.path, size: 0, hash: createHash('sha256'), seq: 0, declaredSize: body.declaredSize });
      return json({ streamId: id, maxChunkBytes: 512 * 1024 });
    }
    if (path.startsWith('/v1/streams/write/') && request.method === 'PUT') {
      const [, , , , streamId, seqText] = path.split('/');
      const stream = this.streams.get(streamId);
      const bytes = request.body;
      assert.ok(bytes instanceof Uint8Array);
      assert.equal(Number(seqText), stream.seq);
      if (this.expectedBuffer) this.chunkSharesExpectedBuffer.push(bytes.buffer === this.expectedBuffer);
      stream.hash.update(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength));
      stream.size += bytes.byteLength;
      stream.seq += 1;
      return json({ streamId, acceptedSeq: Number(seqText), nextSeq: stream.seq, receivedBytes: stream.size });
    }
    if (path.endsWith('/commit') && path.startsWith('/v1/streams/write/')) {
      const streamId = path.split('/')[4];
      const stream = this.streams.get(streamId);
      const digest = stream.hash.digest('hex');
      if (digest !== body.sha256) return error(409, 'STREAM_CHECKSUM_MISMATCH', 'bad hash');
      if (stream.declaredSize != null && stream.declaredSize !== stream.size) return error(409, 'STREAM_SEQUENCE_INVALID', 'size mismatch');
      const info = fileInfo(stream.path, stream.size, digest);
      this.files.set(stream.path, { bytes: null, info });
      return json(info);
    }
    if (path.endsWith('/abort') && path.startsWith('/v1/streams/write/')) return empty();
    if (path === '/v1/streams/read/begin') {
      const record = this.files.get(body.path);
      if (!record) return error(404, 'NOT_FOUND', 'missing');
      const id = `read-${this.readStreams.size + 1}`;
      this.readStreams.set(id, { path: body.path, seq: 0 });
      return json({ streamId: id, file: record.info, chunkSize: 256 * 1024 });
    }
    if (path.startsWith('/v1/streams/read/') && request.method === 'GET') {
      const parts = path.split('/');
      const streamId = parts[4];
      const seq = Number(parts[5]);
      const stream = this.readStreams.get(streamId);
      const record = this.files.get(stream.path);
      if (!record.bytes) throw new Error('fixture has no retained bytes');
      const start = seq * 256 * 1024;
      return { status: 200, body: record.bytes.subarray(start, Math.min(record.bytes.byteLength, start + 256 * 1024)) };
    }
    if (path.endsWith('/close') && path.startsWith('/v1/streams/read/')) return empty();
    if (path === '/v1/fs/delete') return json({ deletedFiles: 10_000 });
    if (path === '/v1/fs/copy') return json({ copiedFiles: 3 });
    if (path === '/v1/fs/move') return json({ movedFiles: 3 });
    if (path === '/v1/events/poll') {
      this.eventPolls += 1;
      if (Array.isArray(this.eventResponses) && this.eventResponses.length > 0) return json(this.eventResponses.shift());
      if (this.eventPolls === 1) return json({ events: [], latestSequence: 10, overflow: false });
      return json({
        events: [{ sequence: 12, eventType: 'file-changed', spaceId: 'space-default', path: '/watched/a.txt', atMs: Date.now() }],
        latestSequence: 12,
        overflow: true,
      });
    }
    if (path === '/v1/kv/get') return json({ entry: null });
    if (path === '/v1/kv/set') return json({ key: body.key, value: body.value, version: 1, etag: 'etag', updatedAtMs: Date.now() });
    throw new Error(`Unhandled path ${request.method} ${path}`);
  }
}

function json(value) { return { status: 200, body: JSON.stringify(value) }; }
function empty() { return { status: 204, body: '' }; }
function error(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function space(key) { return { id: `space-${key}`, key, storageClass: 'persistent', createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy' }; }
function sha(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
function fileInfo(path, size, etag) { return { path, version: 1, etag, size, updatedAtMs: Date.now() }; }
function connect(runtime, extra = {}) {
  return VontaqFS.connect({
    application: { kind: 'figma-plugin', externalId: 'cp6', displayName: 'CP6 fixture' },
    stateStore: new MemoryStateStore(), developmentEndpoint: 'http://localhost:47833', transport: runtime, requestTimeoutMs: 1000, retryCount: 1, ...extra,
  });
}

test('large caller-owned Uint8Array automatically streams using views and incremental SHA-256', async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  const bytes = new Uint8Array(2 * 1024 * 1024 + 17);
  for (let i = 0; i < bytes.length; i += 4096) bytes[i] = i & 0xff;
  runtime.expectedBuffer = bytes.buffer;
  const info = await fs.writeFile('/large.bin', bytes);
  assert.equal(info.size, bytes.byteLength);
  assert.equal(info.etag, sha(bytes));
  assert.equal(runtime.calls.some(call => call.path === '/v1/fs/write-small'), false);
  assert.ok(runtime.calls.filter(call => call.method === 'PUT').length > 1);
  assert.ok(runtime.chunkSharesExpectedBuffer.length > 1);
  assert.ok(runtime.chunkSharesExpectedBuffer.every(Boolean));
});

test('100 MiB write does not create SDK chunk buffers detached from caller-owned storage', { timeout: 30_000 }, async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  const bytes = new Uint8Array(100 * 1024 * 1024);
  bytes[0] = 1; bytes[bytes.length - 1] = 2;
  runtime.expectedBuffer = bytes.buffer;
  const info = await fs.writeFile('/100mb.bin', bytes);
  assert.equal(info.size, bytes.byteLength);
  assert.ok(runtime.chunkSharesExpectedBuffer.length >= 400);
  assert.ok(runtime.chunkSharesExpectedBuffer.every(Boolean));
});

test('materialization ceiling fails before a large high-level read and points to createReader', async () => {
  const runtime = new StreamingRuntime();
  runtime.files.set('/huge.bin', { bytes: null, info: fileInfo('/huge.bin', 2 * 1024 * 1024, 'a'.repeat(64)) });
  const fs = await connect(runtime, { materializationLimitBytes: 1024 * 1024 });
  await assert.rejects(() => fs.readFile('/huge.bin'), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'MATERIALIZATION_LIMIT');
  assert.equal(runtime.calls.some(call => call.path === '/v1/streams/read/begin'), false);
});

test('large JSON automatically selects stream transport without caller chunk metadata', async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  await fs.writeJSON('/analysis.json', { kind: 'analysis', payload: 'x'.repeat(600_000), nested: [1, true, null] });
  assert.ok(runtime.calls.some(call => call.path === '/v1/streams/write/begin'));
  assert.ok(runtime.calls.some(call => call.method === 'PUT'));
});

test('recursive delete/copy/move are one logical client requests', async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  assert.deepEqual(await fs.files.delete('/tree', { recursive: true }), { deletedFiles: 10_000 });
  assert.deepEqual(await fs.files.copy('/a', '/b'), { copiedFiles: 3 });
  assert.deepEqual(await fs.files.move('/b', '/c'), { movedFiles: 3 });
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/delete').length, 1);
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/copy').length, 1);
  assert.equal(runtime.calls.filter(call => call.path === '/v1/fs/move').length, 1);
});

test('watch reports overflow/resync marker before subsequent events', async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  const seen = [];
  let resolveSeen;
  const enough = new Promise(resolve => { resolveSeen = resolve; });
  const unsubscribe = await fs.watch('/watched', event => {
    seen.push(event.eventType);
    if (seen.length >= 2) resolveSeen();
  }, { waitMs: 0 });
  await Promise.race([enough, new Promise((_, reject) => setTimeout(() => reject(new Error('watch timeout')), 1000))]);
  unsubscribe();
  assert.deepEqual(seen.slice(0, 2), ['overflow-resync-required', 'file-changed']);
});

test('stream writer accepts a GiB-scale declared size without materializing it', async () => {
  const runtime = new StreamingRuntime();
  const fs = await connect(runtime);
  const declaredSize = 1024 * 1024 * 1024;
  const writer = await fs.files.createWriter('/one-gib.bin', { declaredSize });
  await writer.abort();
  const stream = [...runtime.streams.values()][0];
  assert.equal(stream.declaredSize, declaredSize);
});


test('incremental SHA-256 matches the standard abc vector across segmented updates', () => {
  const hash = new IncrementalSha256();
  hash.update(new TextEncoder().encode('a'));
  hash.update(new TextEncoder().encode('bc'));
  assert.equal(hash.digestHex(), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
});


test('read stream verifies content against the runtime ETag', async () => {
  const runtime = new StreamingRuntime();
  const bytes = new TextEncoder().encode('expected bytes');
  const corrupted = new TextEncoder().encode('corrupt! bytes');
  runtime.files.set('/corrupt.bin', { bytes: corrupted, info: fileInfo('/corrupt.bin', corrupted.byteLength, sha(bytes)) });
  const fs = await connect(runtime);
  const reader = await fs.files.createReader('/corrupt.bin');
  await assert.rejects(() => reader.read(), errorValue => errorValue instanceof VontaqFSError && errorValue.code === 'STREAM_CHECKSUM_MISMATCH');
  await reader.close();
});


test('watch surfaces runtime sequence reset as resync-required after reconnect', async () => {
  const runtime = new StreamingRuntime();
  runtime.eventResponses = [
    { events: [], latestSequence: 10, overflow: false },
    { events: [], latestSequence: 1, overflow: false },
  ];
  const fs = await connect(runtime);
  let resolveSeen;
  const seen = new Promise(resolve => { resolveSeen = resolve; });
  const unsubscribe = await fs.watch('/watched', event => {
    if (event.eventType === 'overflow-resync-required') resolveSeen(event);
  }, { waitMs: 0 });
  const event = await Promise.race([seen, new Promise((_, reject) => setTimeout(() => reject(new Error('watch reset timeout')), 1000))]);
  unsubscribe();
  assert.equal(event.sequence, 1);
});

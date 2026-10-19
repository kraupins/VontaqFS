import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS, VontaqFSError } from '../dist/index.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class MemoryStateStore {
  constructor() { this.values = new Map([['vontaqfs.client-instance.v1', 'client-existing'], ['vontaqfs.pairing-credential.v1', CREDENTIAL]]); }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
  async delete(key) { this.values.delete(key); }
}

class RetrySafeRuntime {
  constructor() {
    this.calls = [];
    this.stream = null;
    this.dropFirstChunkAck = false;
    this.alwaysDropChunkAck = false;
    this.dropFirstCommitResponse = false;
    this.failStatus = false;
    this.dropNextChunkBeforeAccept = false;
    this.committed = null;
    this.disconnect = false;
    this.failOperationalOnly = false;
  }

  async request(request) {
    const path = new URL(request.url).pathname;
    this.calls.push({ path, method: request.method, body: request.body });
    if (this.disconnect) throw new Error('connection refused');
    if (path === '/v1/health') return json(health());
    if (this.failOperationalOnly && path === '/v1/fs/stat') throw new Error('route reset');
    const body = typeof request.body === 'string' && request.body ? JSON.parse(request.body) : {};
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      const mac = createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex');
      return json({ mac });
    }
    if (path === '/v1/sessions') return json({ token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-1', capabilities: ['files', 'streams', 'stream-status'] });
    if (path === '/v1/spaces/open') return json({ id: 'space-default', key: body.key, storageClass: 'persistent', createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy' });
    if (path === '/v1/fs/stat') return json({ file: null });
    if (path === '/v1/streams/write/capability') return json({ chunkMethod: 'PUT' });
    if (path === '/v1/streams/write/begin') {
      assert.equal(body.chunkMethod, 'PUT');
      this.stream = {
        id: 'stream-1', requestId: body.requestId, method: body.chunkMethod,
        nextSeq: 0, chunks: new Map(), hash: createHash('sha256'), size: 0, path: body.path,
        lastAcceptedChunk: undefined, stateRevision: 1,
      };
      return json({ streamId: this.stream.id, requestId: body.requestId, chunkMethod: body.chunkMethod, maxChunkBytes: 1024 });
    }
    if (request.method === 'GET' && path === '/v1/streams/write/stream-1/status') {
      if (this.failStatus) throw new Error('authoritative status unavailable');
      return json({
        connectionId: 'pairing-1', streamId: this.stream.id, requestId: this.stream.requestId,
        chunkMethod: this.stream.method, phase: this.committed ? 'COMMITTED' : 'OPEN',
        nextSeq: this.stream.nextSeq, confirmedBytes: this.stream.size,
        lastAcceptedChunk: this.stream.lastAcceptedChunk,
        stateRevision: this.stream.stateRevision,
        canContinue: !this.committed, canRetry: !this.committed,
        canRequestCancel: !this.committed, canCheckStatus: true,
        ...(this.committed ? { result: this.committed } : {}),
      });
    }
    if (request.method === 'PUT' && path.startsWith('/v1/streams/write/')) {
      const parts = path.split('/');
      const streamId = parts[4];
      const seq = Number(parts[5]);
      assert.equal(streamId, this.stream.id);
      const bytes = new Uint8Array(request.body.buffer, request.body.byteOffset, request.body.byteLength);
      if (this.dropNextChunkBeforeAccept) {
        this.dropNextChunkBeforeAccept = false;
        throw new Error('connection failed before runtime accepted chunk');
      }
      if (seq === this.stream.nextSeq) {
        const copy = Uint8Array.from(bytes);
        this.stream.chunks.set(seq, copy);
        this.stream.hash.update(copy);
        this.stream.size += copy.byteLength;
        this.stream.lastAcceptedChunk = { seq, length: copy.byteLength, sha256: createHash('sha256').update(copy).digest('hex') };
        this.stream.nextSeq += 1;
        this.stream.stateRevision += 1;
        if (this.alwaysDropChunkAck || this.dropFirstChunkAck) {
          this.dropFirstChunkAck = false;
          throw new Error('ACK lost after runtime accepted chunk');
        }
      } else if (seq === this.stream.nextSeq - 1) {
        assert.deepEqual([...bytes], [...this.stream.chunks.get(seq)], 'duplicate sequence must carry exact same bytes');
        if (this.alwaysDropChunkAck) throw new Error('duplicate ACK also lost');
      } else {
        return runtimeError(409, 'STREAM_SEQUENCE_INVALID', 'bad sequence');
      }
      return json({ streamId, acceptedSeq: seq, nextSeq: this.stream.nextSeq, receivedBytes: this.stream.size });
    }
    if (path === '/v1/streams/write/stream-1/commit') {
      if (this.committed) return json(this.committed);
      const checksum = this.stream.hash.digest('hex');
      assert.equal(body.sha256, checksum);
      this.committed = { path: this.stream.path, version: 1, etag: checksum, size: this.stream.size, updatedAtMs: 1 };
      this.stream.stateRevision += 1;
      if (this.dropFirstCommitResponse) {
        this.dropFirstCommitResponse = false;
        throw new Error('commit response lost after commit');
      }
      return json(this.committed);
    }
    if (path === '/v1/streams/write/stream-1/abort') return { status: 204, body: '' };
    throw new Error(`Unhandled fake path ${request.method} ${path}`);
  }
}

function json(value) { return { status: 200, body: JSON.stringify(value) }; }
function runtimeError(status, code, message) { return { status, body: JSON.stringify({ error: { code, message } }) }; }
function health() { return { service: 'vontaqfs', runtimeVersion: '0.2.3', protocol: { min: 1, max: 1 }, storageFormatVersion: 1, status: 'ready', capabilities: ['files', 'streams', 'stream-status'] }; }
function connect(runtime, retryCount = 1) {
  return VontaqFS.connect({
    application: { kind: 'figma-plugin', externalId: 'stream-retry-test', displayName: 'Stream retry test' },
    stateStore: new MemoryStateStore(),
    developmentEndpoint: 'http://localhost:47833',
    transport: runtime,
    requestTimeoutMs: 500,
    discoveryTimeoutMs: 100,
    retryCount,
  });
}

test('lost chunk ACK is reconciled using server chunk proof without duplicate upload', async () => {
  const runtime = new RetrySafeRuntime();
  runtime.dropFirstChunkAck = true;
  const fs = await connect(runtime, 1);
  const writer = await fs.files.createWriter('/retry.bin');
  await writer.write(Uint8Array.of(1, 2, 3, 4));
  assert.equal(writer.bytesWritten, 4);
  assert.equal(runtime.calls.filter(call => call.method === 'PUT').length, 1);
  assert.ok(runtime.calls.some(call => call.method === 'GET' && call.path.endsWith('/status')));
  assert.equal(runtime.stream.nextSeq, 1);
});

test('lost request before acceptance retries same chunk after authoritative status', async () => {
  const runtime = new RetrySafeRuntime();
  runtime.dropNextChunkBeforeAccept = true;
  const fs = await connect(runtime, 0);
  const writer = await fs.files.createWriter('/resend.bin');
  const bytes = Uint8Array.of(3, 2, 1);
  await writer.write(bytes);
  const puts = runtime.calls.filter(call => call.method === 'PUT');
  assert.equal(puts.length, 2);
  assert.equal(puts[0].path, puts[1].path);
  assert.deepEqual([...puts[0].body], [...puts[1].body]);
  assert.equal(writer.bytesWritten, 3);
  assert.equal(runtime.stream.nextSeq, 1);
});

test('lost ACK with unavailable status becomes UNCERTAIN and cannot be retried blindly', async () => {
  const runtime = new RetrySafeRuntime();
  runtime.dropFirstChunkAck = true;
  runtime.failStatus = true;
  const fs = await connect(runtime, 0);
  const writer = await fs.files.createWriter('/uncertain.bin');
  const bytes = Uint8Array.of(9, 8, 7);
  await assert.rejects(() => writer.write(bytes), error => error instanceof VontaqFSError && error.code === 'STREAM_OUTCOME_UNCERTAIN');
  assert.equal(writer.bytesWritten, 0);
  assert.equal(runtime.stream.nextSeq, 1, 'runtime may have accepted the unacknowledged chunk');
  runtime.failStatus = false;
  assert.equal((await writer.status()).nextSeq, 1, 'server evidence remains queryable');
  await assert.rejects(() => writer.write(bytes), error => error instanceof VontaqFSError && error.code === 'STREAM_EXPIRED');
  assert.equal(runtime.calls.filter(call => call.method === 'PUT').length, 1);
});

test('lost commit response is resolved from authoritative COMMITTED status without second commit', async () => {
  const runtime = new RetrySafeRuntime();
  runtime.dropFirstCommitResponse = true;
  const fs = await connect(runtime, 1);
  const writer = await fs.files.createWriter('/commit.bin');
  await writer.write(Uint8Array.of(5, 6));
  const file = await writer.commit();
  assert.equal(file.path, '/commit.bin');
  assert.equal(runtime.calls.filter(call => call.path.endsWith('/commit')).length, 1);
  assert.ok(runtime.calls.some(call => call.method === 'GET' && call.path.endsWith('/status')));
  assert.deepEqual(await writer.commit(), file);
  assert.equal(runtime.calls.filter(call => call.path.endsWith('/commit')).length, 1);
});

test('connected operation failure stays TRANSPORT_ERROR when health still proves runtime is reachable', async () => {
  const runtime = new RetrySafeRuntime();
  const fs = await connect(runtime, 0);
  runtime.failOperationalOnly = true;
  await assert.rejects(() => fs.files.stat('/x'), error => error instanceof VontaqFSError && error.code === 'TRANSPORT_ERROR');
  assert.ok(runtime.calls.filter(call => call.path === '/v1/health').length >= 3, 'post-connect health confirmation should run');
});

test('previously connected client reports RUNTIME_DISCONNECTED only after health confirmation also fails', async () => {
  const runtime = new RetrySafeRuntime();
  const fs = await connect(runtime, 0);
  runtime.disconnect = true;
  await assert.rejects(() => fs.files.stat('/x'), error => error instanceof VontaqFSError && error.code === 'RUNTIME_DISCONNECTED');
});

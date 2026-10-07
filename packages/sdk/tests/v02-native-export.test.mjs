import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS, VontaqFSError } from '../dist/index.js';

const CREDENTIAL = 'e'.repeat(64);
const TOKEN = 'f'.repeat(64);
const DESTINATION_ID = `dst_${'1'.repeat(32)}`;

class Store {
  constructor() {
    this.values = new Map([
      ['vontaqfs.client-instance.v1', 'client-cp3'],
      ['vontaqfs.pairing-credential.v1', CREDENTIAL],
    ]);
  }
  async get(key) { return this.values.get(key) ?? null; }
  async set(key, value) { this.values.set(key, value); }
}

class CP3Runtime {
  constructor({ capabilities, cancelDestination = false, loseOperation = false } = {}) {
    this.capabilities = capabilities ?? [
      'files', 'spaces', 'operations', 'native-export', 'saved-directories',
      'destination-picker-hints', 'internal-export-bookkeeping', 'tracked-export-prune', 'directory-contents-export',
    ];
    this.cancelDestination = cancelDestination;
    this.loseOperation = loseOperation;
    this.calls = [];
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
      return ok({ mac: createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex') });
    }
    if (path === '/v1/sessions') return ok({
      token: TOKEN, expiresAtMs: Date.now() + 60_000, applicationId: 'app-cp3', capabilities: this.capabilities,
    });
    if (path === '/v1/spaces/open') return ok({
      id: 'space-default', key: body.key, storageClass: 'persistent', storageCategory: 'custom',
      createdAtMs: 1, lastUsedAtMs: 1, logicalBytes: 0, fileCount: 0, formatVersion: 1, state: 'healthy',
    });
    if (path === '/v1/destinations/create') {
      if (this.cancelDestination) return runtimeError(409, 'USER_CANCELLED', 'directory selection was cancelled');
      return ok({ id: DESTINATION_ID, label: body.label ?? 'Exports', capability: body.capability, status: 'available', createdAtMs: 1 });
    }
    if (path === '/v1/exports/start') {
      const now = Date.now();
      return response(202, {
        id: body.operation.id, kind: 'native-export', phase: 'exporting', presentation: body.operation.presentation,
        status: 'running', cancellable: true, itemsCompleted: 0, itemsTotal: 1,
        bytesCompleted: 0, bytesTotal: 4, startedAtMs: now, updatedAtMs: now,
      });
    }
    if (path === '/v1/operations/status') {
      if (this.loseOperation) return runtimeError(404, 'OPERATION_NOT_FOUND', 'missing operation');
      return ok({
        id: body.operationId, kind: 'native-export', phase: 'complete', presentation: 'silent', status: 'completed', cancellable: false,
        itemsCompleted: 1, itemsTotal: 1, bytesCompleted: 4, bytesTotal: 4, startedAtMs: 1, updatedAtMs: 2,
        result: { spaceId: 'space-default', destinationLabel: 'Exports', guarantee: 'file-atomic', added: 1, changed: 0, skipped: 0, unchanged: 0, conflicts: 0, deleted: 0, exportedBytes: 4, manifestWritten: true },
      });
    }
    if (path === '/v1/operations/cancel') return ok({ id: body.operationId, status: 'cancelling' });
    throw new Error(`Unhandled CP3 path ${path}`);
  }
}

function ok(body) { return response(200, body); }
function response(status, body) { return { status, body: JSON.stringify(body) }; }
function runtimeError(status, code, message) { return response(status, { error: { code, message } }); }
function options(runtime) {
  return {
    application: { kind: 'figma-plugin', externalId: 'cp3-plugin', displayName: 'CP3 Plugin' },
    stateStore: new Store(), developmentEndpoint: 'http://localhost:47833', transport: runtime,
    retryCount: 0, pairingPollIntervalMs: 0,
  };
}

test('destination creation forwards opaque initial hint and explicit reuse without exposing paths', async () => {
  const runtime = new CP3Runtime();
  const fs = await VontaqFS.connect(options(runtime));
  const destination = await fs.destinations.create({
    label: 'Exports', capability: 'read-write', initialDestinationId: DESTINATION_ID, reuseInitialIfSame: true,
  });
  assert.equal(destination.id, DESTINATION_ID);
  const call = runtime.calls.find(item => item.path === '/v1/destinations/create');
  assert.equal(call.body.initialDestinationId, DESTINATION_ID);
  assert.equal(call.body.reuseInitialIfSame, true);
  assert.equal('path' in call.body, false);
  assert.equal('physicalPath' in call.body, false);
});

test('destination hint is capability-gated and picker cancellation remains USER_CANCELLED', async () => {
  const oldRuntime = new CP3Runtime({ capabilities: ['files', 'spaces', 'operations', 'native-export', 'saved-directories'] });
  const oldFs = await VontaqFS.connect(options(oldRuntime));
  assert.throws(
    () => oldFs.destinations.create({ initialDestinationId: DESTINATION_ID }),
    error => error instanceof VontaqFSError && error.code === 'CAPABILITY_UNAVAILABLE',
  );
  assert.equal(oldRuntime.calls.some(item => item.path === '/v1/destinations/create'), false);

  const cancelledRuntime = new CP3Runtime({ cancelDestination: true });
  const cancelledFs = await VontaqFS.connect(options(cancelledRuntime));
  await assert.rejects(
    () => cancelledFs.destinations.create(),
    error => error instanceof VontaqFSError && error.code === 'USER_CANCELLED',
  );
});

test('native export forwards internal bookkeeping, tracked prune, stable tracking key and contents layout', async () => {
  const runtime = new CP3Runtime();
  const fs = await VontaqFS.connect(options(runtime));
  const report = await fs.defaultSpace.export('/runs/run-2', {
    mode: 'directory', destinationId: DESTINATION_ID, conflict: 'update-changed',
    bookkeeping: 'internal', prune: 'tracked', trackingKey: 'bridge-document-export', directoryLayout: 'contents',
  });
  assert.equal(report.guarantee, 'file-atomic');
  const call = runtime.calls.find(item => item.path === '/v1/exports/start');
  assert.equal(call.body.bookkeeping, 'internal');
  assert.equal(call.body.prune, 'tracked');
  assert.equal(call.body.trackingKey, 'bridge-document-export');
  assert.equal(call.body.directoryLayout, 'contents');
  assert.deepEqual(call.body.sourcePaths, ['/runs/run-2']);
});

test('new native export modes fail before request when an older runtime lacks their capabilities', async () => {
  const runtime = new CP3Runtime({ capabilities: ['files', 'spaces', 'operations', 'native-export', 'saved-directories'] });
  const fs = await VontaqFS.connect(options(runtime));
  await assert.rejects(
    () => fs.defaultSpace.export('/run', { mode: 'directory', bookkeeping: 'internal' }),
    error => error instanceof VontaqFSError && error.code === 'CAPABILITY_UNAVAILABLE',
  );
  assert.equal(runtime.calls.some(item => item.path === '/v1/exports/start'), false);
});

test('tracked export maps runtime operation disappearance to OPERATION_LOST', async () => {
  const runtime = new CP3Runtime({ loseOperation: true });
  const fs = await VontaqFS.connect(options(runtime));
  await assert.rejects(
    () => fs.defaultSpace.export('/a.vui', { mode: 'file', destinationId: DESTINATION_ID }),
    error => error instanceof VontaqFSError && error.code === 'OPERATION_LOST' && typeof error.details?.operationId === 'string',
  );
});

test('contents layout and tracking key validation reject invalid requests locally', async () => {
  const runtime = new CP3Runtime();
  const fs = await VontaqFS.connect(options(runtime));
  await assert.rejects(() => fs.defaultSpace.export(['/a', '/b'], { mode: 'directory', directoryLayout: 'contents' }), TypeError);
  await assert.rejects(() => fs.defaultSpace.export('/a', { mode: 'directory', trackingKey: 'bad\nkey' }), TypeError);
  assert.equal(runtime.calls.some(item => item.path === '/v1/exports/start'), false);
});

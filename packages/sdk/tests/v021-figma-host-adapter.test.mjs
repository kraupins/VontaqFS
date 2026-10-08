import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, cp } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import {
  bindFigmaDocument,
  createFigmaConnectOptions,
  createFigmaMainHostAdapter,
  handleVontaqFSFigmaUiMessage,
} from '../dist/figma.js';
import { VontaqFSError } from '../dist/index.js';

function replaceGlobal(name, value) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  return () => {
    if (descriptor) Object.defineProperty(globalThis, name, descriptor);
    else delete globalThis[name];
  };
}

function figmaStub() {
  const storage = new Map();
  let pluginData = '';
  return {
    pluginId: 'figma-host-test',
    clientStorage: {
      async getAsync(key) { return storage.get(key); },
      async setAsync(key, value) { storage.set(key, value); },
      async deleteAsync(key) { storage.delete(key); },
    },
    root: {
      name: 'Host Test',
      getPluginData() { return pluginData; },
      setPluginData(_key, value) { pluginData = value; },
    },
  };
}

function respondEntropy(host, request, byte = 7) {
  host.handleUiMessage({
    namespace: request.namespace,
    type: 'entropy-response',
    requestId: request.requestId,
    bytesBase64: Buffer.alloc(request.byteLength, byte).toString('base64'),
  });
}

test('Figma main adapter is composable and works without browser-only globals in main', async () => {
  const posted = [];
  let host;
  const restores = [
    replaceGlobal('crypto', undefined),
    replaceGlobal('fetch', undefined),
    replaceGlobal('TextEncoder', undefined),
    replaceGlobal('TextDecoder', undefined),
    replaceGlobal('AbortController', undefined),
  ];
  try {
    host = createFigmaMainHostAdapter({ postMessage: message => posted.push(message) });
    assert.equal(host.handleUiMessage({ type: 'application-message' }), false);

    const ready = host.ready();
    assert.equal(posted[0].type, 'entropy-request');
    respondEntropy(host, posted[0]);
    await ready;

    const random = new Uint8Array(16);
    assert.equal(host.secureRandom.getRandomValues(random), random);
    assert.deepEqual([...random], Array(16).fill(7));

    const figma = figmaStub();
    const connectOptions = createFigmaConnectOptions(figma, { displayName: 'Test', host });
    assert.equal(connectOptions.transport, host.transport);
    assert.equal(connectOptions.secureRandom, host.secureRandom);

    const binding = await bindFigmaDocument(figma, { secureRandom: host.secureRandom });
    assert.match(binding.id, /^doc-[0-9a-f]{32}$/);

    const requestPromise = host.transport.request({
      method: 'GET',
      url: 'http://localhost:47833/v1/health',
      headers: {},
      responseType: 'binary',
      timeoutMs: 1000,
      purpose: 'operational',
      timeoutMode: 'none',
    });
    const request = posted.find(message => message.type === 'http-request');
    assert.ok(request);
    host.handleUiMessage({
      namespace: request.namespace,
      type: 'http-response',
      requestId: request.requestId,
      response: { status: 200, body: { kind: 'binary', base64: Buffer.from([1, 2, 3]).toString('base64') } },
    });
    const response = await requestPromise;
    assert.deepEqual([...response.body], [1, 2, 3]);
  } finally {
    host?.close();
    for (const restore of restores.reverse()) restore();
  }
});

test('Figma UI helper owns Web Crypto/fetch and materializes binary request bodies as ArrayBuffer', async () => {
  const mainPosted = [];
  const host = createFigmaMainHostAdapter({ postMessage: message => mainPosted.push(message) });
  const ready = host.ready();
  const entropyRequest = mainPosted.shift();
  const uiReplies = [];
  const entropyHandled = await handleVontaqFSFigmaUiMessage(entropyRequest, { postMessage: reply => uiReplies.push(reply) });
  assert.equal(entropyHandled, true);
  assert.equal(uiReplies[0].type, 'entropy-response');
  host.handleUiMessage(uiReplies.shift());
  await ready;

  let observedBody;
  const restoreFetch = replaceGlobal('fetch', async (_url, init) => {
    observedBody = init.body;
    return {
      status: 200,
      headers: new Headers({ 'x-test': 'ok' }),
      async arrayBuffer() { return Uint8Array.of(9, 8, 7).buffer; },
      async text() { return 'ok'; },
    };
  });
  const restoreAbort = replaceGlobal('AbortController', undefined);
  try {
    const pending = host.transport.request({
      method: 'PUT',
      url: 'http://localhost:47834/v1/stream/write',
      headers: { 'content-type': 'application/octet-stream' },
      body: Uint8Array.of(4, 5, 6),
      responseType: 'binary',
      timeoutMs: 123,
      purpose: 'operational',
      timeoutMode: 'none',
    });
    const httpRequest = mainPosted.shift();
    const replies = [];
    assert.equal(await handleVontaqFSFigmaUiMessage(httpRequest, { postMessage: reply => replies.push(reply) }), true);
    assert.ok(observedBody instanceof ArrayBuffer);
    assert.deepEqual([...new Uint8Array(observedBody)], [4, 5, 6]);
    host.handleUiMessage(replies[0]);
    const response = await pending;
    assert.deepEqual([...response.body], [9, 8, 7]);
  } finally {
    restoreAbort();
    restoreFetch();
    host.close();
  }
});

test('Figma relay refuses non-official URLs and unrelated UI messages remain unclaimed', async () => {
  const host = createFigmaMainHostAdapter({ postMessage() {} });
  assert.throws(
    () => host.transport.request({ method: 'GET', url: 'http://example.com/v1/health', headers: {}, timeoutMs: 100, timeoutMode: 'none' }),
    error => error instanceof VontaqFSError && error.code === 'TRANSPORT_ERROR',
  );
  assert.equal(await handleVontaqFSFigmaUiMessage({ type: 'application-message' }, { postMessage() {} }), false);
  host.close();
});

test('bindFigmaDocument remains fail-closed without native or injected secure random', async () => {
  const restore = replaceGlobal('crypto', undefined);
  try {
    await assert.rejects(
      () => bindFigmaDocument(figmaStub()),
      error => error instanceof VontaqFSError && error.code === 'INTERNAL_ERROR',
    );
  } finally {
    restore();
  }
});

test('published-package Figma fixture bundles main and UI without scanner-dangerous import syntax', async () => {
  const repoRoot = path.resolve(fileURLToPath(new URL('../../../', import.meta.url)));
  const sdkRoot = path.join(repoRoot, 'packages', 'sdk');
  const releaseScript = path.join(repoRoot, 'scripts', 'release', 'prepare-sdk-package.mjs');
  const temp = await mkdtemp(path.join(os.tmpdir(), 'vontaqfs-figma-pack-'));
  const localRequire = createRequire(import.meta.url);
  const runNpm = (args, options = {}) => {
    const npmExecPath = process.env.npm_execpath;
    if (npmExecPath) {
      return spawnSync(process.execPath, [npmExecPath, ...args], options);
    }
    return spawnSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', args, {
      ...options,
      shell: process.platform === 'win32',
    });
  };
  const assertSpawnOk = (result, label) => {
    assert.ifError(result.error);
    assert.equal(
      result.status,
      0,
      `${label} failed${result.signal ? ` (signal ${result.signal})` : ''}\n${result.stderr || result.stdout || ''}`,
    );
  };

  try {
    const prepare = spawnSync(process.execPath, [releaseScript, 'prepare'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    assertSpawnOk(prepare, 'prepare-sdk-package');

    const packResult = runNpm(['pack', '--ignore-scripts', '--json', '--pack-destination', temp], {
      cwd: sdkRoot,
      encoding: 'utf8',
    });
    assertSpawnOk(packResult, 'npm pack');
    const packed = JSON.parse(packResult.stdout);
    const packedEntry = Array.isArray(packed) ? packed.at(-1) : packed;
    assert.ok(packedEntry?.filename, `npm pack did not report a tarball filename: ${packResult.stdout}`);
    const tarball = path.join(temp, packedEntry.filename);

    const installRoot = path.join(temp, 'installed');
    const installResult = runNpm([
      'install',
      '--ignore-scripts',
      '--no-save',
      '--no-package-lock',
      '--no-audit',
      '--no-fund',
      '--prefix', installRoot,
      tarball,
    ], { encoding: 'utf8' });
    assertSpawnOk(installResult, 'npm install packed SDK');
    const packedSdkRoot = path.join(installRoot, 'node_modules', '@vontaq', 'fs');

    const fixture = path.join(temp, 'fixture');
    await cp(path.join(repoRoot, 'examples', 'figma-plugin'), fixture, { recursive: true });
    await rm(path.join(fixture, 'dist'), { recursive: true, force: true });

    // The copied public example declares TypeScript as a devDependency. For this
    // offline package-contract test, materialize the already-installed repository
    // copy instead of reaching the registry or relying on machine-global paths.
    const typescriptPackageJson = localRequire.resolve('typescript/package.json');
    const typescriptRoot = path.dirname(typescriptPackageJson);
    const fixtureTypeScriptRoot = path.join(fixture, 'node_modules', 'typescript');
    await cp(typescriptRoot, fixtureTypeScriptRoot, { recursive: true });

    const built = spawnSync(process.execPath, ['build.mjs'], {
      cwd: fixture,
      env: { ...process.env, VONTAQ_FS_SDK_ROOT: packedSdkRoot },
      encoding: 'utf8',
    });
    assertSpawnOk(built, 'packed-package Figma fixture build');
    const main = await readFile(path.join(fixture, 'dist', 'code.js'), 'utf8');
    const ui = await readFile(path.join(fixture, 'dist', 'ui.html'), 'utf8');
    assert.ok(main.includes('createFigmaMainHostAdapter'));
    assert.ok(ui.includes('handleVontaqFSFigmaUiMessage'));
    assert.equal(main.includes('import('), false);
    assert.equal(ui.includes('import('), false);
  } finally {
    const cleanup = spawnSync(process.execPath, [releaseScript, 'cleanup'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    assertSpawnOk(cleanup, 'prepare-sdk-package cleanup');
    await rm(temp, { recursive: true, force: true });
  }
});

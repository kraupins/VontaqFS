import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const endpoints = JSON.parse(await readFile(new URL('../../protocol/endpoints.json', import.meta.url), 'utf8'));
const protocol = await readFile(new URL('../../crates/protocol/src/lib.rs', import.meta.url), 'utf8');
const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const sdkProtocol = await readFile(new URL('../../packages/sdk/src/protocol.ts', import.meta.url), 'utf8');
const sdk = await readFile(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');
const desktop = await readFile(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const manifest = JSON.parse(await readFile(new URL('../../examples/figma-plugin/manifest.json', import.meta.url), 'utf8'));
const tauriConfig = JSON.parse(await readFile(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));

const ports = [endpoints.primary, ...endpoints.fallback];
const localhostUrls = ports.map((port) => `http://localhost:${port}`);

test('D1 canonical endpoint contract is the fixed deterministic 47833-47836 set', () => {
  assert.deepEqual(endpoints, { version: 1, primary: 47833, fallback: [47834, 47835, 47836] });
  assert.match(protocol, /RUNTIME_ENDPOINT_PORTS: \[u16; 4\] = \[47_833, 47_834, 47_835, 47_836\]/);
  assert.match(sdkProtocol, /VONTAQ_FS_ENDPOINT_PORTS = \[47833, 47834, 47835, 47836\]/);
  assert.deepEqual(manifest.networkAccess.allowedDomains, localhostUrls);
  for (const url of localhostUrls) assert.ok(tauriConfig.app.security.csp['connect-src'].includes(url), `missing CSP endpoint ${url}`);
});

test('D1 runtime persists only official selected endpoints and never chooses a random fallback', () => {
  assert.match(runtime, /endpoint_candidate_order/);
  assert.match(runtime, /load_selected_endpoint/);
  assert.match(runtime, /persist_selected_endpoint/);
  assert.match(runtime, /RUNTIME_ENDPOINT_PORTS\.contains\(&port\)/);
  assert.doesNotMatch(runtime.split('#[cfg(test)]')[0], /probe_existing/);
  assert.doesNotMatch(runtime, /bind\([^\n]*0\.0\.0\.0/);
});

test('D1 all-port conflict keeps Desktop host alive and surfaces endpoint state', () => {
  assert.match(runtime, /selected_port: Option<u16>/);
  assert.match(runtime, /occupied_ports: Vec<u16>/);
  assert.match(desktop, /runtime_endpoint_status/);
  assert.match(desktop, /needs-attention/);
  assert.match(desktop, /desktop-instance\.lock/);
  assert.match(desktop, /activate\.request/);
});

test('D1 trusted reconnect verifies pairing-bound challenge before session credential submission', () => {
  const challenge = sdk.indexOf("'/v1/identity/challenge'");
  const session = sdk.indexOf("'/v1/sessions'");
  assert.ok(challenge >= 0 && session >= 0);
  assert.match(sdk, /discoverTrustedRuntime/);
  assert.match(sdk, /verifyRuntimeIdentity/);
  assert.match(sdk, /hmacSha256Hex/);
  assert.match(sdk, /RUNTIME_IDENTITY_MISMATCH/);
  assert.match(sdk, /PAIRING_RUNTIME_NOT_FOUND/);
  assert.doesNotMatch(sdk, /deleteState\(this\.settings\.stateStore, PAIRING_CREDENTIAL_KEY\)/);
});

test('D1 custom endpoint override is explicitly development-only', () => {
  assert.match(sdk, /developmentEndpoint\?: string/);
  assert.doesNotMatch(sdk, /\n\s*endpoint\?: string;/);
});

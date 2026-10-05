import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const runtimePublic = runtime.split('#[cfg(test)]')[0];
const protocol = await readFile(new URL('../../crates/protocol/src/lib.rs', import.meta.url), 'utf8');
const core = await readFile(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const desktop = await readFile(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');

test('D1 revises CP4 to a deterministic official loopback endpoint pool without HTTP identity ownership', () => {
  assert.match(protocol, /RUNTIME_ENDPOINT_PORTS: \[u16; 4\] = \[47_833, 47_834, 47_835, 47_836\]/);
  assert.match(runtime, /endpoint_candidate_order/);
  assert.match(runtime, /bind_official_endpoint/);
  assert.match(runtime, /Ipv4Addr::LOCALHOST/);
  assert.match(runtime, /Ipv6Addr::LOCALHOST/);
  assert.doesNotMatch(runtime, /0\.0\.0\.0/);
  assert.doesNotMatch(runtimePublic, /probe_existing/);
});

test('CP4 hardens Host, Origin, content type and request limits', () => {
  assert.match(runtime, /RUNTIME_ENDPOINT_PORTS\.iter\(\)\.any/);
  assert.match(runtime, /format!\("localhost:\{port\}"\)/);
  assert.match(protocol, /application\/vnd\.vontaqfs\+json; version=1/);
  assert.match(protocol, /MAX_CONTROL_BODY_BYTES: usize = 1024 \* 1024/);
  assert.match(protocol, /MAX_HEADER_BYTES: usize = 32 \* 1024/);
  assert.match(runtime, /Transfer-Encoding is not supported/);
  assert.match(runtime, /Cache-Control: no-store/);
});

test('CP4 pairing authority is credential hash plus short-lived session, not claimed identity', () => {
  assert.match(core, /credential_hash TEXT NOT NULL/);
  assert.match(runtime, /sha256_hex\(credential\.as_bytes\(\)\)/);
  assert.match(protocol, /SESSION_TTL_MS: i64 = 15 \* 60 \* 1000/);
  assert.match(runtime, /Authorization/);
  assert.match(runtime, /Bearer /);
  assert.match(runtime, /PermissionDenied/);
  assert.match(runtime, /\/v1\/identity\/challenge/);
  assert.match(runtime, /hmac_sha256_hex/);
});

test('CP4 exposes no public HTTP admin pairing routes', () => {
  assert.doesNotMatch(runtimePublic, /\/v1\/admin/);
  assert.doesNotMatch(runtimePublic, /\/v1\/pairings\/approve/);
  assert.doesNotMatch(runtimePublic, /\/v1\/pairings\/revoke/);
  assert.match(desktop, /fn approve_pairing/);
  assert.match(desktop, /fn deny_pairing/);
  assert.match(desktop, /fn revoke_pairing/);
});

test('CP4 desktop shell actually hosts the runtime server', () => {
  assert.match(desktop, /RuntimeServer::bind/);
  assert.match(desktop, /server\.serve\(\)\.await/);
  assert.match(desktop, /app\.manage\(DesktopRuntime/);
});

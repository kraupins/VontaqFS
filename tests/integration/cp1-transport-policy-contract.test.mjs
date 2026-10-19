// CP1 static wiring checks — NOT a replacement for Rust/native auth and migration tests.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const source = (path) => readFileSync(resolve(root, path), 'utf8');

test('pairing method declaration optional legacy PUT and strict variant handling', () => {
  const protocol = source('crates/protocol/src/lib.rs');
  const runtime = source('crates/runtime/src/lib.rs');
  assert.match(protocol, /supported_chunk_methods: Option<Vec<ChunkMethod>>/);
  assert.match(protocol, /serde\(rename = "PUT"\)/);
  assert.match(protocol, /serde\(rename = "POST"\)/);
  assert.match(runtime, /unwrap_or_else\(\|\| vec!\[ChunkMethod::Put\]\)/);
  assert.match(runtime, /existing\.supported_chunk_methods/);
});

test('policy remains keyed on stable persisted pairing identity and migrated PUT-only', () => {
  const storage = source('crates/core/src/storage.rs');
  assert.match(storage, /CREATE TABLE IF NOT EXISTS pairing_transport_policies/);
  assert.match(storage, /pairing_id TEXT PRIMARY KEY REFERENCES pairings\(id\)/);
  assert.match(storage, /INSERT OR IGNORE INTO pairing_transport_policies/);
  assert.match(storage, /SELECT id,'\[\\"PUT\\"\]','PUT',1,created_at_ms FROM pairings/);
  assert.match(storage, /set_pairing_transport_method/);
  assert.match(storage, /UPDATE pairing_transport_policies SET selected_method=\?1,revision=\?2/);
});

test('runtime retains per-pairing policy and enables POST only for pinned authorized streams', () => {
  const runtime = source('crates/runtime/src/lib.rs');
  assert.match(runtime, /const ENABLE_POST_CHUNKS: bool = true/);
  assert.match(runtime, /transport\.selected_method != input\.chunk_method\.as_str\(\)/);
  assert.match(runtime, /stream\.chunk_method != wire_method/);
  assert.match(runtime, /RuntimeAdminError::Internal\("chunk method is not authorized/);
  assert.doesNotMatch(runtime, /\("POST", "\/v1\/streams\/write\/\{streamId\}\/\{seq\}\/chunk"\)/);
});

test('only trusted Desktop invokes transport preference mutation', () => {
  const tauri = source('src-tauri/src/lib.rs');
  const runtime = source('crates/runtime/src/lib.rs');
  assert.match(tauri, /fn set_connection_transport_preference\(/);
  assert.match(tauri, /set_connection_transport_preference,\s+approve_pairing/);
  assert.match(runtime, /self\.state\.storage\.set_pairing_transport_method/);
  assert.doesNotMatch(runtime, /\("POST", "\/v1\/pairings\/set-transport"\)/);
});

test('SDK preserves existing pairing API and declares methods only during authorization', () => {
  const sdk = source('packages/sdk/src/index.ts');
  const protocol = source('packages/sdk/src/protocol.ts');
  assert.match(sdk, /supportedChunkMethods\?: readonly ChunkMethod\[\]/);
  assert.match(sdk, /supportedChunkMethods: settings\.supportedChunkMethods/);
  assert.match(sdk, /options\.supportedChunkMethods \?\? \['PUT'\]/);
  assert.match(protocol, /effectiveChunkMethod\?: ChunkMethod/);
});

test('legacy DesktopPreferences and portable storage format unchanged', () => {
  const runtime = source('crates/runtime/src/lib.rs');
  const protocol = source('crates/protocol/src/lib.rs');
  assert.match(runtime, /pub struct DesktopPreferences \{\s+pub automatic_update_check: bool,\s+pub launch_on_login: bool/);
  assert.match(protocol, /pub const STORAGE_FORMAT_VERSION: u32 = 1/);
});

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const protocol = await readFile(new URL('../../crates/protocol/src/lib.rs', import.meta.url), 'utf8');
const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const core = await readFile(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const sdk = await readFile(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');

const productionRuntime = runtime.split('#[cfg(test)]')[0];

test('CP6 protocol centralizes v1 stream, materialization and event safety limits', () => {
  assert.match(protocol, /MAX_ACTIVE_SESSIONS: usize = 32/);
  assert.match(protocol, /MAX_STREAM_CHUNK_BYTES: usize = 512 \* 1024/);
  assert.match(protocol, /DEFAULT_STREAM_CHUNK_BYTES: usize = 256 \* 1024/);
  assert.match(protocol, /MATERIALIZATION_LIMIT_BYTES: u64 = 64 \* 1024 \* 1024/);
  assert.match(protocol, /MAX_MANAGED_FILE_BYTES: u64 = 16 \* 1024 \* 1024 \* 1024/);
  assert.match(protocol, /STREAM_IDLE_TIMEOUT_MS: i64 = 60 \* 1000/);
  assert.match(protocol, /EVENT_LONG_POLL_MAX_MS: u64 = 25 \* 1000/);
  assert.match(protocol, /EVENT_BUFFER_CAPACITY: usize = 2048/);
});

test('CP6 runtime exposes raw-binary write/read streams with sequence, checksum and cleanup gates', () => {
  for (const endpoint of [
    '/v1/streams/write/begin', '/v1/streams/read/begin', '/v1/events/poll',
    '/v1/fs/delete', '/v1/fs/copy', '/v1/fs/move',
  ]) assert.ok(productionRuntime.includes(endpoint), `missing ${endpoint}`);
  assert.match(productionRuntime, /application\/octet-stream/);
  assert.match(productionRuntime, /StreamSequenceInvalid/);
  assert.match(productionRuntime, /StreamChecksumMismatch/);
  assert.match(productionRuntime, /STREAM_IDLE_TIMEOUT_MS/);
  assert.match(productionRuntime, /abort_stream_temp/);
  assert.match(productionRuntime, /requestId is already bound to a different stream mutation/);
});

test('CP6 streaming commits through core atomic replacement instead of exposing temp files', () => {
  assert.match(core, /pending_mutations/);
  assert.match(core, /atomic_replace\(temp, &target\)/);
  assert.match(core, /finalize_file_write/);
  assert.match(core, /stream temp file escaped runtime temp root/);
  assert.match(core, /ensure_no_symlink_escape/);
  assert.match(core, /quarantine_orphan_stream_temps/);
});

test('CP6 bulk operations are runtime-side and bounded globally plus per pairing', () => {
  assert.match(protocol, /MAX_ACTIVE_BULK_REQUESTS: usize = 8/);
  assert.match(protocol, /MAX_BULK_REQUESTS_PER_SESSION: usize = 2/);
  assert.match(productionRuntime, /acquire_bulk\(&session\.pairing_id\)/);
  assert.match(core, /pub fn delete_path/);
  assert.match(core, /pub fn copy_path/);
  assert.match(core, /pub fn move_path/);
});

test('CP6 event long-poll has bounded buffering and explicit overflow/resync signaling', () => {
  assert.match(productionRuntime, /VecDeque<BufferedEvent>/);
  assert.match(productionRuntime, /EVENT_BUFFER_CAPACITY/);
  assert.match(productionRuntime, /overflow/);
  assert.match(protocol, /OverflowResyncRequired/);
  assert.match(sdk, /overflow-resync-required/);
});

test('CP6 SDK keeps normal APIs chunk-free while selecting streams and enforcing materialization limit', () => {
  assert.match(sdk, /writeFile\(path: string, bytes: Uint8Array/);
  assert.match(sdk, /createWriter/);
  assert.match(sdk, /createReader/);
  assert.match(sdk, /MATERIALIZATION_LIMIT/);
  assert.match(sdk, /subarray/);
  assert.match(sdk, /IncrementalSha256/);
  assert.doesNotMatch(sdk, /Buffer\.from/);
});

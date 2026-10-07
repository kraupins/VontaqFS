import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const core = fs.readFileSync(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const runtime = fs.readFileSync(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const protocol = fs.readFileSync(new URL('../../crates/protocol/src/lib.rs', import.meta.url), 'utf8');
const tauri = fs.readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const sdk = fs.readFileSync(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');

function section(source, start, end) {
  const a = source.indexOf(start);
  assert.notEqual(a, -1, `missing ${start}`);
  const b = end ? source.indexOf(end, a) : source.length;
  assert.notEqual(b, -1, `missing ${end}`);
  return source.slice(a, b);
}

test('0.2 destination hints stay opaque and desktop picker receives only a runtime-private initial directory', () => {
  const create = section(runtime, '("POST", "/v1/destinations/create")', '("POST", "/v1/destinations/list")');
  assert.match(create, /initial_destination_id/);
  assert.match(create, /reuse_initial_if_same/);
  assert.match(create, /UserCancelled/);
  assert.match(create, /grant_capability_satisfies/);
  assert.match(tauri, /set_directory\(initial_directory\)/);
  const sdkDestination = section(sdk, 'class ApplicationDestinations', 'class SpaceSnapshots');
  assert.match(sdkDestination, /initialDestinationId/);
  assert.match(sdkDestination, /destinationPickerHints/);
  assert.doesNotMatch(sdkDestination, /physicalPath/);
});

test('0.2 internal export tracking is destination-clean and stable across transient source run paths', () => {
  const nativeExport = section(core, 'pub fn native_export', 'pub fn resolve_directory_grant_import_sources');
  assert.match(nativeExport, /ExportBookkeepingPolicy::Internal/);
  assert.match(nativeExport, /runtime\/native-export/);
  assert.match(nativeExport, /native_export_tracking_scope\(application_id, &destination_identity, tracking_key\)/);
  assert.match(nativeExport, /manifest_checksums_by_destination/);
  assert.match(nativeExport, /export_relative_destination/);
  assert.match(nativeExport, /DirectoryExportLayout::Contents/);
  assert.match(nativeExport, /current_export_paths/);
});

test('0.2 tracked prune deletes only previously tracked unchanged destination bytes', () => {
  const nativeExport = section(core, 'pub fn native_export', 'pub fn resolve_directory_grant_import_sources');
  assert.match(nativeExport, /ExportPrunePolicy::Tracked/);
  assert.match(nativeExport, /previous\.files/);
  assert.match(nativeExport, /sha256_file\(&target\)/);
  assert.match(nativeExport, /current_hash == previous_entry\.checksum/);
  assert.match(nativeExport, /fs::remove_file\(&target\)/);
});

test('0.2 native export verifies selected source hash before atomic target replacement', () => {
  const copy = section(core, 'fn copy_file_atomic_stage', '#[derive(Debug)]\nstruct PendingMutation');
  assert.match(copy, /expected_sha256/);
  assert.match(copy, /Sha256::new\(\)/);
  assert.match(copy, /ExportSourceChanged/);
  const mismatch = copy.indexOf('observed != expected_sha256');
  const replace = copy.indexOf('atomic_replace(temp, target)');
  assert.ok(mismatch >= 0 && replace > mismatch, 'hash verification must occur before atomic replacement');
  const zip = section(core, 'fn add_file_verified_cancellable', 'fn finish(mut self)');
  assert.match(zip, /expected_sha256/);
  assert.match(zip, /ExportSourceChanged/);
});

test('0.2 destination exports serialize by canonical destination and operation loss has a stable SDK error', () => {
  assert.match(runtime, /active_destinations: Mutex<HashMap<String, String>>/);
  assert.match(runtime, /ErrorCode::DestinationBusy/);
  const exportRoute = section(runtime, '("POST", "/v1/exports/start")', '("POST", "/v1/imports/start")');
  assert.match(exportRoute, /canonicalize\(&destination_path\)/);
  assert.match(exportRoute, /lock_destination_api/);
  assert.match(exportRoute, /unlock_destination/);
  const observer = section(sdk, 'class OperationObserver', 'class StreamWriter');
  assert.match(observer, /OPERATION_NOT_FOUND/);
  assert.match(observer, /OPERATION_LOST/);
});

test('0.2 health/runtime capability discovery advertises the native export feature gates', () => {
  for (const capability of ['destination-picker-hints', 'internal-export-bookkeeping', 'tracked-export-prune', 'directory-contents-export']) {
    assert.match(protocol, new RegExp(capability));
    assert.match(runtime, new RegExp(capability));
  }
});

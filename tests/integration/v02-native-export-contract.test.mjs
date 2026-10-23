import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const readSource = url => fs.readFileSync(url, 'utf8').replace(/\r\n/g, '\n');

const core = readSource(new URL('../../crates/core/src/storage.rs', import.meta.url));
const nativeUnix = readSource(new URL('../../crates/core/src/native_dir.rs', import.meta.url));
const nativeWindows = readSource(new URL('../../crates/core/src/native_dir_windows.rs', import.meta.url));
const runtime = readSource(new URL('../../crates/runtime/src/lib.rs', import.meta.url));
const protocol = readSource(new URL('../../crates/protocol/src/lib.rs', import.meta.url));
const tauri = readSource(new URL('../../src-tauri/src/lib.rs', import.meta.url));
const sdk = readSource(new URL('../../packages/sdk/src/index.ts', import.meta.url));

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
  assert.match(create, /admit_directory_grant_request\(/);
  assert.match(create, /requested_capability/);
  assert.match(create, /\.filter\(\|grant\| grant\.revoked_at_ms\.is_none\(\)\)/);
  assert.match(create, /finish_directory_grant_request\(/);
  const issuance = section(core, 'pub fn finish_directory_grant_request', 'pub fn create_directory_grant');
  // Only reuse a matching, live grant with sufficient capabilities and the same bound native root.
  assert.match(issuance, /revoked_at_ms IS NULL/);
  assert.match(issuance, /existing\.capability\.can_read\(\) >= capability\.can_read\(\)/);
  assert.match(issuance, /existing\.capability\.can_write\(\) >= capability\.can_write\(\)/);
  assert.match(issuance, /bound_identity/);
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
  assert.match(nativeExport, /native_export_tracking_scope\(\s*application_id,\s*&destination_identity,\s*tracking_key,?\s*\)/);
  assert.match(nativeExport, /manifest_checksums_by_destination/);
  assert.match(nativeExport, /export_relative_destination/);
  assert.match(nativeExport, /DirectoryExportLayout::Contents/);
  assert.match(nativeExport, /current_export_paths/);
});

test('0.2 tracked prune deletes only previously tracked unchanged destination bytes', () => {
  const nativeExport = section(core, 'pub fn native_export', 'pub fn resolve_directory_grant_import_sources');
  assert.match(nativeExport, /ExportPrunePolicy::Tracked/);
  assert.match(nativeExport, /previous\.files/);
  // Prune uses a held native directory handle and verifies the quarantined inode,
  // not a time-of-check pathname hash followed by fs::remove_file(target).
  assert.match(nativeExport, /native_root\.prune_tracked\(&relative, &previous_entry\.checksum\)/);
  assert.doesNotMatch(nativeExport, /fs::remove_file\(&target\)/);
  for (const native of [nativeUnix, nativeWindows]) {
    assert.match(native, /fn prune_tracked\(/);
    assert.match(native, /expected_sha256/);
    assert.match(native, /quarantine/);
  }
});

test('0.2 native export verifies selected source hash before atomic target replacement', () => {
  const copy = section(core, 'fn copy_file_atomic_stage', '#[derive(Debug)]\nstruct PendingMutation');
  assert.match(copy, /expected_sha256/);
  assert.match(copy, /Sha256::new\(\)/);
  assert.match(copy, /ExportSourceChanged/);
  const mismatch = copy.indexOf('observed != expected_sha256');
  const replace = copy.indexOf('atomic_replace(&temp, target)');
  assert.ok(mismatch >= 0 && replace > mismatch, 'hash verification must occur before atomic replacement');
  const zip = section(core, 'fn add_file_verified_cancellable', 'fn finish(mut self)');
  assert.match(zip, /expected_sha256/);
  assert.match(zip, /ExportSourceChanged/);
});

test('0.2 destination exports serialize by selected destination and enforce native root identity; operation loss is typed', () => {
  assert.match(runtime, /active_destinations: Mutex<HashMap<String, String>>/);
  assert.match(runtime, /ErrorCode::DestinationBusy/);
  const exportRoute = section(runtime, '("POST", "/v1/exports/start")', '("POST", "/v1/imports/start")');
  // Do not follow a swapped symlink during last-minute path canonicalization:
  // the native handle opens and verifies the original directory instead.
  assert.match(exportRoute, /destination_path\.is_absolute\(\)/);
  assert.match(exportRoute, /destination_path\.to_string_lossy\(\)\.to_string\(\)/);
  assert.doesNotMatch(exportRoute, /canonicalize\(&destination_path\)/);
  const nativeExport = section(core, 'pub fn native_export', 'pub fn resolve_directory_grant_import_sources');
  assert.match(nativeExport, /NativeDirectory::open\(&destination_root\)/);
  assert.match(nativeExport, /native_root\.ensure_root_identity\(&destination_meta\)/);
  assert.match(exportRoute, /lock_destination_api/);
  assert.match(exportRoute, /unlock_destination/);
  const observer = section(sdk, 'class OperationObserver', 'class StreamWriter');
  assert.match(observer, /OPERATION_NOT_FOUND/);
  assert.match(observer, /OUTCOME_UNKNOWN/);
});

test('0.2 health/runtime capability discovery advertises the native export feature gates', () => {
  for (const capability of ['destination-picker-hints', 'internal-export-bookkeeping', 'tracked-export-prune', 'directory-contents-export']) {
    assert.match(protocol, new RegExp(capability));
    assert.match(runtime, new RegExp(capability));
  }
});

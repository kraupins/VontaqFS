import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const runtimePublic = runtime.split('#[cfg(test)]')[0];
const core = await readFile(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const desktop = await readFile(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const desktopCargo = await readFile(new URL('../../src-tauri/Cargo.toml', import.meta.url), 'utf8');
const uninstall = await readFile(new URL('../../docs/user/UNINSTALL.md', import.meta.url), 'utf8');

const repairEnd = core.indexOf('pub fn create_directory_grant');
const repair = core.slice(core.indexOf('pub fn repair_space'), repairEnd > 0 ? repairEnd : core.indexOf('pub fn export_space_zip'));
const exportFn = core.slice(core.indexOf('pub fn export_space_zip'), core.indexOf('fn quarantine_orphan_stream_temps_for_space'));

test('CP7 privileged operational controls stay outside the public loopback API', () => {
  for (const command of [
    'start_space_export', 'start_space_repair', 'start_clear_cache_space', 'diagnostics_report',
    'export_diagnostics', 'set_automatic_update_check', 'set_launch_on_login', 'quit_vontaqfs',
  ]) assert.match(desktop, new RegExp(`fn ${command}`));
  for (const route of ['/v1/admin', '/v1/repair', '/v1/export', '/v1/diagnostics', '/v1/preferences']) {
    assert.ok(!runtimePublic.includes(`\"${route}\"`), `privileged route leaked into public HTTP: ${route}`);
  }
});

test('CP7 update preference is local, defaults On, and signed updater actions fail closed until configured', () => {
  assert.match(runtime, /automatic_update_check: true/);
  assert.match(runtime, /runtime\/preferences\.json/);
  assert.match(runtime, /VONTAQFS_UPDATE_ENDPOINT/);
  assert.match(runtime, /VONTAQFS_UPDATER_PUBLIC_KEY/);
  assert.match(runtime, /option_env!\("VONTAQFS_UPDATE_ENDPOINT"\)/);
  assert.match(runtime, /release_transport_ready: endpoint_is_https && public_key_configured/);
  assert.match(desktopCargo, /tauri-plugin-updater = "=2\.12\.0"/);
  assert.match(desktop, /tauri_plugin_updater::Builder::new\(\)\.build\(\)/);
  assert.match(desktop, /fn updater_status/);
  assert.match(desktop, /async fn check_for_updates/);
  assert.match(desktop, /async fn download_update/);
  assert.match(desktop, /async fn install_update_and_restart/);
  assert.match(desktop, /updater_builder\(\)/);
  assert.match(desktop, /\.pubkey\(public_key\)/);
  assert.match(desktop, /pending\.update\.download/);
  assert.match(desktop, /pending\.update\.install/);
  assert.match(desktop, /runtime\.handle\.shutdown\(\)\.await/);
  assert.match(desktop, /DesktopUpdaterPhase::NotConfigured/);
});

test('CP7 lifecycle keeps close-to-tray semantics and graceful explicit quit', () => {
  assert.match(desktop, /CloseRequested/);
  assert.match(desktop, /api\.prevent_close\(\)/);
  assert.match(desktop, /window_to_hide\.hide\(\)/);
  assert.match(desktop, /TrayIconBuilder/);
  assert.match(desktop, /runtime\.shutdown\(\)\.await/);
  assert.match(runtime, /RuntimeShuttingDown/);
  assert.match(runtime, /LIFECYCLE_DRAINING/);
  assert.match(runtime, /SHUTDOWN_GRACE/);
  assert.match(runtime, /SLEEP_RESUME_GAP_MS/);
  assert.match(runtime, /handle_detected_resume/);
  assert.match(runtime, /has_active_long_operations/);
  assert.match(runtime, /entry\.cancel\.store\(true/);
  assert.ok(runtimePublic.indexOf('match state.lifecycle_status()') < runtimePublic.indexOf('handle_write_stream_route(state, request)'), 'lifecycle gate must precede stream chunk dispatch');
  assert.match(desktop, /--minimized/);
});

test('CP7 repair is data-preserving and escalates ambiguous recovery instead of deleting payload', () => {
  assert.match(repair, /create_registry_backup/);
  assert.match(repair, /scan_file_index/);
  assert.match(repair, /destructive-recovery-required/);
  assert.match(repair, /RepairOutcome::DestructiveRecoveryRequired/);
  assert.match(repair, /metadata-commit/);
  assert.doesNotMatch(repair, /remove_file\s*\(/);
  assert.doesNotMatch(repair, /remove_dir_all\s*\(/);
});

test('CP7 cache clear is explicitly limited to cache storage', () => {
  const clear = core.slice(core.indexOf('pub fn clear_cache_space'), core.indexOf('pub fn diagnostics_summary'));
  assert.match(clear, /space\.storage_class != StorageClass::Cache/);
  assert.match(clear, /clear cache is only valid for cache spaces/);
  assert.match(clear, /DELETE FROM kv_entries/);
  assert.match(clear, /DELETE FROM file_entries/);
});

test('CP7 portable export is versioned, checksummed, ZIP64-capable and excludes auth tables by construction', () => {
  assert.match(exportFn, /vontaqfs-storage-export/);
  assert.match(exportFn, /vontaqfs-export\.json/);
  assert.match(exportFn, /kv\.json/);
  assert.match(exportFn, /checksums\.json/);
  assert.doesNotMatch(exportFn, /pairings|credential_hash|session token|grants/i);
  assert.match(core, /ZIP64/);
  assert.match(core, /archive_sha256/);
  assert.match(core, /sync_all\(\)/);
});

test('CP7 diagnostics are local metadata/log summaries and contain no automatic upload path', () => {
  assert.match(runtime, /vontaqfs-diagnostics/);
  assert.match(runtime, /recent_sanitized_logs/);
  assert.match(runtime, /diagnostics_summary/);
  assert.doesNotMatch(runtimePublic, /uploadDiagnostics|sendDiagnostics|postDiagnostics/i);
  assert.match(runtime, /sanitize_log_atom/);
});

test('CP7 long operation model exposes real phase, progress and cancellability transitions', () => {
  assert.match(runtime, /pub struct LongOperationSnapshot/);
  assert.match(runtime, /pub phase: String/);
  assert.match(runtime, /pub completed: Option<u64>/);
  assert.match(runtime, /pub total: Option<u64>/);
  assert.match(runtime, /pub cancellable: bool/);
  assert.match(runtime, /set_operation_cancellable/);
  assert.match(core, /metadata-commit/);
});

test('CP7 uninstall contract preserves user storage by default', () => {
  assert.match(uninstall, /keeps? .*storage|data .*kept|preserv/i);
  assert.match(uninstall, /separate|explicit/i);
});

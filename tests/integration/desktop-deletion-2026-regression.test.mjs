import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const read = async (path) => readFile(new URL(path, import.meta.url),'utf8');
const [storage,runtime,tauri,desktop,api,desktopEntry] = await Promise.all([
  read('../../crates/core/src/storage.rs'),read('../../crates/runtime/src/lib.rs'),
  read('../../src-tauri/src/lib.rs'),read('../../apps/desktop/src/main.tsx'),
  read('../../apps/desktop/src/desktopApi.ts'),
  read('../../src-tauri/src/main.rs'),
]);

test('space deletion removes terminal durable stream journal before space row', () => {
  const helper=storage.slice(storage.indexOf('fn delete_space_registry_rows('));
  assert.ok(helper.includes('phase NOT IN'));
  assert.ok(helper.indexOf('DELETE FROM stream_operations WHERE space_id') < helper.indexOf('DELETE FROM spaces WHERE id'));
  assert.match(storage, /fn deletion_cleans_terminal_stream_journal_and_keeps_other_spaces/);
  assert.match(storage, /fn deletion_refuses_open_journal_and_restores_quarantine_on_restart/);
});
test('full application deletion removes own references in FK-safe order and preserves unrelated data', () => {
  const impl=storage.slice(storage.indexOf('pub fn delete_application_data('),storage.indexOf('fn quarantine_space_root('));
  for(const name of ['delete_space_registry_rows','DELETE FROM pairings','DELETE FROM directory_grants','DELETE FROM export_presets','DELETE FROM application_formats','DELETE FROM application_mutation_receipts','DELETE FROM applications']) assert.ok(impl.includes(name),name);
  assert.ok(impl.indexOf('DELETE FROM pairing_transport_policies') < impl.indexOf('DELETE FROM pairings'));
  assert.ok(impl.indexOf('DELETE FROM export_presets') < impl.indexOf('DELETE FROM directory_grants'));
  assert.ok(impl.includes('tx.commit()?'));
  assert.match(storage,/fn full_application_deletion_removes_own_registry_rows_not_other_apps/);
});
test('crash recovery restores staged directories still referenced by the registry', () => {
  const fn=storage.slice(storage.indexOf('fn cleanup_deletion_quarantine('),storage.indexOf('fn quarantine_orphan_stream_temps_for_space('));
  assert.match(fn,/self\.get_space\(id\)\?\.is_some\(\)/);
  assert.match(fn,/fs::rename\(&path, original\)/);
  assert.match(fn,/quarantined managed space/);
});
test('per-app deletion can only run from desktop commands and prevents live-session deletion', () => {
  assert.match(runtime,/pub fn start_delete_application\(/);
  assert.match(runtime,/application is connected; disconnect its active sessions/);
  assert.match(runtime,/deleting_applications: Mutex<HashSet<String>>/);
  assert.match(runtime,/if deleting\.contains\(&pairing\.application_id\)/);
  assert.match(tauri,/fn start_delete_application\(/);
  assert.match(tauri,/start_delete_application,/);
  assert.match(api,/deleteApplication:[\s\S]*'start_delete_application'/);
});
test('UI differentiates space deletion and full app removal, requires DELETE for both', () => {
  assert.match(desktop,/Remove application completely/);
  assert.match(desktop,/state\.kind === 'delete' \|\| state\.kind === 'delete-application'/);
  assert.match(desktop,/deletePhrase !== 'DELETE'/);
  assert.match(desktop,/External exported files and the installed plugin\/application are NOT removed/);
  assert.match(desktop,/next\.status === 'failed'[\s\S]*setActionError/);
});

test('deletion lifecycle keeps attention WebViews alive instead of closing them on empty work', () => {
  const operations = tauri.slice(tauri.indexOf('fn spawn_operations_attention('), tauri.indexOf('fn spawn_pairing_attention('));
  const pairing = tauri.slice(tauri.indexOf('fn spawn_pairing_attention('), tauri.indexOf('fn install_lifecycle_tray('));
  assert.match(operations, /window\.hide\(\)/);
  assert.doesNotMatch(operations, /window\.close\(\)/);
  assert.match(operations, /window\.is_visible\(\)/);
  assert.match(operations, /ensure_operations_window\(&app\)/);
  assert.match(pairing, /window\.hide\(\)/);
  assert.doesNotMatch(pairing, /window\.close\(\)/);
  assert.match(tauri, /api\.prevent_close\(\)/);
  assert.match(tauri, /RunEvent::ExitRequested \{ code: None, api/);
  assert.match(tauri, /api\.prevent_exit\(\)/);
  assert.match(tauri, /code: Some\(_\)/);
});

test('installed Windows builds record local panic and deletion stages without exposing IDs or paths', () => {
  assert.match(desktopEntry, /APPDATA/);
  assert.match(desktopEntry, /desktop-crash\.log/);
  assert.match(desktopEntry, /Backtrace::force_capture/);
  assert.match(tauri, /desktop-events\.log/);
  assert.match(tauri, /DESKTOP_EVENT_LOG_LOCK/);
  assert.match(tauri, /trace_delete_completion/);
  assert.match(tauri, /trace_desktop_event\("delete-space requested"\)/);
  assert.match(tauri, /trace_desktop_event\("delete-application requested"\)/);
  assert.doesNotMatch(tauri, /trace_desktop_event\(&space_id/);
  assert.doesNotMatch(tauri, /trace_desktop_event\(&application_id/);
});

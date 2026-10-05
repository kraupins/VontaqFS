import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const ui = await readFile(new URL('../../apps/desktop/src/main.tsx', import.meta.url), 'utf8');
const api = await readFile(new URL('../../apps/desktop/src/desktopApi.ts', import.meta.url), 'utf8');
const css = await readFile(new URL('../../apps/desktop/src/styles.css', import.meta.url), 'utf8');
const tauri = await readFile(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const storage = await readFile(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const releaseWorkflow = await readFile(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8');

function sliceBetween(source, start, end) {
  const from = source.indexOf(start);
  assert.notEqual(from, -1, `missing start marker: ${start}`);
  const to = source.indexOf(end, from + start.length);
  assert.notEqual(to, -1, `missing end marker: ${end}`);
  return source.slice(from, to);
}

test('CP10 keeps the locked top-level IA to Overview / Applications / Settings', () => {
  const nav = sliceBetween(ui, '<nav aria-label="Primary">', '</nav>');
  for (const label of ['Overview', 'Applications', 'Settings']) assert.ok(nav.includes(label));
  for (const forbidden of ['Storages', 'Connections', 'Permissions', 'Diagnostics']) assert.ok(!nav.includes(`>${forbidden}<`), `${forbidden} must not be top-level navigation`);
});

test('CP10 pairing uses a dedicated queued approval window with the locked capability and Allow / Deny only', () => {
  assert.match(tauri, /WebviewWindowBuilder::new\(app, "pairing"/);
  assert.match(tauri, /always_on_top\(true\)/);
  assert.match(runtime, /values\.sort_by_key\(\|entry\| \(entry\.created_at_ms, entry\.request_id\.clone\(\)\)\)/);
  const pairingSurface = sliceBetween(ui, 'function PairingSurface()', 'function App()');
  for (const label of ['Own isolated storage', '>Allow<', '>Deny<', 'Technical details', 'shown next']) assert.ok(pairingSurface.includes(label), `missing pairing UX: ${label}`);
  assert.ok(!pairingSurface.includes('Remember'), 'pairing must not expose a Remember control');
});

test('CP10 tray/main-window lifecycle keeps runtime alive and confirms Quit only for active sessions', () => {
  assert.match(tauri, /CloseRequested[\s\S]*prevent_close\(\)[\s\S]*window_to_hide\.hide\(\)/);
  assert.match(tauri, /"pairing-attention"[\s\S]*ensure_pairing_window/);
  assert.match(tauri, /if sessions == 0 \{ graceful_exit\(app\); return; \}/);
  assert.match(tauri, /vontaqfs-confirm-quit/);
  assert.match(tauri, /pending != had_pending \|\| status != last_status/);
  assert.match(ui, /listen<number>\('vontaqfs-confirm-quit'/);
});

test('CP10 keeps revoke, cache clear and persistent delete as distinct consequences', () => {
  assert.match(ui, /Access revoked — data kept/);
  assert.match(ui, /Revoking access never deletes storage/);
  assert.match(ui, /Type <strong>DELETE<\/strong> to confirm/);
  assert.match(ui, /This removes disposable cache data only\. Persistent storage is not affected\./);
  assert.match(api, /revokePairing:[\s\S]*'revoke_pairing'/);
  assert.match(api, /clearCache:[\s\S]*'start_clear_cache_space'/);
  assert.match(api, /deleteSpace:[\s\S]*'start_delete_space'/);
  assert.match(storage, /pub fn delete_space\(&self, space_id: &str\)/);
  assert.match(storage, /runtime\/deletion-quarantine/);
});

test('CP10 shows inline operation state and reserves the blocking overlay for repair metadata commit', () => {
  assert.match(ui, /assignedOperation=\{spaceOperations\[space\.id\]\}/);
  assert.match(ui, /setSpaceOperationIds[\s\S]*current\.space\.id/);
  assert.match(ui, /function OperationRow/);
  assert.match(ui, /operation\.cancellable[\s\S]*>Cancel</);
  assert.match(ui, /operation\.kind === 'repair-space'[\s\S]*operation\.phase === 'metadata-commit'[\s\S]*!operation\.cancellable/);
  assert.match(ui, /Finishing storage repair/);
});

test('CP10 keeps Runtime health separate from per-storage health', () => {
  assert.match(ui, /Storage service and authenticated loopback endpoint are separate from individual storage health/);
  assert.match(ui, /spaces\.filter\(\(space\) => space\.state !== 'healthy'\)/);
  assert.match(ui, /<StatusPill value=\{space\.state\}/);
  assert.match(ui, /className="metric-card runtime-card"/);
  assert.match(ui, /className="metric-card storage-card"/);
});

test('CP10 exposes persisted update preference and explicit check/download/install states', () => {
  for (const label of ['Automatically check for updates', 'Check now', 'Download update', 'Install and restart']) assert.ok(ui.includes(label), `missing update UX: ${label}`);
  assert.match(api, /setAutomaticUpdateCheck/);
  assert.match(api, /checkForUpdates/);
  assert.match(api, /downloadUpdate/);
  assert.match(api, /installUpdateAndRestart/);
  assert.match(tauri, /perform_update_check/);
});

test('CP10 repair is explicitly data-preserving and destructive recovery is not hidden behind Repair', () => {
  assert.match(ui, /Repair will not delete or rewrite user payloads/);
  assert.match(ui, /destructive-recovery-required/);
  assert.match(runtime, /"destructive-recovery-required"/);
  assert.match(runtime, /start_repair_space/);
});

test('CP10 unreachable helper does not pretend installation state is knowable', () => {
  assert.match(ui, /VontaqFS isn’t reachable/);
  assert.match(ui, /Install or start VontaqFS/);
  assert.match(ui, />Try again</);
  const unreachable = sliceBetween(ui, 'if (runtimeError)', 'return (\n    <div className="app-shell">');
  assert.doesNotMatch(unreachable, /not installed|isn['’]t installed|was never installed/i);
});

test('CP10 public policies are exposed from a release-configured public repository without a hardcoded owner', () => {
  for (const doc of ['privacy', 'terms', 'security', 'compliance']) assert.ok(ui.includes(`openPolicy('${doc}')`));
  assert.match(api, /openPublicDocument/);
  assert.match(tauri, /"privacy" => "PRIVACY\.md"/);
  assert.match(tauri, /"terms" => "TERMS\.md"/);
  assert.match(tauri, /"security" => "SECURITY\.md"/);
  assert.match(tauri, /"compliance" => "COMPLIANCE\.md"/);
  assert.match(tauri, /VONTAQFS_PUBLIC_REPOSITORY_URL/);
  assert.match(tauri, /VONTAQFS_PUBLIC_REPOSITORY_REF/);
  assert.match(tauri, /blob\/\{reference\}\/\{file\}/);
  assert.match(releaseWorkflow, /VONTAQFS_PUBLIC_REPOSITORY_URL:\s*https:\/\/github\.com\/\$\{\{ github\.repository \}\}/);
  assert.match(releaseWorkflow, /VONTAQFS_PUBLIC_REPOSITORY_REF:\s*\$\{\{ github\.ref_name \}\}/);
  assert.doesNotMatch(tauri, /github\.com\/kraupins|github\.com\/vontaq/i);
});

test('CP10 Desktop admin commands remain outside the public localhost protocol', () => {
  const publicRoutes = sliceBetween(runtime, 'async fn handle_request(', 'async fn handle_write_stream_route(');
  for (const forbidden of ['desktop_applications', 'reveal_space', 'start_delete_space', 'open_public_document', '/v1/admin', '/v1/spaces/delete']) {
    assert.ok(!publicRoutes.includes(forbidden), `public HTTP surface leaked privileged operation: ${forbidden}`);
  }
  for (const desktopCommand of ['desktop_applications', 'reveal_space', 'start_delete_space', 'open_public_document']) assert.ok(tauri.includes(desktopCommand));
});

test('CP10 Desktop application summaries do not expose pairing credential hashes', () => {
  const dto = sliceBetween(runtime, 'pub struct DesktopPairingSummary', 'pub struct DesktopApplicationSummary');
  assert.doesNotMatch(dto, /credential_hash|credentialHash/);
  assert.doesNotMatch(api, /credentialHash|credential_hash/);
  const accessSection = sliceBetween(ui, '<h3>Access & connections</h3>', '<h3>Storage</h3>');
  assert.doesNotMatch(accessSection, /clientInstanceId/);
  assert.match(ui, /<summary>Technical details<\/summary>[\s\S]*pairing\.clientInstanceId/);
});

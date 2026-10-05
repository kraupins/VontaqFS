import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const read = (rel) => fs.readFileSync(new URL(rel, import.meta.url), 'utf8');
const runtime = read('../../crates/runtime/src/lib.rs');
const tauri = read('../../src-tauri/src/lib.rs');
const api = read('../../apps/desktop/src/desktopApi.ts');
const desktop = read('../../apps/desktop/src/main.tsx');
const preview = read('../../apps/desktop/src/filePreview.ts');
const privacy = read('../../PRIVACY.md');
const terms = read('../../TERMS.md');
const security = read('../../SECURITY.md');
const compliance = read('../../COMPLIANCE.md');
const figma = read('../../packages/sdk/src/figma.ts');
const sdkReadme = read('../../packages/sdk/README.md');
const managerCore = read('../../../vontaq-workspace-manager/supervisor/core.mjs');
const managerSupervisor = read('../../../vontaq-workspace-manager/supervisor/supervisor.mjs');
const managerDashboard = read('../../../vontaq-workspace-manager/media/dashboard.js');
const managerReadme = read('../../../vontaq-workspace-manager/readme.md');
const foundationStatus = read('../../docs/developer/FOUNDATION_STATUS.md');
const releaseCandidate = read('../../docs/developer/RELEASE_CANDIDATE.md');

function section(source, start, end) {
  const a = source.indexOf(start); assert.notEqual(a, -1, `missing ${start}`);
  const b = end ? source.indexOf(end, a) : source.length; assert.notEqual(b, -1, `missing ${end}`);
  return source.slice(a, b);
}

test('D9 Desktop application summaries expose grants and presets without physical OS paths', () => {
  const grant = section(runtime, 'pub struct DesktopDirectoryGrantSummary', 'pub struct DesktopExportPresetSummary');
  assert.match(grant, /id: String/);
  assert.match(grant, /label: String/);
  assert.match(grant, /capability: DirectoryGrantCapability/);
  assert.doesNotMatch(grant, /physical_path/);
  const appSummary = section(runtime, 'pub struct DesktopApplicationSummary', '#[derive(Clone)]');
  assert.match(appSummary, /destinations: Vec<DesktopDirectoryGrantSummary>/);
  assert.match(appSummary, /export_presets: Vec<DesktopExportPresetSummary>/);
  assert.match(runtime, /revoke_desktop_destination/);
  assert.match(runtime, /delete_desktop_export_preset/);
  assert.match(tauri, /revoke_desktop_destination/);
  assert.match(tauri, /delete_desktop_export_preset/);
  assert.match(api, /revokeDestination/);
  assert.match(api, /deleteExportPreset/);
});

test('D9 Desktop keeps locked IA and adds capability details inside Applications', () => {
  assert.match(desktop, /type Page = 'overview' \| 'applications' \| 'settings'/);
  assert.match(desktop, /Saved directories & destinations/);
  assert.match(desktop, /Export presets/);
  assert.match(desktop, /Snapshots & backups/);
  assert.match(desktop, /storageCategory/);
  assert.match(desktop, /Official endpoints/);
  assert.match(desktop, /Occupied endpoints/);
  assert.match(desktop, /Files already exported there remain untouched/);
});

test('D9 opaque/custom preview remains metadata-driven and never sniffs proprietary bytes', () => {
  assert.match(preview, /if \(file\.opaque === true\) return 'metadata-only'/);
  assert.match(preview, /contentType/);
  assert.doesNotMatch(preview, /readFile|arrayBuffer|TextDecoder|JSON\.parse/);
  assert.match(desktop, /Unknown, custom or opaque file payloads are never parsed just to create a preview/);
});

test('D9 public policy documents state local grants, opaque encryption, TOFU and official API boundaries', () => {
  assert.match(privacy, /actual OS path locally/);
  assert.match(privacy, /opaque grant ID/);
  assert.match(privacy, /encrypted or opaque files remain ciphertext\/opaque bytes/);
  assert.match(terms, /read, write or read-write access/);
  assert.match(terms, /Restore does not silently grant a plugin access/);
  assert.match(security, /47833, 47834, 47835, 47836/);
  assert.match(security, /pairing-bound HMAC challenge/);
  assert.match(security, /trust-on-first-use/);
  assert.match(compliance, /official Figma Plugin\/Widget API/);
  assert.match(compliance, /user explicitly selects\/authorizes/);
});

test('D9 SDK docs and helper keep Figma developers away from manual port handling', () => {
  assert.match(figma, /VONTAQ_FS_FIGMA_NETWORK_ACCESS/);
  assert.match(figma, /VONTAQ_FS_ENDPOINTS/);
  assert.match(sdkReadme, /Application code still calls `VontaqFS\.connect\(\)` and never probes or chooses ports itself/);
  assert.match(sdkReadme, /Native export, import and saved directories/);
  assert.match(sdkReadme, /Batch, storage categories and Workspace/);
});

test('D9 Manager records the selected VFS readiness endpoint while preserving endpoint-pool readiness', () => {
  assert.match(managerCore, /urls: \[\.\.\.VFS_HEALTH_URLS\]/);
  assert.match(managerCore, /expect: \{ service: 'vontaqfs' \}/);
  assert.match(managerSupervisor, /op\.readinessUrl = result\?\.url \|\| null/);
  assert.match(managerSupervisor, /entry\.port = Number\(new URL\(entry\.readinessUrl\)\.port\)/);
  assert.match(managerDashboard, /127\.0\.0\.1:\$\{run\.port\}/);
  assert.match(managerReadme, /official endpoint pool/);
  assert.match(managerReadme, /privateBackup: false/);
});

test('D9/D10 release/status docs separate historical validation from current owner-pending source integration', () => {
  assert.match(foundationStatus, /post-build DEV CHECKPOINT D10/);
  assert.match(foundationStatus, /OWNER RUNTIME VALIDATION PENDING/);
  assert.match(foundationStatus, /Historical validation/);
  assert.match(releaseCandidate, /post-build integration delta through D10/i);
  assert.match(releaseCandidate, /Native Rust\/Tauri compilation and end-to-end runtime behavior are \*\*NOT RUN \/ UNVERIFIED\*\*/);
});

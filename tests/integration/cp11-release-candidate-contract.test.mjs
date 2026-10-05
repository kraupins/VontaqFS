import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const core = await readFile(new URL('../../crates/core/src/storage.rs', import.meta.url), 'utf8');
const runtime = await readFile(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');
const sdk = await readFile(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');
const figma = await readFile(new URL('../../packages/sdk/src/figma.ts', import.meta.url), 'utf8');
const example = await readFile(new URL('../../examples/figma-plugin/src/code.ts', import.meta.url), 'utf8');
const exampleTsconfig = JSON.parse(await readFile(new URL('../../examples/figma-plugin/tsconfig.json', import.meta.url), 'utf8'));
const release = await readFile(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8');
const compliance = await readFile(new URL('../../COMPLIANCE.md', import.meta.url), 'utf8');
const privacy = await readFile(new URL('../../PRIVACY.md', import.meta.url), 'utf8');
const terms = await readFile(new URL('../../TERMS.md', import.meta.url), 'utf8');
const security = await readFile(new URL('../../SECURITY.md', import.meta.url), 'utf8');
const fixture = JSON.parse(await readFile(new URL('../fixtures/bridge-readiness.json', import.meta.url), 'utf8'));
const productionRuntime = runtime.split('#[cfg(test)]')[0];

const mib = 1024 * 1024;

test('CP11 enforces the free-disk reserve before direct writes, stream growth and server-side copies', () => {
  assert.match(core, /MIN_FREE_DISK_RESERVE_BYTES: u64 = 512 \* 1024 \* 1024/);
  assert.match(core, /FREE_DISK_RESERVE_PERCENT_DENOMINATOR: u64 = 50/);
  assert.match(core, /ensure_disk_reserve\(&data_root, bytes\.len\(\) as u64\)/);
  assert.match(core, /self\.ensure_write_capacity\(space_id, record\.size\)/);
  assert.match(productionRuntime, /ensure_write_capacity\(&input\.space_id, declared_size\)/);
  assert.match(productionRuntime, /ensure_write_capacity\(&stream\.space_id, request\.body\.len\(\) as u64\)/);
  assert.match(productionRuntime, /ErrorCode::DiskSpaceLow/);
  for (const op of ['delete_path_with_progress', 'copy_path_with_progress', 'move_path_with_progress']) assert.match(productionRuntime, new RegExp(`spawn_blocking\\(move\\s*\\|\\|\\s*storage\\.${op}`));
  assert.match(core, /if fs::rename\(&source, &destination\)\.is_ok\(\)/);
  assert.match(core, /move_prefers_same_space_rename_when_destination_is_absent/);
  assert.match(core, /release_candidate_stress_50000_files_and_10000_recursive_delete/);
  assert.match(release, /npm run test:rc-stress/);
});

test('CP11 startup recovery visits each pending file mutation once', () => {
  const body = core.match(/fn recover_pending_mutations\(&self\)[\s\S]*?\n    fn recover_pending\(/)?.[0] ?? '';
  assert.equal((body.match(/self\.recover_pending\(mutation\)/g) ?? []).length, 1);
});

test('CP11 Bridge-readiness fixture stays application-neutral and covers small, large, binary and project-scoped data', () => {
  assert.equal(fixture.projectSpace.storageClass, 'persistent');
  assert.equal(fixture.smallState.kvKey, 'analysis-resume');
  assert.ok(fixture.largeJson.generatedPayloadBytes > 256 * 1024);
  assert.ok(fixture.binaryAsset.generatedBytes > 256 * 1024);
  assert.match(fixture.largeJson.path, /^\//);
  assert.match(fixture.binaryAsset.path, /^\//);
  assert.doesNotMatch(JSON.stringify(fixture), /authToken|sessionToken|sharedPluginData/i);
});

test('CP11 public SDK exposes the required generic primitives without Bridge-specific schema', () => {
  for (const token of ['openSpace', 'readonly files', 'readonly kv', 'createReader', 'createWriter', 'watch']) {
    assert.ok(sdk.includes(token), `missing SDK primitive ${token}`);
  }
  assert.doesNotMatch(sdk, /CanonicalProjectAnalysis|BridgeAnalysisResume|vontaq-bridge/i);
  assert.doesNotMatch(figma, /CanonicalProjectAnalysis|BridgeAnalysisResume|vontaq-bridge/i);
});

test('CP11 example consumes only the public SDK surface and the fixed loopback contract', () => {
  assert.match(example, /from '@vontaq\/fs'/);
  assert.match(example, /from '@vontaq\/fs\/figma'/);
  assert.doesNotMatch(example, /\.\.\/\.\.\/Bridge|vontaq-bridge|47831|47832/);
  assert.equal(exampleTsconfig.compilerOptions.strict, true);
  assert.equal(exampleTsconfig.compilerOptions.paths['@vontaq/fs'][0], '../../packages/sdk/src/index.ts');
});

test('CP11 runtime remains loopback-only and exposes no public Desktop admin endpoints', () => {
  assert.match(productionRuntime, /Ipv4Addr::LOCALHOST/);
  assert.match(productionRuntime, /Ipv6Addr::LOCALHOST/);
  assert.doesNotMatch(productionRuntime, /0\.0\.0\.0/);
  for (const forbidden of ['/v1/admin', '/v1/diagnostics/export', '/v1/storage/delete', '/v1/pairings/approve', '/v1/pairings/revoke']) {
    assert.equal(productionRuntime.includes(forbidden), false, `public API leaked privileged route ${forbidden}`);
  }
});

test('CP11 compliance boundary explicitly rejects Figma Desktop internals and circumvention', () => {
  for (const phrase of ['inject code into Figma Desktop', 'read Figma process memory', 'private Figma application databases', 'extract Figma credentials']) {
    assert.match(compliance, new RegExp(phrase.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i'));
  }
});

test('CP11 policy files retain explicit owner/legal release gates instead of invented values', () => {
  assert.match(privacy, /\[INSERT PRIVACY CONTACT\]/);
  assert.match(terms, /LEGAL REVIEW REQUIRED/);
  assert.match(security, /\[INSERT SECURITY CONTACT\]/);
});

test('CP11 GitHub CI supports four architectures and the owner-selected self-signed distribution mode', () => {
  for (const target of ['x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc', 'aarch64-apple-darwin', 'x86_64-apple-darwin']) {
    assert.ok(release.includes(target), `missing ${target}`);
  }
  assert.match(release, /pfx-self-signed/);
  assert.match(release, /MACOS_SIGNING_MODE/);
  assert.match(release, /self-signed\|developer-id/);
  assert.match(release, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(release, /VONTAQFS_UPDATER_PUBLIC_KEY/);
  assert.match(release, /PUBLISH_NPM/);
  assert.doesNotMatch(release, /continue-on-error:\s*true/);
});

test('CP11 large-data limits remain aligned with the RC fixture', () => {
  assert.ok(fixture.largeJson.generatedPayloadBytes < 64 * mib);
  assert.ok(fixture.binaryAsset.generatedBytes < 64 * mib);
  assert.match(sdk, /MATERIALIZATION_LIMIT/);
  assert.match(sdk, /DIRECT_PAYLOAD_TARGET_BYTES/);
});

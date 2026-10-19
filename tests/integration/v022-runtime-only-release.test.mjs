import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { sdkPublishEligibility } from '../../scripts/release/sdk-publish-eligibility.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const read = rel => readFileSync(path.join(root, rel), 'utf8');
const pkg = JSON.parse(read('package.json'));
const sdk = JSON.parse(read('packages/sdk/package.json'));
const lock = JSON.parse(read('package-lock.json'));
const workflow = read('.github/workflows/release.yml');
const spawn = (script, env) => spawnSync(process.execPath, [path.join(root, script)], {
  cwd: root, env: {...process.env, ...env}, encoding: 'utf8',
});

test('Runtime/Desktop and SDK 0.2.3 versions agree across package and lock', () => {
  assert.equal(pkg.version, '0.2.3');
  assert.equal(sdk.version, '0.2.3');
  assert.equal(lock.packages[''].version, pkg.version);
  assert.equal(lock.packages['apps/desktop'].version, pkg.version);
  assert.equal(lock.packages['packages/sdk'].version, sdk.version);
  assert.equal(JSON.parse(read('apps/desktop/package.json')).version, pkg.version);
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version, pkg.version);
  for (const crate of ['vontaqfs-core', 'vontaqfs-protocol', 'vontaqfs-runtime', 'vontaqfs-desktop']) {
    assert.match(read('Cargo.lock'), new RegExp(`name = "${crate}"\\nversion = "0\\.2\\.3"`));
  }
});

test('npm publication is blocked in both PUBLISH_NPM branches for a runtime-only patch', () => {
  for (const publishNpm of ['true', 'false']) {
    const result = sdkPublishEligibility({
      runtimeVersion: '0.2.2', sdkVersion: '0.2.1', releaseTag: 'v0.2.2', publishNpm,
    });
    assert.equal(result.publish, false);
    assert.match(result.reason, /runtime-only/);
    // The 0.2.2 / 0.2.1 scenario is historical; test the pure policy without invoking current release metadata.
  }
  assert.match(workflow, /id: sdk_release/);
  assert.match(workflow, /PUBLISH_NPM: \$\{\{ vars\.PUBLISH_NPM \}\}/);
  assert.match(workflow, /sdk-publish-eligibility\.mjs/);
  const publishSection = workflow.split('  publish-sdk:')[1].split('  finalize-release:')[0];
  for (const block of ['id: npm_state', 'npm publish --workspace @vontaq/fs --access public']) {
    assert.ok(publishSection.includes(block));
  }
  assert.match(publishSection, /if: vars\.PUBLISH_NPM == 'true' && steps\.sdk_release\.outputs\.publish == 'true'/);
  assert.match(publishSection, /steps\.sdk_release\.outputs\.publish == 'true' && steps\.npm_state\.outputs\.already_published != 'true'/);
});

test('matching SDK release still requires explicit npm opt-in and matching tag', () => {
  const args = {runtimeVersion: '0.2.3', sdkVersion: '0.2.3', releaseTag: 'v0.2.3'};
  assert.equal(sdkPublishEligibility({...args, publishNpm: 'false'}).publish, false);
  assert.equal(sdkPublishEligibility({...args, publishNpm: 'true'}).publish, true);
  assert.throws(() => sdkPublishEligibility({...args, releaseTag: 'v0.2.2', publishNpm: 'true'}), /tag/);
});

test('release verify accepts v0.2.3 and rejects a mismatched old tag', () => {
  const gate = path.join(root, 'scripts/release/verify-release.mjs');
  const ok = spawnSync(process.execPath, [gate, '--tag', 'v0.2.3'], {cwd: root, encoding:'utf8'});
  assert.equal(ok.status, 0, ok.stderr);
  assert.equal(JSON.parse(ok.stdout).sdkVersion, '0.2.3');
  const old = spawnSync(process.execPath, [gate, '--tag', 'v0.2.1'], {cwd: root, encoding:'utf8'});
  assert.notEqual(old.status, 0);
  assert.match(old.stderr, /does not match/);
});

test('workflow retains signed updater with protocol v1 feature negotiation', () => {
  assert.match(workflow, /releases\/latest\/download\/latest\.json/);
  assert.match(workflow, /generate-updater-json\.mjs/);
  assert.match(workflow, /verify-published-updater\.mjs/);
  assert.match(read('crates/protocol/src/lib.rs'), /PROTOCOL_MIN[^\n]*1/);
  assert.match(read('crates/protocol/src/lib.rs'), /PROTOCOL_MAX[^\n]*1/);
});

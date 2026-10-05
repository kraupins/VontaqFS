import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const releaseWorkflow = await readFile(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8');
const tauri = JSON.parse(await readFile(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const sdk = JSON.parse(await readFile(new URL('../../packages/sdk/package.json', import.meta.url), 'utf8'));
const provisioning = await readFile(new URL('../../docs/developer/PRODUCTION_RELEASE.md', import.meta.url), 'utf8');
const verifierPath = new URL('../../scripts/release/verify-assets.mjs', import.meta.url);
const updaterPath = new URL('../../scripts/release/generate-updater-json.mjs', import.meta.url);
const checksumsPath = new URL('../../scripts/release/generate-checksums.mjs', import.meta.url);
const releaseGatePath = new URL('../../scripts/release/verify-release.mjs', import.meta.url);
const prepareWindowsSigning = new URL('../../scripts/release/prepare-windows-signing-config.mjs', import.meta.url);

function run(file, args, options = {}) {
  return spawnSync(process.execPath, [file.pathname, ...args], { encoding: 'utf8', ...options });
}

test('CP9 release workflow is tag-driven, protected and draft-first', () => {
  assert.match(releaseWorkflow, /tags:\s*\n\s*- 'v\*\.\*\.\*'/);
  assert.match(releaseWorkflow, /environment: production-release/);
  assert.match(releaseWorkflow, /--draft/);
  assert.match(releaseWorkflow, /--draft=false --latest/);
  assert.match(releaseWorkflow, /refusing to mutate a published release/i);
  assert.match(releaseWorkflow, /- run: npm run build\n/);
  assert.match(releaseWorkflow, /node-version: '24'/);
});

test('CP9 builds the required four desktop targets', () => {
  for (const value of ['x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc', 'aarch64-apple-darwin', 'x86_64-apple-darwin']) {
    assert.match(releaseWorkflow, new RegExp(value.replaceAll('-', '\\-')));
  }
  assert.match(releaseWorkflow, /windows-11-arm/);
  assert.match(releaseWorkflow, /macos-15-intel/);
});

test('CP9 release artifacts fail closed on signature integrity and support owner-selected self-signed mode', () => {
  assert.equal(tauri.bundle.createUpdaterArtifacts, true);
  for (const value of ['TAURI_SIGNING_PRIVATE_KEY', 'VONTAQFS_UPDATER_PUBLIC_KEY', 'Get-AuthenticodeSignature', 'codesign --verify', 'pfx-self-signed', 'MACOS_SIGNING_MODE', 'self-signed', 'developer-id']) {
    assert.ok(releaseWorkflow.includes(value), `missing release gate: ${value}`);
  }
  assert.match(releaseWorkflow, /WINDOWS_SIGNING_MODE/);
  assert.match(releaseWorkflow, /azure-artifact-signing/);
  assert.match(releaseWorkflow, /APPLE_API_PRIVATE_KEY/);
  assert.match(releaseWorkflow, /Self-signed macOS artifact: Apple notarization\/Gatekeeper trust is intentionally not asserted/);
  assert.match(releaseWorkflow, /WINDOWS_SIGNING_MODE -eq 'pfx-self-signed'/);
  assert.doesNotMatch(releaseWorkflow, /continue-on-error:\s*true/);
  assert.match(releaseWorkflow, /SIGNING_NOTICE\.txt/);
  assert.match(releaseWorkflow, /Apple notarization and Gatekeeper trust are intentionally not claimed/);
  assert.match(releaseWorkflow, /self-signed certificate.*publicly trusted/i);
});

test('CP9 npm publication remains OIDC-only and can be owner-deferred without blocking GitHub artifacts', () => {
  assert.equal(sdk.name, '@vontaq/fs');
  assert.equal(sdk.publishConfig.access, 'public');
  assert.match(releaseWorkflow, /id-token: write/);
  assert.match(releaseWorkflow, /vars\.PUBLISH_NPM == 'true'/);
  assert.match(releaseWorkflow, /npm publish --workspace @vontaq\/fs --access public/);
  assert.match(releaseWorkflow, /npm publication remains owner-deferred/);
  assert.doesNotMatch(releaseWorkflow, /NPM_TOKEN|NODE_AUTH_TOKEN/);
  assert.match(provisioning, /Trusted Publisher/);
  assert.match(provisioning, /pfx-self-signed/);
  assert.match(provisioning, /MACOS_SIGNING_MODE=self-signed/);
  assert.match(provisioning, /intentionally contains no `NPM_TOKEN`\/long-lived publish token/i);
});



test('CP9 self-signed Windows PFX overlay does not require a timestamp authority', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'vontaqfs-self-signed-'));
  try {
    const output = path.join(dir, 'windows.json');
    const result = run(prepareWindowsSigning, [output], {
      env: {
        ...process.env,
        WINDOWS_SIGNING_MODE: 'pfx-self-signed',
        WINDOWS_CERTIFICATE_THUMBPRINT: 'A'.repeat(40),
        WINDOWS_TIMESTAMP_URL: '',
      },
    });
    assert.equal(result.status, 0, result.stderr);
    const config = JSON.parse(await readFile(output, 'utf8'));
    assert.equal(config.bundle.windows.certificateThumbprint, 'A'.repeat(40));
    assert.equal(config.bundle.windows.digestAlgorithm, 'sha256');
    assert.equal('timestampUrl' in config.bundle.windows, false);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('CP9 updater manifest and checksum scripts produce and verify four-platform metadata', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'vontaqfs-cp9-'));
  try {
    const assets = [
      'VontaqFS_0.1.0_windows-x86_64-setup.exe',
      'VontaqFS_0.1.0_windows-x86_64.msi',
      'VontaqFS_0.1.0_windows-aarch64-setup.exe',
      'VontaqFS_0.1.0_windows-aarch64.msi',
      'VontaqFS_0.1.0_darwin-aarch64.dmg',
      'VontaqFS_0.1.0_darwin-aarch64.app.tar.gz',
      'VontaqFS_0.1.0_darwin-x86_64.dmg',
      'VontaqFS_0.1.0_darwin-x86_64.app.tar.gz',
    ];
    for (const name of assets) await writeFile(path.join(dir, name), `fixture:${name}`);
    for (const name of assets.filter((name) => name.endsWith('-setup.exe') || name.endsWith('.app.tar.gz'))) {
      await writeFile(path.join(dir, `${name}.sig`), `signature:${name}`);
    }
    let result = run(updaterPath, ['--dir', dir, '--repo', 'kraupins/VontaqFS', '--tag', 'v0.1.0', '--version', '0.1.0']);
    assert.equal(result.status, 0, result.stderr);
    result = run(checksumsPath, [dir]);
    assert.equal(result.status, 0, result.stderr);
    result = run(verifierPath, [dir]);
    assert.equal(result.status, 0, result.stderr);
    const latest = JSON.parse(await readFile(path.join(dir, 'latest.json'), 'utf8'));
    assert.deepEqual(Object.keys(latest.platforms).sort(), ['darwin-aarch64', 'darwin-x86_64', 'windows-aarch64', 'windows-x86_64']);
    assert.match(await readFile(path.join(dir, 'SHA256SUMS.txt'), 'utf8'), /latest\.json/);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('CP9 immutable release gate accepts the current coherent version/tag contract', () => {
  const result = run(releaseGatePath, ['--tag', 'v0.1.0'], { cwd: path.resolve(new URL('../..', import.meta.url).pathname) });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /\"ok\": true/);
});

test('CP9 release workflow generates metadata only after desktop and npm jobs pass', () => {
  assert.match(releaseWorkflow, /finalize-release:[\s\S]*needs: \[create-release, build-desktop, publish-sdk\]/);
  assert.match(releaseWorkflow, /generate-updater-json\.mjs/);
  assert.match(releaseWorkflow, /generate-checksums\.mjs/);
  assert.match(releaseWorkflow, /verify-assets\.mjs/);
});

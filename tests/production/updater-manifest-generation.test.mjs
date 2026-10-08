import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const generator = path.join(root, 'scripts/release/generate-updater-json.mjs');
const releaseVersion = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const releaseTag = `v${releaseVersion}`;

function createFixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'vontaqfs-updater-manifest-'));
  const assets = [
    `VontaqFS_${releaseVersion}_windows-x86_64-setup.exe`,
    `VontaqFS_${releaseVersion}_windows-x86_64.msi`,
    `VontaqFS_${releaseVersion}_windows-aarch64-setup.exe`,
    `VontaqFS_${releaseVersion}_windows-aarch64.msi`,
    `VontaqFS_${releaseVersion}_darwin-x86_64.app.tar.gz`,
    `VontaqFS_${releaseVersion}_darwin-aarch64.app.tar.gz`,
  ];
  for (const asset of assets) {
    fs.writeFileSync(path.join(directory, asset), `artifact:${asset}\n`);
    fs.writeFileSync(path.join(directory, `${asset}.sig`), `signature:${asset}\n`);
  }
  return directory;
}

function generate(legacyWindowsInstaller) {
  const directory = createFixture();
  const result = spawnSync(process.execPath, [
    generator,
    '--dir', directory,
    '--repo', 'kraupins/VontaqFS',
    '--tag', releaseTag,
    '--version', releaseVersion,
    '--legacy-windows-installer', legacyWindowsInstaller,
  ], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(fs.readFileSync(path.join(directory, 'latest.json'), 'utf8'));
}

test('updater manifest publishes both Windows installer families and MSI legacy fallback when requested', () => {
  const manifest = generate('msi');
  for (const key of ['windows-x86_64-nsis', 'windows-x86_64-msi', 'windows-aarch64-nsis', 'windows-aarch64-msi']) {
    assert.ok(manifest.platforms[key]?.url, `missing ${key}`);
    assert.ok(manifest.platforms[key]?.signature, `missing ${key} signature`);
  }
  assert.equal(manifest.platforms['windows-x86_64'].url, manifest.platforms['windows-x86_64-msi'].url);
  assert.equal(manifest.platforms['windows-aarch64'].url, manifest.platforms['windows-aarch64-msi'].url);
  assert.match(manifest.platforms['windows-x86_64-msi'].url, /\.msi$/);
  assert.match(manifest.platforms['windows-x86_64-nsis'].url, /-setup\.exe$/);
});

test('updater manifest keeps NSIS as production legacy fallback by default', () => {
  const directory = createFixture();
  const result = spawnSync(process.execPath, [
    generator,
    '--dir', directory,
    '--repo', 'kraupins/VontaqFS',
    '--tag', releaseTag,
    '--version', releaseVersion,
  ], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'latest.json'), 'utf8'));
  assert.equal(manifest.platforms['windows-x86_64'].url, manifest.platforms['windows-x86_64-nsis'].url);
});

import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const generator = path.join(root, 'scripts/release/generate-updater-json.mjs');
const repairSource = await readFile(path.join(root, 'scripts/release/repair-published-updater.mjs'), 'utf8');
const releaseVersion = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
const releaseTag = `v${releaseVersion}`;

const updaterAssets = [
  `VontaqFS_${releaseVersion}_windows-x86_64-setup.exe`,
  `VontaqFS_${releaseVersion}_windows-x86_64.msi`,
  `VontaqFS_${releaseVersion}_windows-aarch64-setup.exe`,
  `VontaqFS_${releaseVersion}_windows-aarch64.msi`,
  `VontaqFS_${releaseVersion}_darwin-x86_64.app.tar.gz`,
  `VontaqFS_${releaseVersion}_darwin-aarch64.app.tar.gz`,
];

test('updater manifest constructs tag/version asset URLs for every updater family', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'vontaqfs-updater-contract-'));
  try {
    for (const name of updaterAssets) {
      await writeFile(path.join(dir, name), `fixture:${name}`);
      await writeFile(path.join(dir, `${name}.sig`), `signature:${name}`);
    }
    const result = spawnSync(process.execPath, [
      generator,
      '--dir', dir,
      '--repo', 'kraupins/VontaqFS',
      '--tag', releaseTag,
      '--version', releaseVersion,
    ], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const latest = JSON.parse(await readFile(path.join(dir, 'latest.json'), 'utf8'));
    const releaseBase = `https://github.com/kraupins/VontaqFS/releases/download/${encodeURIComponent(releaseTag)}`;
    assert.equal(latest.platforms['windows-x86_64'].url, `${releaseBase}/${encodeURIComponent(updaterAssets[0])}`);
    assert.equal(latest.platforms['windows-aarch64'].url, `${releaseBase}/${encodeURIComponent(updaterAssets[2])}`);
    assert.equal(latest.platforms['darwin-x86_64'].url, `${releaseBase}/${encodeURIComponent(updaterAssets[4])}`);
    assert.equal(latest.platforms['darwin-aarch64'].url, `${releaseBase}/${encodeURIComponent(updaterAssets[5])}`);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('published updater repair replaces only latest.json using existing release assets and then verifies public transport', () => {
  assert.match(repairSource, /browser_download_url/);
  assert.match(repairSource, /downloadTextAsset\(repository, signatureAsset\)/);
  assert.match(repairSource, /release', 'upload'/);
  assert.match(repairSource, /'--clobber'/);
  assert.match(repairSource, /releases\/latest\/download\/latest\.json/);
  assert.match(repairSource, /Range: 'bytes=0-0'/);
});

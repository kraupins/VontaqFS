import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, stat } from 'node:fs/promises';
import { test } from 'node:test';

const root = new URL('../../', import.meta.url);
const read = async path => readFile(new URL(path, root), 'utf8');
const fixtureBase = new URL('../fixtures/v01-storage/', import.meta.url);

async function sha256(url) {
  return createHash('sha256').update(await readFile(url)).digest('hex');
}

test('0.2 release versions are synchronized while protocol/storage stay v1', async () => {
  const packageJson = JSON.parse(await read('package.json'));
  const sdk = JSON.parse(await read('packages/sdk/package.json'));
  const desktop = JSON.parse(await read('apps/desktop/package.json'));
  const tauri = JSON.parse(await read('src-tauri/tauri.conf.json'));
  const cargo = await read('Cargo.toml');
  const protocol = await read('crates/protocol/src/lib.rs');
  const storage = await read('crates/core/src/storage.rs');
  assert.equal(packageJson.version, '0.2.0');
  assert.equal(sdk.version, '0.2.0');
  assert.equal(desktop.version, '0.2.0');
  assert.equal(tauri.version, '0.2.0');
  assert.match(cargo, /\[workspace\.package\][\s\S]*version = "0\.2\.0"/);
  assert.match(protocol, /PROTOCOL_(?:MIN|MAX)[^\n]*1/);
  assert.match(storage, /const SCHEMA_VERSION: i64 = 1;/);
  assert.match(storage, /const SPACE_FORMAT_VERSION: u32 = 1;/);
});

test('frozen 0.1 compatibility fixture contains required records and exact-byte payloads', async () => {
  const fixture = JSON.parse(await readFile(new URL('fixture.json', fixtureBase), 'utf8'));
  assert.equal(fixture.sourceVersion, '0.1.0');
  assert.equal(fixture.registrySchemaVersion, 1);
  assert.equal(fixture.spaceFormatVersion, 1);
  for (const key of ['applicationId','pairingId','persistentSpaceId','cacheSpaceId','snapshotId','destinationGrantId','exportPresetId']) {
    assert.ok(fixture[key], `missing ${key}`);
  }
  const stream = new URL(`root/spaces/${fixture.persistentSpaceId}/data/stream.bin`, fixtureBase);
  assert.equal((await stat(stream)).size, fixture.expected.streamBytes);
  assert.equal(await sha256(stream), fixture.expected.streamSha256);
  const snapshotStream = new URL(`root/spaces/${fixture.persistentSpaceId}/.vontaqfs/snapshots/${fixture.snapshotId}/data/stream.bin`, fixtureBase);
  assert.equal(await sha256(snapshotStream), fixture.expected.streamSha256);
  const registry = await readFile(new URL('root/runtime/registry.sqlite3', fixtureBase));
  assert.equal(registry.subarray(0, 16).toString('utf8'), 'SQLite format 3\u0000');
});

test('fixture SHA256 manifest is frozen and complete', async () => {
  const sums = (await readFile(new URL('SHA256SUMS.txt', fixtureBase), 'utf8')).trim().split('\n');
  assert.ok(sums.length >= 8);
  for (const line of sums) {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    assert.ok(match, `invalid checksum line: ${line}`);
    assert.equal(await sha256(new URL(match[2], fixtureBase)), match[1], match[2]);
  }
});

test('release docs/package workflow share the 0.2 changelog', async () => {
  const changelog = await read('CHANGELOG.md');
  const sdk = JSON.parse(await read('packages/sdk/package.json'));
  const prepare = await read('scripts/release/prepare-sdk-package.mjs');
  const release = await read('.github/workflows/release.yml');
  assert.match(changelog, /## \[0\.2\.0\] - 2026-10-06/);
  for (const heading of ['Added','Changed','Fixed','Compatibility']) assert.match(changelog, new RegExp(`### ${heading}`));
  assert.deepEqual(sdk.files, ['dist','README.md','CHANGELOG.md','LICENSE']);
  assert.match(prepare, /CHANGELOG\.md/);
  assert.match(release, /extract-changelog-section\.mjs/);
  assert.doesNotMatch(release, /generate_release_notes=true/);
});

test('pre-tag smoke covers required Windows and macOS platform classes', async () => {
  const workflow = await read('.github/workflows/pretag-smoke.yml');
  for (const required of ['windows-2025','macos-15','macos-15-intel','vontaqfs-core','vontaqfs-runtime']) {
    assert.ok(workflow.includes(required), `missing ${required}`);
  }
});

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const sdk = await readFile(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');
const figma = await readFile(new URL('../../packages/sdk/src/figma.ts', import.meta.url), 'utf8');
const example = await readFile(new URL('../../examples/figma-plugin/src/code.ts', import.meta.url), 'utf8');
const manifest = JSON.parse(await readFile(new URL('../../examples/figma-plugin/manifest.json', import.meta.url), 'utf8'));

test('CP5 SDK owns runtime discovery, pairing bootstrap and short-lived session refresh', () => {
  assert.match(sdk, /\/v1\/health/);
  assert.match(sdk, /\/v1\/pairings\/request/);
  assert.match(sdk, /\/v1\/pairings\/poll/);
  assert.match(sdk, /\/v1\/identity\/challenge/);
  assert.match(sdk, /discoverTrustedRuntime/);
  assert.match(sdk, /\/v1\/sessions/);
  assert.match(sdk, /PAIRING_CREDENTIAL_KEY/);
  assert.match(sdk, /AUTH_INVALID/);
  assert.match(sdk, /refreshSession/);
});

test('CP5 normal API exposes default files, KV, spaces and CAS without wire chunk metadata', () => {
  assert.match(sdk, /readonly files: FileAPI/);
  assert.match(sdk, /readonly kv: KeyValueAPI/);
  assert.match(sdk, /openSpace\(options: OpenSpaceOptions\)/);
  assert.match(sdk, /ifVersion/);
  assert.match(sdk, /ifMatch/);
  assert.doesNotMatch(example, /chunk|sequence|streamId|temp path/i);
});

test('CP5 Figma helper uses plugin-local clientStorage and plugin-private document data', () => {
  assert.match(figma, /clientStorage/);
  assert.match(figma, /getPluginData/);
  assert.match(figma, /setPluginData/);
  assert.match(figma, /pluginId/);
  assert.match(figma, /widgetId/);
  assert.doesNotMatch(figma, /fileKey/);
});

test('CP5 Figma example grants only VontaqFS loopback access', () => {
  assert.deepEqual(manifest.networkAccess.allowedDomains, [
    'http://localhost:47833',
    'http://localhost:47834',
    'http://localhost:47835',
    'http://localhost:47836',
  ]);
});

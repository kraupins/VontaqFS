import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

// Source-linked regression for the actual SDK/Runtime boundary.
// The SDK's JSON.stringify envelope is syntactically valid, but a serde
// field-name mismatch previously produced a misleading HTTP 400 invalid JSON.
const protocol = fs.readFileSync(new URL('../../crates/protocol/src/lib.rs', import.meta.url), 'utf8');
const sdk = fs.readFileSync(new URL('../../packages/sdk/src/index.ts', import.meta.url), 'utf8');

const batch = protocol.split('pub enum BatchOperationWire {')[1]?.split('pub struct BatchRequest {')[0];
const wire = sdk.split('function batchOperationWire(')[1]?.split('function normalizeSpaceInfo(')[0];

test('D8 SDK camelCase JSON is accepted without changing persisted snake_case batch fingerprints', () => {
  assert.ok(batch, 'BatchOperationWire was not found');
  assert.ok(wire, 'SDK batchOperationWire was not found');
  assert.match(protocol, /#\[serde\(tag = "type", rename_all = "kebab-case"\)\]\s*pub enum BatchOperationWire/);
  for (const [field, count] of [
    ['requestId', 5], ['dataBase64', 1], ['ifMatch', 4], ['ifVersion', 1],
  ]) {
    const snake = field.replace(/[A-Z]/g, letter => `_${letter.toLowerCase()}`);
    const pattern = new RegExp(`#\\[serde\\(alias = "${field}"\\)\\]`, 'g');
    assert.equal([...batch.matchAll(pattern)].length, count, `${field} must be accepted for every variant`);
    assert.match(wire, new RegExp(`\\b${field}\\b`), `SDK must send ${field}`);
  }
  assert.match(sdk, /resume \? '\/v1\/batch\/resume' : '\/v1\/batch'/);
});

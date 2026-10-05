import test from 'node:test';
import assert from 'node:assert/strict';

const enabled = process.env.VONTAQFS_LIVE_INTEGRATION === '1';

test('CP5 live runtime health endpoint is compatible when explicitly enabled', { skip: !enabled }, async () => {
  const response = await fetch('http://localhost:47833/v1/health');
  assert.equal(response.status, 200);
  const health = await response.json();
  assert.equal(health.service, 'vontaqfs');
  assert.ok(health.protocol.min <= 1 && health.protocol.max >= 1);
});

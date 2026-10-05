import test from 'node:test';
import assert from 'node:assert/strict';
import { VONTAQ_FS_ENDPOINT, VONTAQ_FS_ENDPOINTS, VONTAQ_FS_ENDPOINT_PORTS, VONTAQ_FS_PROTOCOL_MIN, VONTAQ_FS_PROTOCOL_MAX, VontaqFSError } from '../dist/index.js';

test('SDK foundation exports locked endpoint and protocol range', () => {
  assert.equal(VONTAQ_FS_ENDPOINT, 'http://localhost:47833');
  assert.deepEqual(VONTAQ_FS_ENDPOINT_PORTS, [47833, 47834, 47835, 47836]);
  assert.deepEqual(VONTAQ_FS_ENDPOINTS, ['http://localhost:47833', 'http://localhost:47834', 'http://localhost:47835', 'http://localhost:47836']);
  assert.equal(VONTAQ_FS_PROTOCOL_MIN, 1);
  assert.equal(VONTAQ_FS_PROTOCOL_MAX, 1);
  assert.equal(new VontaqFSError('RUNTIME_UNREACHABLE', 'offline').code, 'RUNTIME_UNREACHABLE');
});

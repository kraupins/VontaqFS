import test from 'node:test';
import assert from 'node:assert/strict';
import { VONTAQ_FS_FIGMA_NETWORK_ACCESS, createVontaqFSFigmaNetworkAccess } from '../dist/figma.js';
import { VONTAQ_FS_ENDPOINTS } from '../dist/protocol.js';

test('D9 Figma manifest helper exposes the full official endpoint set without a mutable shared array', () => {
  assert.deepEqual([...VONTAQ_FS_FIGMA_NETWORK_ACCESS.allowedDomains], [...VONTAQ_FS_ENDPOINTS]);
  assert.equal(VONTAQ_FS_FIGMA_NETWORK_ACCESS.reasoning, 'Connects to the locally installed VontaqFS runtime for local persistent storage.');
  const copy = createVontaqFSFigmaNetworkAccess();
  assert.deepEqual(copy.allowedDomains, [...VONTAQ_FS_ENDPOINTS]);
  copy.allowedDomains.pop();
  assert.deepEqual([...VONTAQ_FS_FIGMA_NETWORK_ACCESS.allowedDomains], [...VONTAQ_FS_ENDPOINTS]);
});

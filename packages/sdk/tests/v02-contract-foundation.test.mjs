import test from 'node:test';
import assert from 'node:assert/strict';

import {
  VONTAQ_FS_CAPABILITY_IDS,
  VONTAQ_FS_PROTOCOL_MIN,
  VONTAQ_FS_PROTOCOL_MAX,
  VONTAQ_FS_STORAGE_FORMAT_VERSION,
  VONTAQ_FS_READ_MANY_MAX_ITEMS,
  VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES,
  VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES,
} from '../dist/protocol.js';
import { VONTAQ_FS_ERROR_CODES } from '../dist/errors.js';

test('0.2 contract foundation stays additive on protocol/storage v1', () => {
  assert.equal(VONTAQ_FS_PROTOCOL_MIN, 1);
  assert.equal(VONTAQ_FS_PROTOCOL_MAX, 1);
  assert.equal(VONTAQ_FS_STORAGE_FORMAT_VERSION, 1);
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.bulkRead, 'bulk-read');
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.spaceClear, 'space-clear');
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.destinationPickerHints, 'destination-picker-hints');
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.internalExportBookkeeping, 'internal-export-bookkeeping');
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.trackedExportPrune, 'tracked-export-prune');
  assert.equal(VONTAQ_FS_CAPABILITY_IDS.directoryContentsExport, 'directory-contents-export');
  assert.equal(VONTAQ_FS_READ_MANY_MAX_ITEMS, 64);
  assert.equal(VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES, 256 * 1024);
  assert.equal(VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES, 4 * 1024 * 1024);
});

test('0.2 stable generic error codes are exported', () => {
  for (const code of [
    'CAPABILITY_UNAVAILABLE',
    'USER_CANCELLED',
    'OPERATION_LOST',
    'DESTINATION_BUSY',
    'EXPORT_SOURCE_CHANGED',
  ]) {
    assert.ok(VONTAQ_FS_ERROR_CODES.includes(code), `${code} should be public`);
  }
});

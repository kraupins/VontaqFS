import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { fixture, checkAck } from '../../scripts/diagnostics/cp0-live-stream-baseline.mjs';

for (const size of [1, 65536, 262144, 524288, 960671]) {
  test(`CP0 deterministic ${size}-byte fixture integrity and chunk boundaries`, () => {
    const bytes = fixture(size);
    assert.equal(bytes.length, size);
    assert.deepEqual(bytes, fixture(size));
    const hash = createHash('sha256').update(bytes).digest('hex');
    assert.match(hash, /^[0-9a-f]{64}$/);
    const reconstructed = Buffer.concat(Array.from({ length: Math.ceil(size / 262144) }, (_, i) => bytes.subarray(i * 262144, (i + 1) * 262144)));
    assert.equal(createHash('sha256').update(reconstructed).digest('hex'), hash);
  });
}
test('CP0 ACK checks require exact accepted seq/byte count/stream identity', () => {
  const ack = { streamId: 'stream-1', acceptedSeq: 2, nextSeq: 3, receivedBytes: 65_540 };
  assert.equal(checkAck(ack, 'stream-1', 2, 4, 65536), true);
  for (const changed of [{ streamId: 'stream-2' }, { acceptedSeq: 3 }, { nextSeq: 2 }, { receivedBytes: 65539 }, { receivedBytes: 65540.5 }]) {
    assert.equal(checkAck({ ...ack, ...changed }, 'stream-1', 2, 4, 65536), false);
  }
});

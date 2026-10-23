import { test } from 'node:test';
import assert from 'node:assert/strict';
import { jsonFragments } from '../dist/json-encoder.js';

function encode(value) { return [...jsonFragments(value)].join(''); }
function verify(create) {
  const expected = create();
  const native = JSON.stringify(expected);
  if (native === undefined) assert.throws(() => encode(create()), TypeError);
  else {
    const fragments = [...jsonFragments(create())];
    assert.ok(fragments.every(part => part.length <= 8192), 'bounded fragments');
    assert.equal(fragments.join(''), native);
    const bytes = Buffer.concat(fragments.map(part => Buffer.from(part, 'utf8')));
    assert.deepEqual(JSON.parse(bytes.toString('utf8')), JSON.parse(native));
  }
}

test('JSON.stringify parity with holder key, omission and wrapped values', () => {
  verify(() => ({ first: { toJSON(key) { assert.equal(key, 'first'); return undefined; } }, other: 1 }));
  verify(() => ({ a: { toJSON(key) { return { key, ok: true }; } }, b: [null, undefined, , () => 1, Symbol()] }));
  verify(() => [new Number(5), new String('abc'), new Boolean(false), new Date('2024-01-01T00:00:00Z')]);
  verify(() => Object.assign(Object.create(null), { '': false, 'quote"': '\\', '🍀': '\ud800', 'pair': '😀' }));
  verify(() => ({ value: NaN, infinity: Infinity, minusZero: -0, nested: [NaN, -Infinity] }));
  verify(() => ({ a: { toJSON() { return false; } }, b: { toJSON() { return null; } } }));
  verify(() => new Date('not-a-date'));
  verify(() => ({ long: '😀'.repeat(70000) + '\ud800' + '\n'.repeat(70000) }));
});

test('getters evaluate in JSON.stringify order without dangling commas/keys', () => {
  const mk = () => {
    const value = { c: 3, b: 2 };
    Object.defineProperty(value, 'a', { enumerable: true, get() { delete value.b; return undefined; } });
    return value;
  };
  verify(mk);
  verify(() => ({ 1: 'one', 2: 'two', key: undefined }));
});

test('cycles, BigInts and unsupported roots never serialize as malformed JSON', () => {
  for (const make of [() => 2n, () => ({ x: 1n }), () => [1n], () => Symbol(), () => undefined, () => () => 1]) {
    assert.throws(() => encode(make()), TypeError);
  }
  const cyclic = {}; cyclic.me = cyclic;
  assert.throws(() => encode(cyclic), TypeError);
  assert.equal(encode([cyclic].map(() => ({ a: true }))), '[{"a":true}]');
});

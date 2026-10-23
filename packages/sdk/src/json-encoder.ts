/** Pure, incremental JSON.stringify-compatible encoder (no replacer/space argument).
 * Fragments are bounded and UTF-8-safe at their boundaries; consumers may encode
 * each fragment separately without breaking a valid surrogate pair.
 */
export function* jsonFragments(root: unknown): Generator<string> {
  const ancestors = new Set<object>();
  const holder: Record<string, unknown> = { '': root };
  const omitted = Symbol('json-omitted');

  function prepare(parent: Record<string, unknown> | unknown[], key: string): unknown | typeof omitted {
    // Property retrieval and toJSON(key) happen before deciding to emit an object key.
    let value = (parent as Record<string, unknown>)[key];
    if ((typeof value === 'object' && value !== null) || typeof value === 'function' || typeof value === 'bigint') {
      const toJSON = (value as { toJSON?: (key: string) => unknown }).toJSON;
      if (typeof toJSON === 'function') value = toJSON.call(value, key);
    }
    if (value !== null && typeof value === 'object') {
      if (value instanceof Number) value = Number(value);
      else if (value instanceof String) value = String(value);
      else if (value instanceof Boolean) value = Boolean.prototype.valueOf.call(value);
      else if (Object.prototype.toString.call(value) === '[object BigInt]') {
        value = (value as object & { valueOf(): bigint }).valueOf();
      }
    }
    return value === undefined || typeof value === 'function' || typeof value === 'symbol' ? omitted : value;
  }

  function* encode(value: unknown): Generator<string> {
    if (value === null) { yield 'null'; return; }
    switch (typeof value) {
      case 'string': yield* jsonStringFragments(value); return;
      case 'number': yield Number.isFinite(value) ? String(value) : 'null'; return;
      case 'boolean': yield value ? 'true' : 'false'; return;
      case 'bigint': throw new TypeError('Do not know how to serialize a BigInt');
      default: break;
    }
    if (typeof value !== 'object') throw new TypeError('writeJSON() requires a JSON-compatible root value.');
    if (ancestors.has(value)) throw new TypeError('Converting circular structure to JSON');
    ancestors.add(value);
    try {
      if (Array.isArray(value)) {
        yield '[';
        for (let index = 0; index < value.length; index += 1) {
          if (index) yield ',';
          const child = prepare(value, String(index));
          if (child === omitted) yield 'null';
          else yield* encode(child);
        }
        yield ']';
      } else {
        yield '{';
        let first = true;
        // Object.keys() captures the own enumerable key list before getter calls.
        for (const key of Object.keys(value)) {
          const child = prepare(value as Record<string, unknown>, key);
          if (child === omitted) continue;
          if (!first) yield ',';
          first = false;
          yield* jsonStringFragments(key);
          yield ':';
          yield* encode(child);
        }
        yield '}';
      }
    } finally {
      ancestors.delete(value);
    }
  }
  const prepared = prepare(holder, '');
  if (prepared === omitted) throw new TypeError('writeJSON() requires a JSON-compatible root value.');
  yield* encode(prepared);
}

/** Escapes one string in code-point-safe bounded pieces, including lone surrogates. */
function* jsonStringFragments(value: string): Generator<string> {
  yield '"';
  let buffer = '';
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index);
    let part: string;
    if (code === 0x22) part = '\\"';
    else if (code === 0x5c) part = '\\\\';
    else if (code === 0x08) part = '\\b';
    else if (code === 0x0c) part = '\\f';
    else if (code === 0x0a) part = '\\n';
    else if (code === 0x0d) part = '\\r';
    else if (code === 0x09) part = '\\t';
    else if (code < 0x20 || (code >= 0xd800 && code <= 0xdfff &&
      !(code <= 0xdbff && index + 1 < value.length && value.charCodeAt(index + 1) >= 0xdc00 && value.charCodeAt(index + 1) <= 0xdfff))) {
      part = `\\u${code.toString(16).padStart(4, '0')}`;
    } else if (code >= 0xd800 && code <= 0xdbff) {
      part = value.slice(index, index + 2);
      index++;
    } else part = value[index];
    if (buffer.length + part.length > 8192) { yield buffer; buffer = ''; }
    buffer += part;
  }
  if (buffer) yield buffer;
  yield '"';
}

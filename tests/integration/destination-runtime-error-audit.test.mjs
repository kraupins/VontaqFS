import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const runtime = readFileSync(new URL('../../crates/runtime/src/lib.rs', import.meta.url), 'utf8');

function section(start, end) {
  const a = runtime.indexOf(start);
  const b = runtime.indexOf(end, a + start.length);
  assert.ok(a >= 0 && b > a, `missing runtime route ${start}`);
  return runtime.slice(a, b);
}

test('destination creation logs only bounded failure stage/category while keeping public 500 generic', () => {
  const route = section('("POST", "/v1/destinations/create")', '("POST", "/v1/destinations/list")');
  for (const stage of [
    'destination-admit', 'destination-application', 'destination-previous-grant',
    'destination-picker', 'destination-cancel', 'destination-finish',
  ]) assert.ok(route.includes(`"${stage}"`), stage);
  const implementation = section('fn destination_create_error(', 'fn read_recent_logs(');
  assert.match(implementation, /self\.audit_log\(stage, &category\)/);
  assert.match(implementation, /ApiError::from_storage\(error\)/);
  assert.match(implementation, /StorageError::Sqlite\(source\)/);
  assert.match(implementation, /source\.sqlite_error\(\)/);
  assert.match(implementation, /StorageError::Io\(source\)/);
  assert.match(implementation, /StorageError::Internal\(_\)/);
  assert.doesNotMatch(implementation, /\{error:\?\}|error\.to_string\(\)|format!\([^\n]*\{error/);
  assert.match(runtime, /fn internal\(\) -> Self \{\s*Self::new\(500, ErrorCode::InternalError, "internal runtime error"\)/);
});

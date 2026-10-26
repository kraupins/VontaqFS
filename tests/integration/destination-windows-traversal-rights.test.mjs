import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const src = readFileSync(new URL('../../crates/core/src/native_dir_windows.rs', import.meta.url), 'utf8');
const section = (start, end) => {
  const begin = src.indexOf(start);
  const finish = src.indexOf(end, begin + start.length);
  assert.ok(begin >= 0 && finish > begin, `missing Windows native directory section: ${start}`);
  return src.slice(begin, finish);
};

test('native destination preflight descends using traverse/attributes, not DELETE or LIST_DIRECTORY on ancestors', () => {
  const helper = section('fn open_traversal_directory(', 'fn nt_open_with_access(');
  assert.match(helper, /nt_open_with_access\(/);
  assert.match(helper, /FILE_READ_ATTRIBUTES\s*\|\s*SYNCHRONIZE\s*\|\s*FILE_TRAVERSE/);
  assert.doesNotMatch(helper, /\bDELETE\b|\bFILE_READ_DATA\b|\bFILE_WRITE_DATA\b/);

  const opening = section('pub(crate) fn open(path: &Path)', 'pub(crate) fn identity(');
  assert.match(opening, /dir\s*=\s*open_traversal_directory\(&dir,\s*&validate\(name\)\?\)\?/);
  assert.match(opening, /id_info\(&dir\)\?/);
});

test('Windows grant preflight retains reparse rejection and root identity checks', () => {
  const opens = section('fn nt_open_with_access(', 'fn file_info(');
  assert.match(opens, /FILE_OPEN_REPARSE_POINT/);
  assert.match(opens, /FILE_ATTRIBUTE_REPARSE_POINT/);
  const verification = section('pub(crate) fn ensure_root_identity(', 'fn parent(');
  assert.match(verification, /same_id\(&current_path\.root,\s*&self\.root\)\?/);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const text = (path) => readFile(resolve(root, path), 'utf8');

test('D2 registry stores optional metadata outside file payload and journals it for crash recovery', async () => {
  const [model, storage] = await Promise.all([text('crates/core/src/model.rs'), text('crates/core/src/storage.rs')]);
  assert.match(model, /pub struct FileMetadata/);
  assert.match(model, /pub content_type: Option<String>/);
  assert.match(model, /pub format_id: Option<String>/);
  assert.match(model, /pub opaque: Option<bool>/);
  assert.match(storage, /CREATE TABLE IF NOT EXISTS file_entries\([^;]*content_type TEXT,format_id TEXT,opaque INTEGER/);
  assert.match(storage, /CREATE TABLE IF NOT EXISTS pending_mutations\([^;]*content_type TEXT,format_id TEXT,opaque INTEGER/);
  assert.match(storage, /finalize_file_write\([^)]*metadata: Option<&FileMetadata>/);
  assert.match(storage, /sha256_hex\(bytes\)/);
});

test('D2 format registry is application-owned presentation metadata only', async () => {
  const [storage, runtime, sdk] = await Promise.all([
    text('crates/core/src/storage.rs'), text('crates/runtime/src/lib.rs'), text('packages/sdk/src/index.ts'),
  ]);
  assert.match(storage, /CREATE TABLE IF NOT EXISTS application_formats\(application_id TEXT NOT NULL REFERENCES applications\(id\)/);
  assert.match(runtime, /"\/v1\/formats\/register"/);
  assert.match(runtime, /register_format\(\s*&session\.application_id/);
  assert.match(sdk, /readonly formats: FormatAPI/);
  assert.match(sdk, /\/v1\/formats\/register/);
  assert.doesNotMatch(storage, /register.*mime.*handler|decrypt.*payload|parse.*proprietary/i);
});

test('D2 Runtime write paths forward metadata without extension-based payload parsing', async () => {
  const [runtime, storage] = await Promise.all([text('crates/runtime/src/lib.rs'), text('crates/core/src/storage.rs')]);
  assert.match(runtime, /write_file_with_metadata/);
  assert.match(runtime, /commit_stream_file_with_metadata/);
  const writeCore = storage.slice(storage.indexOf('pub fn write_file_with_metadata'), storage.indexOf('pub fn create_directory_grant'));
  assert.doesNotMatch(writeCore, /match\s+.*extension\(\)/);
  assert.doesNotMatch(writeCore, /ends_with\("\.json"\)|ends_with\("\.vui"\)|ends_with\("\.zip"\)/);
});

test('D2 Desktop preview policy is fail-closed for opaque and unknown formats', async () => {
  const preview = await text('apps/desktop/src/filePreview.ts');
  assert.match(preview, /if \(file\.opaque === true\) return 'metadata-only'/);
  assert.match(preview, /contentType === 'application\/json'/);
  assert.match(preview, /SAFE_IMAGE_TYPES/);
  assert.match(preview, /return 'metadata-only'/);
  assert.doesNotMatch(preview, /JSON\.parse|FileReader|atob\(/);
});

test('D2 application format mutations use bounded idempotency receipts for mutation retries', async () => {
  const [storage, runtime] = await Promise.all([text('crates/core/src/storage.rs'), text('crates/runtime/src/lib.rs')]);
  assert.match(storage, /CREATE TABLE IF NOT EXISTS application_mutation_receipts/);
  assert.match(storage, /application_receipt\(&conn, application_id, request_id, &format!\("format-register:\{id\}"\)\)/);
  assert.match(storage, /store_application_receipt\(&conn, application_id, request_id, &format!\("format-delete:\{id\}"\)/);
  assert.match(storage, /DELETE FROM application_mutation_receipts WHERE created_at_ms < \?1/);
  assert.match(runtime, /register_format\([\s\S]*&input\.request_id/);
  assert.match(runtime, /delete_format\(&session\.application_id, &input\.id, &input\.request_id\)/);
});

test('D2 existing portable export copies arbitrary payload files without format parsing or reserialization', async () => {
  const storage = await text('crates/core/src/storage.rs');
  assert.match(storage, /zip\.add_file_cancellable\(&format!\("files\/\{\}"/);
  assert.match(storage, /checksums = scan\.files\.iter\(\)\.map\(\|item\| \(item\.path\.clone\(\), item\.etag\.clone\(\)\)\)/);
  assert.doesNotMatch(storage, /export_space_zip[\s\S]{0,9000}(JSON\.parse|serde_json::from_slice::<[^>]*File|extension\(\).*match)/);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const core=fs.readFileSync(new URL('../../crates/core/src/storage.rs',import.meta.url),'utf8');
const model=fs.readFileSync(new URL('../../crates/core/src/model.rs',import.meta.url),'utf8');
const runtime=fs.readFileSync(new URL('../../crates/runtime/src/lib.rs',import.meta.url),'utf8');
const protocol=fs.readFileSync(new URL('../../crates/protocol/src/lib.rs',import.meta.url),'utf8');
const tauri=fs.readFileSync(new URL('../../src-tauri/src/lib.rs',import.meta.url),'utf8');
const sdk=fs.readFileSync(new URL('../../packages/sdk/src/index.ts',import.meta.url),'utf8');
const sdkProtocol=fs.readFileSync(new URL('../../packages/sdk/src/protocol.ts',import.meta.url),'utf8');

test('D6 public SDK exposes native import without physical OS paths',()=>{assert.match(sdk,/async import\(options: NativeImportOptions\)/);assert.match(sdk,/\/v1\/imports\/start/);assert.match(sdk,/sourceId: options\.sourceId \?\? null/);assert.match(sdk,/targetPath/);const method=sdk.slice(sdk.indexOf('async import(options: NativeImportOptions)'),sdk.indexOf('async watch(',sdk.indexOf('async import(options: NativeImportOptions)')));assert.doesNotMatch(method,/physicalPath|absolutePath|osPath/);assert.match(method,/IMPORT_CANCELLED/)});

test('D6 HostServices owns native file and folder picker while storage core stays headless',()=>{assert.match(runtime,/pub trait RuntimeHostServices[\s\S]*choose_files[\s\S]*choose_directory[\s\S]*confirm_import_replace/);assert.match(tauri,/blocking_pick_file\(\)/);assert.match(tauri,/blocking_pick_files\(\)/);assert.match(tauri,/blocking_pick_folder\(\)/);assert.doesNotMatch(core,/tauri_plugin_dialog|AppHandle|blocking_pick_file/)});

test('D6 saved grant import requires read capability and resolves only relative safe paths',()=>{assert.match(runtime,/!grant\.capability\.can_read\(\)/);assert.match(runtime,/resolve_directory_grant_import_sources/);assert.match(core,/saved-directory source path must be relative/);assert.match(core,/saved-directory relative path contains traversal or prefix components/);assert.match(core,/saved-directory import refuses symlink\/reparse traversal/)});

test('D6 Runtime copies external data directly into atomic VFS temp/commit path',()=>{const native=core.slice(core.indexOf('pub fn native_import'),core.indexOf('pub fn export_space_zip'));assert.match(native,/prepare_stream_temp/);assert.match(native,/copy_reader_to_temp/);assert.match(native,/commit_stream_file/);assert.match(native,/ensure_write_capacity/);assert.doesNotMatch(native,/read_to_end|Vec::with_capacity\(item\.size|base64/i)});

test('D6 ZIP import rejects traversal and symlink entries and supports deflate-capable archive reader',()=>{assert.match(core,/zip::ZipArchive::new/);assert.match(core,/enclosed_name\(\)/);assert.match(core,/archive symlink entries are not allowed/);assert.match(core,/MAX_NATIVE_IMPORT_ITEMS/);const cargo=fs.readFileSync(new URL('../../crates/core/Cargo.toml',import.meta.url),'utf8');const lock=fs.readFileSync(new URL('../../Cargo.lock',import.meta.url),'utf8');assert.match(cargo,/zip = \{ version = "4\.6\.1", default-features = false, features = \["deflate"\] \}/);assert.match(lock,/name = "zip"[\s\S]*version = "4\.6\.1"[\s\S]*"flate2"/)});

test('D6 import is operation-tracked, cancellable and journaled for interrupted-work diagnosis',()=>{assert.match(runtime,/begin_application_operation\([^\n]*"native-import"/);assert.match(runtime,/update_operation_metrics\([^\n]*"importing"/);assert.match(core,/native-import-journal\.json/);assert.match(core,/native-import-interrupted-/);assert.match(core,/OperationCancelled/);assert.match(protocol,/ImportCancelled/);assert.match(sdkProtocol,/NativeImportReport/)});

test('D6 logical result descriptors report imported VFS files rather than external paths',()=>{assert.match(model,/pub struct NativeImportReport[\s\S]*imported_files: Vec<FileRecord>/);assert.match(sdkProtocol,/importedFiles: readonly FileInfo\[\]/);assert.match(model,/source_label: String/);assert.doesNotMatch(sdkProtocol,/sourcePhysicalPath|physicalPath/)});

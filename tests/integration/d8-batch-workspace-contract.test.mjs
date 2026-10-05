import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const model=fs.readFileSync(new URL('../../crates/core/src/model.rs',import.meta.url),'utf8');
const core=fs.readFileSync(new URL('../../crates/core/src/storage.rs',import.meta.url),'utf8');
const protocol=fs.readFileSync(new URL('../../crates/protocol/src/lib.rs',import.meta.url),'utf8');
const runtime=fs.readFileSync(new URL('../../crates/runtime/src/lib.rs',import.meta.url),'utf8');
const sdk=fs.readFileSync(new URL('../../packages/sdk/src/index.ts',import.meta.url),'utf8');
const sdkProtocol=fs.readFileSync(new URL('../../packages/sdk/src/protocol.ts',import.meta.url),'utf8');

function section(source,start,end){const a=source.indexOf(start);assert.notEqual(a,-1,`missing ${start}`);const b=end?source.indexOf(end,a):source.length;assert.notEqual(b,-1,`missing ${end}`);return source.slice(a,b)}

test('D8 storage category is orthogonal to retention class and old v1 data defaults to user-data',()=>{
  assert.match(model,/pub enum StorageClass[\s\S]*Persistent[\s\S]*Cache[\s\S]*Temporary/);
  assert.match(model,/pub enum StorageCategory[\s\S]*UserData[\s\S]*Generated[\s\S]*Index[\s\S]*Backup[\s\S]*Snapshot[\s\S]*Custom/);
  assert.match(model,/pub struct SpaceRecord[\s\S]*storage_class: StorageClass[\s\S]*storage_category: StorageCategory/);
  assert.match(core,/storage_category TEXT NOT NULL DEFAULT 'user-data'/);
  assert.match(core,/ensure_column\(conn, "spaces", "storage_category", "TEXT NOT NULL DEFAULT 'user-data'"\)/);
  const open=section(core,'pub fn open_space_with_category','pub fn list_spaces');
  assert.match(open,/WHERE owner_application_id=\?1 AND key=\?2 AND storage_class=\?3/);
  assert.doesNotMatch(open,/WHERE owner_application_id=\?1 AND key=\?2 AND storage_class=\?3 AND storage_category/);
  assert.match(open,/storage_category: storage_category\.unwrap_or_default\(\)/);
});

test('D8 backup and restore preserve storage category without changing payload semantics',()=>{
  const backupShape=section(core,'struct PortableBackupSpace','struct SnapshotDiskManifest');
  assert.match(backupShape,/storage_category: StorageCategory/);
  const backup=section(core,'pub fn create_portable_backup','pub fn restore_portable_backup');
  assert.match(backup,/storage_category: space\.storage_category/);
  const restore=section(core,'pub fn restore_portable_backup','pub fn create_snapshot');
  assert.match(restore,/commit_staged_space_replace\(self,[\s\S]*Some\(manifest\.space\.storage_category\)\)/);
  const commit=section(core,'fn commit_staged_space_replace','fn row_directory_grant');
  assert.match(commit,/storage_category=\?4/);
  assert.match(commit,/write_space_manifest\(&engine\.root, &desired_space, &application\)/);
  assert.match(commit,/space manifest rollback failed/);
});

test('D8 batch has fixed safety bounds, validates the whole request before mutation and reports per-item outcomes',()=>{
  assert.match(protocol,/pub const MAX_BATCH_OPERATIONS: usize = 64/);
  assert.match(protocol,/pub const MAX_BATCH_PAYLOAD_BYTES: usize = 512 \* 1024/);
  assert.match(protocol,/pub enum BatchOperationWire/);
  assert.match(protocol,/pub struct BatchItemResultWire[\s\S]*pub ok: bool[\s\S]*pub error: Option<BatchItemErrorWire>/);
  const route=section(runtime,'("POST", "/v1/batch")','("POST", "/v1/operations/status")');
  const validationEnd=route.indexOf('state.begin_application_operation');
  assert.ok(validationEnd>0,'batch operation must start after prevalidation');
  const validation=route.slice(0,validationEnd);
  assert.match(validation,/input\.operations\.is_empty\(\) \|\| input\.operations\.len\(\) > MAX_BATCH_OPERATIONS/);
  assert.match(validation,/for operation in &input\.operations/);
  assert.match(validation,/LogicalPath::parse/);
  assert.match(validation,/total_payload_bytes > MAX_BATCH_PAYLOAD_BYTES/);
  assert.match(route,/"batch"/);
  assert.match(route,/cancel\.load\(Ordering::SeqCst\)/);
  assert.match(route,/failed_items = failed_items\.saturating_add\(1\)/);
  assert.match(route,/BatchResponse \{ results, completed_items, failed_items, cancelled \}/);
});

test('D8 SDK batch/writeTree keeps high-level automatic streaming for large payloads',()=>{
  assert.match(sdkProtocol,/VONTAQ_FS_MAX_BATCH_OPERATIONS = 64/);
  assert.match(sdkProtocol,/VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES = 512 \* 1024/);
  const batch=section(sdk,'async batch(operations','async writeTree(entries');
  assert.match(batch,/\/v1\/batch/);
  assert.match(batch,/VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES/);
  const tree=section(sdk,'async writeTree(entries','async export(');
  assert.match(tree,/group\.length < VONTAQ_FS_MAX_BATCH_OPERATIONS/);
  assert.match(tree,/groupBytes \+ candidate\.bytes\.byteLength > VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES/);
  assert.match(tree,/candidate\.bytes\.byteLength > VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES/);
  assert.match(tree,/this\.files\.writeFile\(/);
  assert.match(tree,/this\.batch\(/);
});

test('D8 typed capabilities explicitly degrade absent newer Runtime features to false',()=>{
  assert.match(protocol,/"batch"\.into\(\)/);
  assert.match(protocol,/"storage-category"\.into\(\)/);
  assert.match(protocol,/"system-progress-window"\.into\(\)/);
  assert.match(sdkProtocol,/export interface VontaqFSCapabilities/);
  const caps=section(sdk,'async capabilities(): Promise<VontaqFSCapabilities>','async openSpace(');
  assert.match(caps,/capabilityMap\(this\.connection\.sessionCapabilityIds\)/);
  const map=section(sdk,'function capabilityMap','function validateWorkspaceKey');
  assert.match(map,/nativeExport: has\('native-export'\)/);
  assert.match(map,/batch: has\('batch'\)/);
  assert.match(map,/storageCategory: has\('storage-category'\)/);
  assert.match(map,/systemProgressWindow: has\('system-progress-window'\)/);
  assert.match(sdk,/storageCategory: info\.storageCategory \?\? 'user-data'/);
});

test('D8 Workspace is only an SDK convenience over ordinary spaces, not a Runtime storage engine',()=>{
  const workspace=section(sdk,'async workspace(key: string','watch(path: string');
  assert.match(workspace,/this\.openSpace\(/);
  assert.match(workspace,/workspaceSpaceKey\(key, 'persistent'\)/);
  const workspaceClass=section(sdk,'export class VontaqFSWorkspace','function validateSnapshotId');
  assert.match(workspaceClass,/this\.client\.openSpace\(/);
  assert.match(workspaceClass,/storageCategory: 'generated'/);
  assert.match(workspaceClass,/storageCategory: 'index'/);
  assert.doesNotMatch(runtime,/"\/v1\/workspace/);
  assert.doesNotMatch(core,/CREATE TABLE[^;]*workspace/i);
});

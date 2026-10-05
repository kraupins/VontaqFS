import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const text = (path) => readFile(resolve(root, path), 'utf8');

test('D3 protocol exposes one normalized operation contract and actionable errors', async () => {
  const protocol = await text('crates/protocol/src/lib.rs');
  assert.match(protocol, /pub enum OperationPresentationWire[\s\S]*Silent[\s\S]*Client[\s\S]*Vontaqfs/);
  assert.match(protocol, /pub struct OperationRequestWire[\s\S]*pub id: String[\s\S]*pub presentation: OperationPresentationWire/);
  assert.match(protocol, /OperationNotFound/);
  assert.match(protocol, /OperationNotOwned/);
  assert.match(protocol, /OperationNotCancellable/);
  assert.match(protocol, /"operations"/);
});

test('D3 Runtime operation registry is application-owned and status/cancel are authorization-scoped', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  assert.match(runtime, /owner_application_id: Option<String>/);
  assert.match(runtime, /fn operation_status_for_application\([^)]*application_id/);
  assert.match(runtime, /entry\.owner_application_id\.as_deref\(\)!=Some\(application_id\)/);
  assert.match(runtime, /OPERATION_NOT_OWNED|OperationNotOwned/);
  assert.match(runtime, /\("POST", "\/v1\/operations\/status"\)/);
  assert.match(runtime, /\("POST", "\/v1\/operations\/cancel"\)/);
  assert.match(runtime, /cancel_application_operation\(&session\.application_id/);
});

test('D3 real progress is wired to streaming bytes and server-side tree operations', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  const storage = await text('crates/core/src/storage.rs');
  assert.match(runtime, /update_operation_bytes\(/);
  assert.match(runtime, /update_operation_items\(/);
  assert.match(runtime, /"write","writing",true,input\.declared_size/);
  assert.match(runtime, /"read","reading",true,Some\(metadata\.size\)/);
  assert.match(runtime, /storage\.copy_path_with_progress/);
  assert.match(runtime, /storage\.move_path_with_progress/);
  assert.match(storage, /pub fn copy_path_with_progress/);
  assert.match(storage, /pub fn move_path_with_progress/);
  assert.match(storage, /progress\([^,]+,\s*Some\(total\)\)/);
});

test('D3 progress never invents totals and destructive delete uses a non-cancellable safety boundary', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  assert.match(runtime, /bytes_total:Option<u64>/);
  assert.match(runtime, /items_total:Option<u64>/);
  assert.match(runtime, /throughput_bytes_per_second:\s*None/);
  assert.match(runtime, /eta_ms:\s*None/);
  assert.match(runtime, /"delete","deleting",false,None,None/);
  assert.match(runtime, /delete_path_with_progress\([^;]*\|\|false/);
  assert.match(runtime, /requested,"read","reading",false,Some\(metadata\.size\),None/);
  assert.match(runtime, /requested,"write","writing",false,Some\(bytes\.len\(\) as u64\),None/);
  assert.match(runtime, /if operations\.len\(\)>=OPERATION_MAX_ROWS/);
  assert.match(runtime, /fn application_operations_are_owned_and_preserve_real_progress_through_cancel/);
  assert.match(runtime, /fn application_operation_does_not_invent_unknown_totals_and_respects_non_cancellable_boundary/);
});

test('D3 SDK keeps high-level APIs chunk-free while adding progress presentation and AbortSignal', async () => {
  const sdk = await text('packages/sdk/src/index.ts');
  const protocol = await text('packages/sdk/src/protocol.ts');
  assert.match(sdk, /export interface OperationOptions[\s\S]*presentation\?: ProgressPresentation[\s\S]*onProgress\?[\s\S]*signal\?: AbortSignal/);
  assert.match(sdk, /class OperationObserver/);
  assert.match(sdk, /onProgress \? 'client' : 'silent'/);
  assert.match(sdk, /\/v1\/operations\/status/);
  assert.match(sdk, /\/v1\/operations\/cancel/);
  assert.match(protocol, /export type ProgressPresentation = 'silent' \| 'client' \| 'vontaqfs'/);
  assert.match(protocol, /bytesCompleted\?: number/);
  assert.match(protocol, /itemsCompleted\?: number/);
  assert.doesNotMatch(sdk, /writeFile\([^)]*chunkCount|copy\([^)]*chunkCount/);
});

test('D3 Desktop continues consuming the same normalized operation shape after D4 presentation is added', async () => {
  const [api, main, tauri] = await Promise.all([
    text('apps/desktop/src/desktopApi.ts'),
    text('apps/desktop/src/main.tsx'),
    text('src-tauri/src/lib.rs'),
  ]);
  assert.match(api, /export type LongOperation[\s\S]*presentation: 'silent' \| 'client' \| 'vontaqfs'/);
  assert.match(api, /bytesCompleted: number \| null/);
  assert.match(api, /itemsCompleted: number \| null/);
  assert.match(api, /status: 'queued' \| 'running' \| 'cancelling' \| 'completed' \| 'failed' \| 'cancelled'/);
  assert.match(main, /operation\.error\?\.message/);
  assert.match(tauri, /desktop_operations/);
});

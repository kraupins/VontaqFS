import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const text = (path) => readFile(resolve(root, path), 'utf8');

test('D4 uses one dedicated operations window with delayed appearance', async () => {
  const tauri = await text('src-tauri/src/lib.rs');
  assert.match(tauri, /OPERATIONS_WINDOW_SHOW_DELAY_MS:\s*i64\s*=\s*300/);
  assert.match(tauri, /WebviewWindowBuilder::new\(app, "operations"/);
  assert.match(tauri, /fn spawn_operations_attention/);
  assert.match(tauri, /now\.saturating_sub\(row\.operation\.started_at_ms\)\s*>=\s*OPERATIONS_WINDOW_SHOW_DELAY_MS/);
  assert.doesNotMatch(tauri, /WebviewWindowBuilder::new\([^\n]*operation\.id/);
});

test('D4 Runtime exposes only explicit vontaqfs-presentation rows and derives safe labels itself', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  assert.match(runtime, /pub struct DesktopOperationSummary[\s\S]*operation: LongOperationSnapshot[\s\S]*application_name: String[\s\S]*label: String/);
  assert.match(runtime, /entry\.snapshot\.presentation != OperationPresentationWire::Vontaqfs/);
  assert.match(runtime, /fn operation_display_label\(kind: &str, application_name: &str\)/);
  assert.match(runtime, /"write" => "Saving local data"/);
  assert.match(runtime, /_ => "Working with local data"/);
  assert.doesNotMatch(runtime, /operation_display_label\([^)]*client.*text/i);
});

test('D4 failures remain present until dismissed while successful completion uses a bounded hold', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  assert.match(runtime, /OPERATION_DESKTOP_COMPLETION_HOLD_MS:\s*i64\s*=\s*1_200/);
  assert.match(runtime, /LongOperationStatus::Failed[\s\S]*entry\.snapshot\.presentation==OperationPresentationWire::Vontaqfs[\s\S]*!entry\.desktop_dismissed/);
  assert.match(runtime, /pub fn dismiss_desktop_operation/);
  assert.match(runtime, /LongOperationStatus::Completed \| LongOperationStatus::Failed \| LongOperationStatus::Cancelled/);
  assert.match(runtime, /fn vontaqfs_presented_operations_use_runtime_labels_and_require_terminal_dismissal/);
});

test('D4 compact UI reuses operation progress and never renders paths or arbitrary HTML', async () => {
  const main = await text('apps/desktop/src/main.tsx');
  const css = await text('apps/desktop/src/styles.css');
  assert.match(main, /function OperationsSurface\(/);
  assert.match(main, /function OperationsWindowRow\(/);
  assert.match(main, /row\.applicationName/);
  assert.match(main, /row\.label/);
  assert.match(main, /operation\.bytesCompleted/);
  assert.match(main, /operation\.itemsCompleted/);
  assert.match(main, /operation\.throughputBytesPerSecond/);
  assert.match(main, /operation\.etaMs/);
  assert.match(main, /operation\.status === 'cancelling'[\s\S]*Cancelling…/);
  assert.match(main, /operation\.cancellable[\s\S]*Cancel/);
  assert.match(main, /operation\.status === 'failed'[\s\S]*Open VontaqFS[\s\S]*Dismiss/);
  assert.doesNotMatch(main, /dangerouslySetInnerHTML/);
  assert.doesNotMatch(main, /operations-window-copy[\s\S]{0,800}space\.path/);
  assert.match(css, /\.operations-window-row/);
  assert.match(css, /\.progress-track/);
});

test('D4 operations UI is privileged Desktop state, not a new public admin HTTP surface', async () => {
  const runtime = await text('crates/runtime/src/lib.rs');
  const tauri = await text('src-tauri/src/lib.rs');
  const api = await text('apps/desktop/src/desktopApi.ts');
  const publicSource = runtime.split('#[cfg(test)]')[0];
  assert.doesNotMatch(publicSource, /\/v1\/operations\/desktop/);
  assert.match(tauri, /fn desktop_operations\(/);
  assert.match(tauri, /fn dismiss_desktop_operation\(/);
  assert.match(api, /desktopOperations: \(\) => invoke<DesktopOperationSummary\[]>\('desktop_operations'\)/);
  assert.match(api, /dismissDesktopOperation/);
});

#!/usr/bin/env python3
"""CP18 source-linked lifecycle invariants; does NOT compile or execute Rust."""
from pathlib import Path

root = Path(__file__).parents[1]
runtime = (root/'crates/runtime/src/lib.rs').read_text()
desktop = (root/'src-tauri/src/lib.rs').read_text()

# Session/auth paths cannot re-enter global stream journal scanning.
auth = runtime.split('    fn authenticate(&self, request:')[1].split('    fn authorize_space(')[0]
assert 'self.prune_session_ephemeral()' in auth
assert 'self.prune_ephemeral()' not in auth
assert 'resolve_stream_by_id' not in auth
assert 'storage.finish_stream_operation' not in auth

sweep = runtime.split('    fn prune_ephemeral(&self)')[1].split('    async fn acquire_bulk(')[0]
assert 'resolve_stream_by_id' in sweep
assert 'STREAM_IDLE_TIMEOUT_MS' in sweep
periodic = runtime.split('    pub async fn serve(self)')[1].split('    let mut acceptors')[0]
assert 'spawn_blocking(move ||' in periodic
assert 'sweep_state.prune_ephemeral()' in periodic
assert 'run_bounded_receipt_gc' in periodic

# Nothing marks stopped on grace timeout; worker ownership extends beyond grace.
shutdown = runtime.split('    pub async fn serve(self)')[1].split('async fn bind_official_endpoint')[0]
assert 'while !self.state.shutdown_drain_complete()' in shutdown
assert 'while !self.state.fence_streams_for_shutdown()' in shutdown
assert shutdown.index('while !self.state.fence_streams_for_shutdown()') < shutdown.index('.store(LIFECYCLE_STOPPED')
assert 'shutdown_notify.notified() => break' in shutdown
assert 'state.shutdown_notify.notified() => break' in shutdown
assert 'waiting-for-owned-workers' in shutdown

fence = runtime.split('    fn fence_streams_for_shutdown(&self)')[1].split('    fn shutdown_drain_complete(&self)')[0]
assert fence.index('resolve_stream_by_id') < fence.index('finish_stream_operation')
assert '"EXPIRED_FENCED"' in fence
assert 'abort_stream_temp' not in fence
assert 'return false' in fence

# Desktop UI singleton is advisory; Core root OS lock is authoritative.
root_lock = (root/'crates/core/src/root_lock.rs').read_text()
assert 'LOCK_EX | libc::LOCK_NB' in root_lock
assert 'options.share_mode(0)' in root_lock
assert 'StorageRootLock::acquire(&root)' in (root/'crates/core/src/storage.rs').read_text()
assert 'Core\'s independent OS-held' in desktop
assert 'record.pid == self.owner.pid' in desktop
quit_fn = desktop.split('fn graceful_exit(app: tauri::AppHandle)')[1].split('#[cfg_attr(mobile, tauri::mobile_entry_point)]')[0]
assert 'match runtime.shutdown().await' in quit_fn
assert 'Ok(()) => app.exit(0)' in quit_fn
assert 'vontaqfs-runtime-shutdown-error' in quit_fn
assert 'let _ = runtime.shutdown().await' not in quit_fn
ui = (root/'apps/desktop/src/main.tsx').read_text()
assert "listen<string>('vontaqfs-runtime-shutdown-error'" in ui
assert 'setActionError(event.payload)' in ui

print('PASS CP18 source-linked auth, background sweep, worker drain, stream fencing, Core/Desktop lock and quit guards')

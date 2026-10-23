#!/usr/bin/env python3
"""CP19 source-linked safety gates (NOT Rust/Tauri execution)."""
from pathlib import Path
import re
root=Path(__file__).parents[1]
core=(root/'crates/core/src/storage.rs').read_text()
desktop=(root/'src-tauri/src/lib.rs').read_text()
ui=(root/'apps/desktop/src/main.tsx').read_text()
repair=core.split('    pub fn repair_space<F, P>(')[1].split('    /// The grant picker',1)[0]
assert 'Claim::space_exclusive(space_id)' in repair
assert repair.index('Claim::space_exclusive(space_id)') < repair.index('create_registry_backup()')
assert repair.index('create_registry_backup()') < repair.index('scan_file_index(') < repair.index('tx.commit()?')
assert "phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED')" in repair
assert 'StorageError::RecoveryRequired' in repair
reconcile=core.split('    pub fn reconcile_space_usage<F>(')[1].split('    /// Legacy internal clear entry-point')[0]
assert 'Claim::space_exclusive(space_id)' in reconcile
assert reconcile.index('scan_file_index(') < reconcile.index('scan_matches_index(&tx') < reconcile.index('UPDATE spaces SET')
assert 'StorageError::RecoveryRequired' in reconcile
verify=core.split('    pub fn verify_space_usage<F>(')[1].split('    /// Explicit counter reconciliation')[0]
assert 'scan_matches_index(&conn' in verify
assert 'UPDATE spaces SET' not in verify
updater=desktop.split('struct DesktopUpdater {')[1].split('#[tauri::command]',1)[0]
assert 'inner: Mutex<DesktopUpdaterState>' in updater
assert 'generation: u64' in updater
assert 'UPDATE_SUPERSEDED' in updater
assert 'UPDATE_BUSY' in updater
assert 'generation != generation' in updater
assert 'updater.pending' not in desktop and 'updater.status' not in desktop
check=desktop.split('async fn perform_update_check(')[1].split('fn runtime_status_label(',1)[0]
assert 'begin_check()' in check
assert 'commit(generation' in check
assert '.pubkey(public_key)' in check and '.check().await' in check
assert 'update.restart_after_install(true)' in check
assert 'state.pending = Some(PendingDesktopUpdate { update, downloaded: None })' in check
assert 'state.pending = None' in check
download=desktop.split('async fn download_update(')[1].split('async fn install_update_and_restart(',1)[0]
assert 'state.generation == generation' in download
assert 'updater.commit(generation' in download
assert 'state.pending = Some(pending)' in download
install=desktop.split('async fn install_update_and_restart(')[1].split('fn runtime_lifecycle(',1)[0]
assert 'DesktopUpdaterPhase::Installing' in install
assert 'runtime.handle.shutdown().await' in install and 'pending.update.install(bytes)' in install
assert '.restart()' in install
assert 'updaterEpoch.current' in ui and 'requestEpoch === updaterEpoch.current' in ui
assert 'cp19_external_change_is_not_silently_adopted_by_usage_reconciliation' in core
assert 'cp19_repair_keeps_space_lease_through_scan_and_commit' in core
assert 'stale_check_cannot_restore_older_updater_state' in desktop
print('PASS CP19: Core scan/adoption/lease, updater generation, pending ownership, UI stale-poll gates (source only)')

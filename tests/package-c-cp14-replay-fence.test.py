"""Source-linked CP14 assertions. They do not substitute for Rust compilation."""
from pathlib import Path

base = Path(__file__).resolve().parents[1]
core = (base / 'crates/core/src/storage.rs').read_text()
runtime = (base / 'crates/runtime/src/lib.rs').read_text()
errors = (base / 'crates/core/src/error.rs').read_text()
protocol = (base / 'crates/protocol/src/lib.rs').read_text()


def test_stream_methods_are_top_level_siblings():
    # Regression for malformed nesting in the supplied checkpoint archive.
    begin = core.index('    pub fn commit_stream_file_with_metadata(')
    inner = core.index('    pub fn commit_stream_file_with_metadata_using_lease(')
    end = core.index('    pub fn abort_stream_temp(', inner)
    wrapper = core[begin:inner]
    assert wrapper.index('let lease = self.admit_space') < wrapper.index('self.commit_stream_file_with_metadata_using_lease')
    assert wrapper.count('    ) -> StorageResult<FileRecord> {') == 1
    assert '\n    }\n\n    /// Use the lease already held by a stream' in wrapper
    assert 'let tx = conn.transaction()?;' not in wrapper
    assert '_lease: &AdmissionLease' in core[inner:end]


def test_v3_replay_is_fail_closed_until_complete():
    assert 'durable_replay: None,' in runtime
    assert 'ErrorCode::RequestIdMigrationRequired' in runtime
    assert 'StorageError::RequestIdMigrationRequired' in core
    assert 'RequestIdMigrationRequired' in errors
    assert 'RequestIdMigrationRequired' in protocol
    assert 'fn versioned_replay_ids_require_negotiated_runtime_support()' in runtime
    assert 'fn v3_request_ids_cannot_enter_unfingerprinted_legacy_receipt_paths()' in core

if __name__ == '__main__':
    test_stream_methods_are_top_level_siblings()
    test_v3_replay_is_fail_closed_until_complete()
    print('2 CP14 source-linked checks passed (Rust not compiled)')


def test_capacity_reserved_by_all_known_durable_parents():
    # The seven-day guaranteed pool includes opaque/legacy IDs that are
    # stored in v3 receipt tables. These are static contracts, not Rust runtime
    # or capacity-exhaustion integration tests.
    sql_start = core.index('fn sql_atomic_receipt<')
    sql_end = core.index('fn sql_atomic_store<', sql_start)
    helper = core[sql_start:sql_end]
    assert helper.index('ensure_v3_result_capacity(conn, now)?') > helper.index('if request_id.starts_with("request-v3-")')
    assert helper.count('ensure_v3_result_capacity(conn, now)?') == 1
    assert 'validate_new_v3_operation_id(batch_id, now)?' in core
    batch = core[core.index('pub fn prepare_batch_items('):core.index('pub fn mark_batch_item_running(')]
    copy = core[core.index('fn prepare_composite_copy('):core.index('fn register_copy_item(')]
    native = core[core.index('fn prepare_native_import_items('):core.index('fn prior_native_import_item(')]
    # BATCH/MOVE remain open for composite child reservation; do not claim
    # completeness from their legacy single-parent preflight.
    assert 'ensure_v3_result_capacity(&tx, now)?' in batch
    assert 'ensure_v3_result_capacity_slots(&tx, now, item_slots.saturating_add(2))?' in copy
    assert 'ensure_v3_result_capacity_slots(&tx, now, child_slots.saturating_add(1))?' in native
    assert copy.index('ensure_v3_result_capacity_slots(') < copy.index('INSERT INTO durable_operations(')
    assert native.index('ensure_v3_result_capacity_slots(') < native.index('INSERT INTO durable_operations(')
    assert 'json_extract(i.result_json' in core and 'if reservations == 1' in helper

if __name__ == '__main__':
    test_capacity_reserved_by_all_known_durable_parents()
    print('CP14 capacity source checks passed')

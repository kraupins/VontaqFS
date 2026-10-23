"""CP14 source-linked checks; Rust execution remains a separate validation gate."""
from pathlib import Path

CORE = (Path(__file__).resolve().parents[1] / 'crates/core/src/storage.rs').read_text()


def test_fingerprint_is_defined_by_full_file_intent():
    start = CORE.index('fn file_write_fingerprint(')
    end = CORE.index('fn sql_identity_table(', start)
    method = CORE[start:end]
    for token in ('"file-write"', '"sha256"', '"path"', '"size"', '"ifMatch"', '"contentType"', '"formatId"', '"opaque"'):
        assert token in method, token


def test_both_file_write_paths_record_pending_identity_before_swap():
    sections = [
        CORE[CORE.index('pub fn commit_stream_file_with_metadata_using_lease('):CORE.index('pub fn abort_stream_temp(')],
        CORE[CORE.index('fn write_file_fault_with_metadata('):CORE.index('fn space_data_root(', CORE.index('fn write_file_fault_with_metadata('))],
    ]
    for method in sections:
        assert 'sql_atomic_receipt::<FileRecord>' in method
        assert 'file_write_fingerprint(' in method
        assert 'request_fingerprint) VALUES(' in method
        assert 'ensure_v3_result_capacity' not in method or 'sql_atomic_receipt' in method
        assert method.index('INSERT INTO pending_mutations') < method.index('atomic_replace(')


def test_finalization_preserves_legacy_and_v3_identity():
    start = CORE.index('fn finalize_file_write(')
    end = CORE.index('fn file_row(', start)
    finalize = CORE[start:end]
    assert finalize.index('SELECT request_fingerprint FROM pending_mutations') < finalize.index('DELETE FROM pending_mutations')
    assert 'sql_atomic_store(' in finalize
    assert 'store_receipt(' in finalize
    assert 'if let Some(fingerprint) = pending_fingerprint.as_deref()' in finalize
    for rust_test in ('fn file_write_receipt_binds_bytes_preconditions_and_metadata_across_restart()',
                      'fn pending_file_write_keeps_fingerprint_during_recovery()'):
        assert rust_test in CORE


if __name__ == '__main__':
    for test in (test_fingerprint_is_defined_by_full_file_intent,
                 test_both_file_write_paths_record_pending_identity_before_swap,
                 test_finalization_preserves_legacy_and_v3_identity):
        test()
    print('CP14 file identity: 3 source-linked assertions PASS (Rust not run)')

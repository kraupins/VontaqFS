#!/usr/bin/env python3
"""Finite Architect G5/G6/G7/G8 source-linked checks and SQLite model.
NOT a Rust compile, OS stream test or production native proof.
"""
from pathlib import Path
import re, sqlite3
root=Path(__file__).resolve().parents[1]
rt=(root/'crates/runtime/src/lib.rs').read_text()
core=(root/'crates/core/src/storage.rs').read_text()
cargo=(root/'crates/runtime/Cargo.toml').read_text()

# G8: ONE OFF-by-default predicate drives BOTH capability and runtime validators.
predicate=rt.split('fn v3_admission_enabled() -> bool {',1)[1].split('}',1)[0]
assert 'all(test, feature = "v3-native-validation")' in predicate
assert 'v3-native-validation = []' in cargo
assert 'durable_replay: negotiated_durable_replay()' in rt
negotiation=rt.split('fn negotiated_durable_replay()',1)[1].split('fn validate_v3_id_shape(',1)[0]
assert '!v3_admission_enabled()' in negotiation and 'identity_version: 3' in negotiation
for method,next_method in [('fn validate_request_id(', 'fn validate_operation_id('),('fn validate_operation_id(','fn validate_batch_file_metadata(')]:
    section=rt.split(method,1)[1].split(next_method,1)[0]
    assert '!v3_admission_enabled()' in section
    assert 'validate_v3_id_shape(' in section
assert 'cfg!(all(test, feature = "v3-native-validation"))' in rt

# G6: before Core admission, NEVER acknowledge cancelling from memory alone.
cancel=rt.split('fn cancel_application_operation(',1)[1].split('fn operation_cancelled(',1)[0]
assert 'let fingerprint = fingerprint.ok_or_else(' in cancel
assert 'ErrorCode::OperationNotFound' in cancel
assert cancel.index('let fingerprint = fingerprint.ok_or_else(') < cancel.index('request_application_long_job_cancel(') < cancel.index('entry.cancel.store(true,')
assert 'request_application_long_job_cancel' in core

# G7: source admission checks versioned ID timestamp ONLY after preceding lookup.
for start,end,prefix,var in [
    ('fn prepare_fs_intent(', 'fn transition_fs_intent(', 'request', 'request_id'),
    ('fn prepare_composite_copy(', 'fn register_copy_item(', 'request','request_id'),
    ('fn prepare_composite_move(', 'fn composite_move_child(', 'request','request_id'),
    ('fn prepare_native_import_items(', 'fn prior_native_import_item(', 'operation','request_id'),
    ('fn clear_disposable_space_inner(', 'pub fn clear_cache_space(', 'request','request'),
    ('pub fn open_space_with_request(', 'pub fn list_spaces(', 'request','request_id'),
]:
    section=core.split(start,1)[1].split(end,1)[0]
    assert f'validate_new_v3_{prefix}_id({var}, now)?' in section, start
    assert 'ensure_v3_result_capacity' in section, start
assert 'fn validate_new_v3_id(value: &str, prefix: &str, now: i64)' in core
assert 'started_ms < now.saturating_sub(DURABLE_SQL_RECEIPT_TTL_MS)' in core

# G5: stream committed v3 receipt must not go through legacy receipt helper.
resolver=core.split('pub fn resolve_stream_operation(',1)[1].split('pub fn stream_operation_by_id(',1)[0]
assert 'confirmed_stream_file_receipt(&conn, space_id, request_id, &path)' in resolver
helper=core.split('fn confirmed_stream_file_receipt(',1)[1].split('fn receipt<T:',1)[0]
for marker in ['operation,identity_version,request_fingerprint,expires_at_ms,result_json',
               'if stored_op != operation', 'version < 3', 'expiry <= now_ms()']:
    assert marker in helper
begin=core.split('pub fn begin_stream_operation(',1)[1].split('pub fn advance_stream_chunk(',1)[0]
assert 'validate_new_v3_request_id(&op.request_id, now_ms())?' in begin
assert 'WHERE space_id=?1 AND request_id=?2' in begin
assert 'StorageError::RequestIdConflict' in begin
chunk=rt.split('async fn handle_write_stream_route(',1)[1].split('async fn handle_read_stream_route(',1)[0]
assert 'seq.checked_add(1) == Some(stream.next_seq)' in chunk
assert 'stream.last_chunk_sha256.as_deref() == Some(chunk_hash.as_str())' in chunk
assert 'if seq != stream.next_seq' in chunk

# Synthetic minimal SQL model: read-only terminal retrieval rejects missing
# proof, changed operation, and expired result. This is NOT a native test.
c=sqlite3.connect(':memory:')
c.executescript('''
CREATE TABLE receipts(space_id TEXT,request_id TEXT,operation TEXT,identity_version INTEGER,
 request_fingerprint TEXT,expires_at_ms INTEGER,result_json TEXT,PRIMARY KEY(space_id,request_id));
INSERT INTO receipts VALUES('s','v3','file-write:/target',3,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',800,'{"version":1}');
''')
def lookup(space, id, op, now):
    r=c.execute('SELECT operation,identity_version,request_fingerprint,expires_at_ms,result_json FROM receipts WHERE space_id=? AND request_id=?',(space,id)).fetchone()
    if r is None: return None
    if r[0]!=op or r[1]<3 or not r[2] or len(r[2])!=64: return 'RECOVERY_REQUIRED'
    if r[3]<=now: return 'RESULT_EXPIRED'
    return r[4]
assert lookup('s','v3','file-write:/target',700)=='{"version":1}'
assert lookup('other','v3','file-write:/target',700) is None
assert lookup('s','v3','file-write:/changed',700)=='RECOVERY_REQUIRED'
assert lookup('s','v3','file-write:/target',800)=='RESULT_EXPIRED'
print('PASS finite Unit 2 G5/G6/G7/G8 source checks + synthetic SQL status model (Rust NOT RUN)')

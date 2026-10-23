"""Synthetic SQLite and source wiring checks; does not execute Rust code."""
from pathlib import Path
import json
import sqlite3
import hashlib

project = Path(__file__).resolve().parents[1]
core = (project / 'crates/core/src/storage.rs').read_text()
runtime = (project / 'crates/runtime/src/lib.rs').read_text()
for token in (
    'pub fn delete_space_with_request(',
    'pub fn delete_application_data_with_request(',
    'owner_deletion_replay::<(u64, u64)>(',
    'owner_deletion_replay::<(u64, u64, u64)>(',
    'pub fn desktop_owner_deletion_status(',
    'owner_deletion_fingerprint(kind, scope_id)',
    'self.commit_owner_deletion(&tx,"space",space_id,&request_id,&result)?;',
    'self.commit_owner_deletion(&tx,"application",application_id,&request_id,&result)?;',
):
    assert token in core, token
assert 'storage.delete_space_with_request(&work_space_id, &durable_request_id)' in runtime
assert 'storage.delete_application_data_with_request(&work_id, &durable_request_id)' in runtime
assert 'let durable_request_id = operation_id.clone();' in runtime
assert 'let durable_request_id = id.clone();' in runtime
assert 'self.state.storage.desktop_owner_deletion_status(operation_id)?' in runtime
assert 'LongOperationStatus::Reconciling' in runtime

# Same-key replay is decided by the persisted identity before resource rows
# are looked up. A changed kind/payload is a conflict; ADMITTED is uncertain.
db = sqlite3.connect(':memory:')
db.executescript('''
CREATE TABLE durable_operations (
 scope_kind TEXT NOT NULL, scope_id TEXT NOT NULL, request_id TEXT NOT NULL,
 kind TEXT NOT NULL, request_fingerprint TEXT NOT NULL, phase TEXT NOT NULL,
 result_json TEXT, expires_at_ms INTEGER,
 PRIMARY KEY(scope_kind,scope_id,request_id));
''')
space = 'deleted-space-uuid'
request = 'stable-operation-identity'
def fp(kind, target):
    return hashlib.sha256(json.dumps({'kind': kind, 'payload': {'targetId': target}}, sort_keys=True).encode()).hexdigest()
fingerprint = fp('SPACE_DELETE', space)
db.execute('INSERT INTO durable_operations VALUES (?,?,?,?,?,?,?,?)',
           ('space', space, request, 'SPACE_DELETE', fingerprint, 'ADMITTED', None, None))
def replay(kind, digest, clock=100):
    row = db.execute('SELECT kind,request_fingerprint,phase,result_json,expires_at_ms FROM durable_operations WHERE scope_kind=? AND scope_id=? AND request_id=?',
                     ('space',space,request)).fetchone()
    if row is None: return 'not-found'
    stored_kind, stored_fp, phase, result, expires = row
    if stored_kind != kind or stored_fp != digest: return 'REQUEST_ID_CONFLICT'
    if phase != 'COMMITTED': return 'RECOVERY_REQUIRED'
    if expires is None or expires <= clock: return 'RESULT_EXPIRED'
    return json.loads(result)
assert replay('SPACE_DELETE', fingerprint) == 'RECOVERY_REQUIRED'
assert replay('APP_DELETE', fingerprint) == 'REQUEST_ID_CONFLICT'
assert replay('SPACE_DELETE', fp('SPACE_DELETE', 'another-target')) == 'REQUEST_ID_CONFLICT'
db.execute("UPDATE durable_operations SET phase='COMMITTED',result_json=?,expires_at_ms=?", ('[4,1]', 200))
assert replay('SPACE_DELETE', fingerprint) == [4,1]
assert replay('SPACE_DELETE', fingerprint, 201) == 'RESULT_EXPIRED'
print('owner deletion stable identity, persisted replay, uncertain and expiry SQL synthetic checks: PASS')

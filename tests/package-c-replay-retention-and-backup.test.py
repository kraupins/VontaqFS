#!/usr/bin/env python3
"""CP14 SQL-v3 GC/capacity and CP15 backup source-code contract smoke.

These tests execute the production v3 DDL on a synthetic schema, but do NOT
compile Rust or certify Windows/race/crash behavior.
"""
from pathlib import Path
import sqlite3
import time

root = Path(__file__).resolve().parents[1]
storage = (root/'crates/core/src/storage.rs').read_text()
runtime = (root/'crates/runtime/src/lib.rs').read_text()
ddl = (root/'crates/core/src/registry_v3.sql').read_text()

assert 'validate_new_v3_request_id(request_id, now)?' in storage
assert 'validate_new_v3_operation_id(operation_id, now)?' in storage
assert 'ensure_v3_result_capacity(&tx, now)?' in storage
assert 'ensure_v3_result_capacity(conn, now)?' in storage
assert 'V3_PROTECTED_RECEIPT_CAPACITY' in storage
assert 'V3_GC_BATCH_ROWS' in storage
assert 'prune_expired_v3_results()?' in storage
assert 'durable_replay: None' in runtime, 'handshake must not falsely advertise unsupported mutation classes'

backup = storage.split('pub fn create_portable_backup',1)[1].split('pub fn restore_portable_backup',1)[0]
assert backup.index('NativeExportCapture::create') < backup.index('drop(capture_lease)')
assert backup.index('drop(capture_lease)') < backup.index('SimpleZipWriter::new')
assert 'let data_root = captured_sources.root.as_path()' in backup
assert 'self.file_records_under(space_id, "/")? != files' in backup
assert 'self.list_kv_entries(space_id)? != kv' in backup

db = sqlite3.connect(':memory:')
db.executescript('''
PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON;
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY,owner_application_id TEXT);
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
CREATE TABLE stream_operations(id TEXT,space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT,physical_path TEXT,capability TEXT);
INSERT INTO applications VALUES('app');
INSERT INTO spaces VALUES('space','app');
''')
db.executescript(ddl)
now = int(time.time() * 1000)
future = now + 7 * 86400000
expired = now - 1000

def parent(request, phase, expires_at):
    terminal_at = now - 8 * 86400000 if expires_at is not None else None
    db.execute('''INSERT INTO durable_operations(
        scope_kind,scope_id,request_id,kind,owner_application_id,identity_version,
        request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms,
        terminal_at_ms,expires_at_ms)
        VALUES('space','space',?,'COPY','app',3,'hash',?,?,?,?,?,?)''',
        (request,phase,now,now,now,terminal_at,expires_at))

parent('unexpired','COMMITTED',future)
parent('expired','COMMITTED',expired)
parent('unfinished','RECONCILING',None)
parent('expired-child','COMMITTED',expired)
db.execute("INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,updated_at_ms) VALUES('space','space','expired-child',0,'f','RUNNING',?)",(now,))
parent('expired-intent','COMMITTED',expired)
db.execute("INSERT INTO filesystem_intents(intent_id,scope_kind,scope_id,request_id,intent_kind,state,payload_json,updated_at_ms) VALUES('i','space','space','expired-intent','MOVE','DB_COMMITTED','{}',?)",(now,))
for request in ('unexpired','unfinished','expired-child','expired-intent'):
    try: db.execute('DELETE FROM durable_operations WHERE request_id=?',(request,))
    except sqlite3.IntegrityError: pass
    else: raise AssertionError(f'Protected parent could be deleted: {request}')
db.execute("DELETE FROM durable_operations WHERE request_id='expired'")
assert db.execute("SELECT COUNT(*) FROM durable_operations WHERE request_id='expired'").fetchone()[0] == 0

db.execute("INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES('space','recent','kv','{}',?,3,'hash',?,?)",(now,now,future))
try: db.execute("DELETE FROM mutation_receipts WHERE request_id='recent'")
except sqlite3.IntegrityError: pass
else: raise AssertionError('Unexpired v3 SQL receipt could be deleted')

db.execute("INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES('space','old','kv','{}',?,3,'hash',?,?)",(now,now,expired))
db.execute("DELETE FROM mutation_receipts WHERE identity_version>=3 AND expires_at_ms<=? LIMIT 128",(now,))
assert db.execute("SELECT request_id FROM mutation_receipts").fetchall() == [('recent',)]
assert db.execute('PRAGMA foreign_key_check').fetchall() == []
print('PASS: CP14 SQLite v3 protected/expired rows, SQL receipt TTL; CP15 staged backup source contract (Rust not executed)')

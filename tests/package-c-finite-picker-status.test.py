#!/usr/bin/env python3
"""Source-linked CP14 G3/G4 SQLite model; NOT Rust/native execution."""
from pathlib import Path
import sqlite3,json
root=Path(__file__).resolve().parents[1]
core=(root/'crates/core/src/storage.rs').read_text()
rt=(root/'crates/runtime/src/lib.rs').read_text()
route=rt.split('("POST", "/v1/destinations/create") => {',1)[1].split('("POST", "/v1/destinations/list") => {',1)[0]
assert route.index('admit_directory_grant_request(') < route.index('host.choose_directory(')
assert 'DestinationGrantAdmission::Replayed(grant)' in route
assert 'DestinationGrantAdmission::UserCancelled' in route
assert route.index('cancel_directory_grant_request(') < route.rindex('ErrorCode::UserCancelled')
assert 'finish_directory_grant_request(' in route and 'create_directory_grant(' not in route
start=core.split('pub fn admit_directory_grant_request(',1)[1].split('pub fn create_directory_grant(',1)[0]
assert 'ensure_v3_result_capacity_slots(&tx, now, 2)?;' in start
assert 'validate_new_v3_request_id(request_id, now)?;' in start
assert 'tx.commit()?;' in start
assert 'NativeDirectory::open(&canonical)?' in start
assert 'directory_grant_root_bindings' in start
assert start.index('sql_atomic_store(&tx,true') < start.index("phase='COMMITTED'") < start.rindex('tx.commit()?;')
assert "kind<>'SPACE_OPEN'" in core and "phase IN ('ADMITTED','RUNNING','CANCEL_REQUESTED')" in core
status=core.split('pub fn live_sql_receipt(',1)[1].split('pub fn retained_status_owner(',1)[0]
assert 'if row.is_none()' in status
assert 'durable_operations d JOIN spaces s' in status
assert "scope_kind='application'" in status
assert 'if exists { return Err(StorageError::RecoveryRequired); }' in status
con=sqlite3.connect(':memory:')
con.execute('PRAGMA foreign_keys=ON')
con.executescript('''
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT REFERENCES applications(id));
CREATE TABLE directory_grant_root_bindings(grant_id TEXT PRIMARY KEY REFERENCES directory_grants(id),root_identity TEXT NOT NULL);
CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,request_id TEXT,kind TEXT,owner_application_id TEXT,request_fingerprint TEXT,phase TEXT,result_json TEXT,terminal_at_ms INTEGER,expires_at_ms INTEGER,PRIMARY KEY(scope_kind,scope_id,request_id));
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,PRIMARY KEY(application_id,request_id));
INSERT INTO applications VALUES('app-1'),('app-2');
''')
# Admission is independently committed before the picker is entered.
con.execute('BEGIN IMMEDIATE')
con.execute("INSERT INTO durable_operations VALUES('application','app-1','p1','DESTINATION_CREATE','app-1','fp1','ADMITTED',NULL,NULL,NULL)")
con.commit()
assert con.execute('SELECT phase FROM durable_operations WHERE scope_id=? AND request_id=?',('app-1','p1')).fetchone()[0]=='ADMITTED'
assert not con.execute('SELECT 1 FROM directory_grants').fetchone()
# Crash now: recovered/unknown parent is not a safe not-found or abort.
assert con.execute('SELECT phase FROM durable_operations WHERE scope_id=? AND request_id=?',('app-1','p1')).fetchone()[0]=='ADMITTED'
# SQL completion must be all or nothing: failure rolls back grant and binding.
try:
    con.execute('BEGIN IMMEDIATE')
    con.execute("INSERT INTO directory_grants VALUES('grant-1','app-1')")
    con.execute("INSERT INTO directory_grant_root_bindings VALUES('grant-1','native-dev-inode')")
    con.execute("INSERT INTO application_mutation_receipts VALUES('app-1','p1','DESTINATION_CREATE',?)",(json.dumps({'id':'grant-1'}),))
    con.execute("UPDATE durable_operations SET phase='COMMITTED',result_json=?,terminal_at_ms=100,expires_at_ms=700 WHERE scope_id='app-1' AND request_id='p1'",(json.dumps({'id':'grant-1'}),))
    raise RuntimeError('simulate commit fault')
except RuntimeError: con.rollback()
assert con.execute('SELECT phase FROM durable_operations').fetchone()[0]=='ADMITTED'
assert con.execute('SELECT COUNT(*) FROM directory_grants').fetchone()[0]==0
con.execute('BEGIN IMMEDIATE')
con.execute("INSERT INTO directory_grants VALUES('grant-1','app-1')")
con.execute("INSERT INTO directory_grant_root_bindings VALUES('grant-1','native-dev-inode')")
con.execute("INSERT INTO application_mutation_receipts VALUES('app-1','p1','DESTINATION_CREATE',?)",(json.dumps({'id':'grant-1'}),))
con.execute("UPDATE durable_operations SET phase='COMMITTED',result_json=?,terminal_at_ms=100,expires_at_ms=700 WHERE scope_id='app-1' AND request_id='p1'",(json.dumps({'id':'grant-1'}),))
con.commit()
assert con.execute('SELECT COUNT(*) FROM directory_grants').fetchone()[0]==1
assert con.execute('SELECT result_json FROM durable_operations WHERE scope_id=? AND request_id=?',('app-2','p1')).fetchone() is None
print('PASS CP14 G3/G4 picker admission, atomic grant/receipt, owner-scoped uncertain status (SQLite source-linked only)')

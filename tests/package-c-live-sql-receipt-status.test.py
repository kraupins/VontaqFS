"""Executable SQLite ownership and source-linked contract tests. NOT native Rust tests."""
from pathlib import Path
import sqlite3

root = Path(__file__).resolve().parents[1]
core = (root / 'crates/core/src/storage.rs').read_text()
runtime = (root / 'crates/runtime/src/lib.rs').read_text()
sdk = (root / 'packages/sdk/src/index.ts').read_text()
assert 'pub fn live_sql_receipt(' in core
assert 'JOIN spaces s ON s.id=r.space_id' in core
assert 's.owner_application_id=?1' in core
assert 'JOIN applications a ON a.id=r.application_id' in core
assert 'sql_receipt_status_outcome(scope_kind,scope_id,request_id,row)' in core
assert 'let session = state.authenticate(&request)?;' in runtime
assert 'state.storage.live_sql_receipt(&session.application_id,kind,&scope.id,&input.operation_id)' in runtime
assert 'state.storage.retained_sql_receipt(&owner,kind,&scope.id,&input.operation_id)' in runtime
assert "receiptStatus<T = unknown>(scope: RetainedReceiptScope, requestId: string)" in sdk

conn=sqlite3.connect(':memory:')
conn.executescript('''
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY, owner_application_id TEXT NOT NULL);
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,identity_version INTEGER,request_fingerprint TEXT,created_at_ms INTEGER,terminal_at_ms INTEGER,expires_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,identity_version INTEGER,request_fingerprint TEXT,created_at_ms INTEGER,terminal_at_ms INTEGER,expires_at_ms INTEGER);
INSERT INTO applications VALUES ('a'),('b');
INSERT INTO spaces VALUES ('s-a','a'),('s-b','b');
INSERT INTO mutation_receipts VALUES ('s-a','r','kv-set','{"version":1}',3,'hash',1,2,9999999999999);
INSERT INTO application_mutation_receipts VALUES ('a','q','preset','{"id":"p"}',3,'hash',1,2,9999999999999);
''')
space_sql='''SELECT r.operation,r.result_json,r.identity_version,r.request_fingerprint,r.created_at_ms,r.terminal_at_ms,r.expires_at_ms
   FROM mutation_receipts r JOIN spaces s ON s.id=r.space_id
   WHERE s.owner_application_id=? AND r.space_id=? AND r.request_id=?'''
app_sql='''SELECT r.operation,r.result_json,r.identity_version,r.request_fingerprint,r.created_at_ms,r.terminal_at_ms,r.expires_at_ms
   FROM application_mutation_receipts r JOIN applications a ON a.id=r.application_id
   WHERE a.id=? AND r.application_id=? AND r.request_id=?'''
assert conn.execute(space_sql,('a','s-a','r')).fetchone()[0]=='kv-set'
assert conn.execute(space_sql,('b','s-a','r')).fetchone() is None
assert conn.execute(space_sql,('a','s-b','r')).fetchone() is None
assert conn.execute(app_sql,('a','a','q')).fetchone()[0]=='preset'
assert conn.execute(app_sql,('b','a','q')).fetchone() is None
conn.execute('DELETE FROM spaces WHERE id=?',('s-a',))
assert conn.execute(space_sql,('a','s-a','r')).fetchone() is None
print('live SQL receipt owner join, application scope, deleted-space fence and source paths: PASS')

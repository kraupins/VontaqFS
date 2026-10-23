#!/usr/bin/env python3
"""CP14 startup SPACE_OPEN recovery: source wiring and synthetic SQL transitions.

The SQLite simulation checks migration-compatible SQL statements and transaction
visibility; it does NOT run Rust, filesystem crash injection, or Windows tests.
"""
import sqlite3
from pathlib import Path

root = Path(__file__).resolve().parents[1]
source = (root / 'crates/core/src/storage.rs').read_text()
start = source.split('fn reconcile_interrupted_space_opens(', 1)[1].split('fn space_open_manifest_matches(', 1)[0]
manifest = source.split('fn space_open_manifest_matches(', 1)[1].split('fn finish_unapplied_space_open(', 1)[0]
admission = source.split('pub fn open_space_with_request(', 1)[1].split('pub fn list_spaces(', 1)[0]
long_jobs = source.split('fn reconcile_interrupted_long_jobs(', 1)[1].split('pub fn prepare_batch_items(', 1)[0]
startup = source.split('pub fn open(root:', 1)[1].split('/// Durable application-owned long-job identity.', 1)[0]
assert startup.index('recover_filesystem_intents()?') < startup.index('recover_initializing_spaces()?') < startup.index('reconcile_interrupted_space_opens()?')
assert "kind<>'SPACE_OPEN'" in long_jobs
assert 'result_json,request_started_at_ms FROM durable_operations' in start
assert "kind='SPACE_OPEN' AND phase='ADMITTED'" in start
assert 'SpaceOpenRecoveryPlan' in start and 'semantic_fingerprint("space-open-existing"' in start
assert 'space_open_manifest_matches(&space)' in start
assert 'space.created_at_ms >= admitted_at' in start
assert 'sql_atomic_store(&tx,true,&owner,&request' in start
assert 'finish_unapplied_space_open' in start and 'finish_unresolved_space_open' in start
assert 'is_link_like(&meta)' in manifest and 'SpaceManifest' in manifest
assert 'manifest.application_kind == application.kind' in manifest
assert 'manifest.application_display_name == application.display_name' not in manifest  # Mutable app label
assert 'fs::read_dir(' in start
assert 'let plan = SpaceOpenRecoveryPlan {' in admission
assert 'result_json,request_started_at_ms' in admission
assert admission.index('let plan = SpaceOpenRecoveryPlan') < admission.index('INSERT INTO durable_operations') < admission.index('open_space_with_category_context(')
assert 'SPACE_OPEN_ABORTED' in admission
for name in ['cp14_interrupted_space_open_resolves_verified_manifest_without_reapplying',
             'cp14_interrupted_space_open_before_first_effect_aborts_without_new_space',
             'cp14_interrupted_space_open_with_changed_manifest_requires_repair']:
    assert f'fn {name}(' in source

# In-memory SQLite drives the exact terminal/receipt SQL shape with parent
# evidence retained on rollback; results cannot be half-committed.
db=sqlite3.connect(':memory:')
db.executescript('''
CREATE TABLE durable_operations(scope_kind TEXT, scope_id TEXT, request_id TEXT,kind TEXT,
 request_fingerprint TEXT,phase TEXT,result_json TEXT,error_code TEXT,terminal_at_ms INTEGER,
 expires_at_ms INTEGER,updated_at_ms INTEGER,PRIMARY KEY(scope_kind,scope_id,request_id));
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,
 operation TEXT,result_json TEXT,created_at_ms INTEGER,identity_version INTEGER,
 request_fingerprint TEXT,terminal_at_ms INTEGER,expires_at_ms INTEGER,
 PRIMARY KEY(application_id,request_id));
INSERT INTO durable_operations VALUES('application','owner','open-1','SPACE_OPEN',
 'abcdef','ADMITTED','{"recoveryPlan":true}',NULL,NULL,NULL,1000);
''')
terminal_update='''UPDATE durable_operations SET phase='COMMITTED',result_json=?1,
 terminal_at_ms=?2,expires_at_ms=?3,updated_at_ms=?2 WHERE scope_kind='application'
 AND scope_id=?4 AND request_id=?5 AND kind='SPACE_OPEN' AND request_fingerprint=?6 AND phase='ADMITTED' '''
receipt_insert='''INSERT INTO application_mutation_receipts(application_id,request_id,
 operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms)
 VALUES(?,?,?, ?,?,3,?,?,?)'''
db.execute('BEGIN IMMEDIATE')
assert db.execute(terminal_update,('{"id":"space-1"}',2000,2000+604800000,'owner','open-1','abcdef')).rowcount==1
db.execute(receipt_insert,('owner','open-1','space-open-existing','{"id":"space-1"}',2000,'abcdef',2000,2000+604800000))
db.rollback()
assert db.execute('SELECT phase FROM durable_operations').fetchone()[0]=='ADMITTED'
assert db.execute('SELECT count(*) FROM application_mutation_receipts').fetchone()[0]==0
db.execute('BEGIN IMMEDIATE')
assert db.execute(terminal_update,('{"id":"space-1"}',2000,2000+604800000,'owner','open-1','abcdef')).rowcount==1
db.execute(receipt_insert,('owner','open-1','space-open-existing','{"id":"space-1"}',2000,'abcdef',2000,2000+604800000))
db.commit()
assert db.execute('SELECT count(*) FROM application_mutation_receipts').fetchone()[0]==1
assert db.execute(terminal_update,('{"id":"space-2"}',3000,3000+604800000,'owner','open-1','abcdef')).rowcount==0
# Startup ABORTED is terminal with no replay receipt; NEEDS_REPAIR is
# nonterminal and remains unexpired until explicit repair.
db.execute("INSERT INTO durable_operations VALUES('application','owner','open-2','SPACE_OPEN','xyz','ADMITTED','{}',NULL,NULL,NULL,1000)")
db.execute("UPDATE durable_operations SET phase='ABORTED',result_json=NULL,error_code='SPACE_OPEN_NOT_APPLIED',terminal_at_ms=2000,expires_at_ms=604802000 WHERE request_id='open-2'")
assert db.execute("SELECT phase,error_code FROM durable_operations WHERE request_id='open-2'").fetchone()==('ABORTED','SPACE_OPEN_NOT_APPLIED')
db.execute("INSERT INTO durable_operations VALUES('application','owner','open-3','SPACE_OPEN','xyz','ADMITTED','{}',NULL,NULL,NULL,1000)")
db.execute("UPDATE durable_operations SET phase='NEEDS_REPAIR',error_code='SPACE_OPEN_RECOVERY_UNPROVEN' WHERE request_id='open-3'")
assert db.execute("SELECT terminal_at_ms,phase FROM durable_operations WHERE request_id='open-3'").fetchone()==(None,'NEEDS_REPAIR')
print('PASS: SPACE_OPEN restart recovery source-contract + synthetic SQL atomicity (not native execution)')

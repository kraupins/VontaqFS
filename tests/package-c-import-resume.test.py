#!/usr/bin/env python3
"""CP12/13 source-linked contract + SQLite admission simulation.
NOT a substitute for Rust compilation, native filesystem or crash injection.
"""
from pathlib import Path
import sqlite3, json, time
root=Path(__file__).resolve().parents[1]
core=(root/'crates/core/src/storage.rs').read_text()
rt=(root/'crates/runtime/src/lib.rs').read_text()
sdk=(root/'packages/sdk/src/index.ts').read_text()
assert 'resume_application_native_import' in core
assert "phase='RECONCILING'" in core.split('pub fn resume_application_native_import(',1)[1].split('/// Retrieve only',1)[0]
assert 'if !allow_resume || phase != "RUNNING"' in core
assert 'self.reconcile_native_import_receipts()?' in core
assert '"PENDING" | "COMMITTED" | "SKIPPED"' in core
assert 'fn prior_native_import_item(' in core
assert 'let Some(previous) = self.prior_native_import_item' in core
assert 'previous.get("requestId")' in core
assert 'previous.get("targetRoot")' in core
assert '"/v1/imports/resume"' in rt and 'claim_native_import_resume_worker' in rt
assert 'async resumeImport(options: NativeImportResumeOptions)' in sdk
protocol=(root/'crates/protocol/src/lib.rs').read_text()
assert 'pub durable_replay: Option<DurableReplayNegotiation>' in protocol
assert 'durable_replay: None' in rt  # until every public mutation has the seven-day guarantee
assert "resume ? '/v1/imports/resume' : '/v1/imports/start'" in sdk
assert '"delete" | "copy" | "move"' in core.split('fn reconcile_batch_child_receipts(',1)[1].split('/// All claims',1)[0]
# SQL reproduction: only authenticated RECONCILING owner/fingerprint can
# atomically claim the same operation. No second worker may claim it.
db=sqlite3.connect(':memory:')
db.executescript('''
PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON;
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY,owner_application_id TEXT);
CREATE TABLE mutation_receipts(space_id TEXT, request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE pending_mutations(id TEXT, space_id TEXT);
CREATE TABLE stream_operations(id TEXT, space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT,physical_path TEXT,capability TEXT);
INSERT INTO applications VALUES('owner');INSERT INTO spaces VALUES('space','owner');
''')
db.executescript((root/'crates/core/src/registry_v3.sql').read_text())
now=int(time.time()*1000)
def app_insert(owner='owner',fingerprint='f'):
  db.execute("INSERT INTO durable_operations(scope_kind,scope_id,request_id,operation_id,kind,owner_application_id,identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms,result_json) VALUES('application','owner','operation-1','operation-1','native-import',?,3,?,'RECONCILING',?,?,?,?)",
    (owner,fingerprint,now,now,now,json.dumps({'spaceId':'space','requestId':'request-1'})))
app_insert()
db.execute("INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','space','request-1','NATIVE_IMPORT','owner',1,'plan-fp','RUNNING',?,?,?)",(now,now,now))
for idx,phase in enumerate(['COMMITTED','SKIPPED','PENDING']):
  db.execute("INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,result_json,updated_at_ms) VALUES('space','space','request-1',?,?,?,NULL,?)",(idx,'item-'+str(idx),phase,now))
claim="UPDATE durable_operations SET phase='RUNNING',updated_at_ms=? WHERE scope_kind='application' AND scope_id=? AND request_id=? AND owner_application_id=? AND kind='native-import' AND request_fingerprint=? AND phase='RECONCILING'"
assert db.execute(claim,(now,'owner','operation-1','WRONG','f')).rowcount==0
assert db.execute(claim,(now,'owner','operation-1','owner','WRONG')).rowcount==0
assert db.execute(claim,(now,'owner','operation-1','owner','f')).rowcount==1
assert db.execute(claim,(now,'owner','operation-1','owner','f')).rowcount==0
assert db.execute("SELECT phase FROM durable_operation_items WHERE request_id='request-1' ORDER BY item_index").fetchall()==[('COMMITTED',),('SKIPPED',),('PENDING',)]
# Unknown RUNNING is not admissible from a missing receipt; only proof of
# COMMITTED changes its phase. Terminal evidence is protected by DB triggers.
db.execute("UPDATE durable_operation_items SET phase='RUNNING',result_json=? WHERE scope_id='space' AND item_index=2",(json.dumps({'target':'/x','childRequestId':'request-1:import:2'}),))
assert db.execute("SELECT COUNT(*) FROM durable_operation_items WHERE scope_id='space' AND phase='RUNNING'").fetchone()[0]==1
try: db.execute("DELETE FROM durable_operation_items WHERE scope_id='space' AND item_index=2")
except sqlite3.IntegrityError: pass
else: raise AssertionError('unfinished item deleted')
assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
print('PASS: CP12 stable resume owner/fingerprint CAS + unchanged item evidence; CP13 receipt-only composite reconciliation assertions')

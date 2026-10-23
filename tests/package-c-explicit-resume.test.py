#!/usr/bin/env python3
"""CP12/13 source-linked and synthetic SQLite checks; no Rust/Windows claim."""
from pathlib import Path
import sqlite3, json, time
root=Path(__file__).resolve().parents[1]
core=(root/'crates/core/src/storage.rs').read_text()
rt=(root/'crates/runtime/src/lib.rs').read_text()
sdk=(root/'packages/sdk/src/index.ts').read_text()
for name in ('resume_application_native_export','native_export_bound_resume','prior_native_export_item',
             'resume_application_batch','prior_batch_result'):
    assert f'fn {name}' in core, name
assert '"/v1/exports/resume"' in rt and '"/v1/batch/resume"' in rt
assert "resume ? '/v1/exports/resume' : '/v1/exports/start'" in sdk
assert "resume ? '/v1/batch/resume' : '/v1/batch'" in sdk
assert 'phase IN (\'RUNNING\',\'FAILED\')' in core
assert 'if !matches!(state.as_str(),"COMMITTED" | "PENDING")' in core
assert 'if phase != "COMMITTED" { return Err(StorageError::OutcomeUnknown); }' in core
assert 'if let Some(previous) = storage.prior_batch_result' in rt
assert '"hadTarget":had_target' in core
assert 'if prior.status != LongOperationStatus::Reconciling' in rt
# Model duplicate claims, immutable per-item plan, and no retry of uncertain
# RUNNING state against the exact SQL v3 blueprint.
db=sqlite3.connect(':memory:')
db.executescript('''
PRAGMA foreign_keys=ON;PRAGMA recursive_triggers=ON;
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY,owner_application_id TEXT);
CREATE TABLE mutation_receipts(space_id TEXT, request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
CREATE TABLE stream_operations(id TEXT,space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT,physical_path TEXT,capability TEXT);
INSERT INTO applications VALUES('owner');INSERT INTO spaces VALUES('space','owner');
''')
db.executescript((root/'crates/core/src/registry_v3.sql').read_text())
now=int(time.time()*1000)
for scope,ident,kind in [('application','owner','native-export'),('space','space','BATCH')]:
    db.execute('''INSERT INTO durable_operations(scope_kind,scope_id,request_id,operation_id,kind,owner_application_id,
    identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms,result_json)
    VALUES(?,?,?, ?,?,'owner',3,'fingerprint','RECONCILING',?,?,?,?)''',
    (scope,ident,'operation-1' if scope=='application' else 'batch-1',
     'operation-1' if scope=='application' else 'batch-1',kind,now,now,now,
     json.dumps({'spaceId':'space','destinationGrantId':'grant-1'}) if scope=='application' else None))
for ident,scope,phases in [('operation-1','application',['COMMITTED','PENDING']),('batch-1','space',['COMMITTED','RUNNING'])]:
    for idx,phase in enumerate(phases):
        db.execute('''INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,result_json,updated_at_ms)
        VALUES(?,?,?,?,?,?,?,?)''',(scope,'owner' if scope=='application' else 'space',ident,idx,'fp'+str(idx),phase,
        json.dumps({'index':idx,'ok':True}) if phase=='COMMITTED' else None,now))
# Export's durable prefix plus untouched PENDING can be admitted once.
q="""UPDATE durable_operations SET phase='RUNNING',updated_at_ms=? WHERE scope_kind='application' AND scope_id=? AND request_id=? AND owner_application_id=? AND kind='native-export' AND request_fingerprint=? AND phase='RECONCILING'"""
assert db.execute(q,(now,'owner','operation-1','wrong','fingerprint')).rowcount==0
assert db.execute(q,(now,'owner','operation-1','owner','fingerprint')).rowcount==1
assert db.execute(q,(now,'owner','operation-1','owner','fingerprint')).rowcount==0
# Batch cannot be resumed while a child is RUNNING; it must reconcile first.
assert db.execute("SELECT COUNT(*) FROM durable_operation_items WHERE scope_kind='space' AND request_id='batch-1' AND phase NOT IN ('COMMITTED','PENDING')").fetchone()[0]==1
try: db.execute("DELETE FROM durable_operation_items WHERE scope_kind='space' AND request_id='batch-1' AND item_index=1")
except sqlite3.IntegrityError: pass
else: raise AssertionError('unresolved evidence erased')
assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
print('PASS: CP12/13 explicit saved-identity resume source contracts + synthetic SQLite admission/evidence guards')

#!/usr/bin/env python3
"""CP14 long-job cancellation fence, source-linked SQLite model.

No Rust/native execution: tests the exact SQL statements and phase invariants.
"""
from pathlib import Path
import sqlite3

root = Path(__file__).resolve().parents[1]
core = (root/'crates/core/src/storage.rs').read_text()
runtime = (root/'crates/runtime/src/lib.rs').read_text()
start = core.index('pub fn request_application_long_job_cancel(')
end = core.index('    /// Explicit restart resume',start)
admission = core[start:end]
assert 'TransactionBehavior::Immediate' in admission
assert 'request_fingerprint=?5' in admission
assert 'phase IN (\'ADMITTED\',\'RUNNING\')' in admission
assert 'fields.contains_key("_cancelRequestedAtMs")' in admission
assert 'fields.insert("_cancelRequestedAtMs"' in admission
assert 'tx.commit()?;' in admission

cancel = runtime.split('fn cancel_application_operation(',1)[1].split('fn operation_cancelled(',1)[0]
assert cancel.index('request_application_long_job_cancel(') < cancel.index('entry.cancel.store(true')
assert cancel.index('request_application_long_job_cancel(') < cancel.index('entry.snapshot.status = LongOperationStatus::Cancelling')
assert 'entry.owner_application_id.as_deref() != Some(application_id)' in cancel
assert 'fingerprint.as_deref()' in cancel
assert "phase IN ('ADMITTED','RUNNING','CANCEL_REQUESTED')" in core
assert "phase IN ('ADMITTED','RUNNING','CANCEL_REQUESTED','RECONCILING')" in core
assert "'_cancelRequestedAtMs'" not in core, 'Use unambiguous JSON key literal'
assert '_cancelRequestedAtMs' in core.split('pub fn resume_application_native_export(',1)[1]
assert '_cancelRequestedAtMs' in core.split('pub fn resume_application_native_import(',1)[1]
assert '_cancelRequestedAtMs' in core.split('pub fn resume_application_batch(',1)[1]
assert '_cancelRequestedAtMs' in core.split('pub fn mark_application_long_job_uncertain(',1)[1]
assert 'CANCEL_OUTCOME_UNKNOWN' in runtime

con=sqlite3.connect(':memory:')
con.executescript('''
 CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,request_id TEXT,owner_application_id TEXT,
 request_fingerprint TEXT,phase TEXT CHECK(phase IN ('ADMITTED','RUNNING','RECONCILING','COMMITTED','ABORTED','PARTIALLY_APPLIED','NEEDS_REPAIR')),result_json TEXT,updated_at_ms INT,
 PRIMARY KEY(scope_kind,scope_id,request_id));
 INSERT INTO durable_operations VALUES('application','app-1','job-1','app-1','fp-1','RUNNING',
 '{"spaceId":"one","requestId":"child-1"}',1);
''')
from json import loads, dumps
select_sql="SELECT request_fingerprint,phase,result_json FROM durable_operations WHERE scope_kind='application' AND scope_id=?1 AND request_id=?2 AND owner_application_id=?1"
update_sql="UPDATE durable_operations SET result_json=?1,updated_at_ms=?2 WHERE scope_kind='application' AND scope_id=?3 AND request_id=?4 AND owner_application_id=?3 AND request_fingerprint=?5 AND phase IN ('ADMITTED','RUNNING')"
recover_sql="UPDATE durable_operations SET phase='RECONCILING',updated_at_ms=?1 WHERE scope_kind='application' AND kind<>'SPACE_OPEN' AND phase IN ('ADMITTED','RUNNING','CANCEL_REQUESTED')"
assert select_sql in admission
assert update_sql in admission
assert recover_sql in core
row=con.execute(select_sql,('app-1','job-1')).fetchone()
assert row[:2]==('fp-1','RUNNING')
evidence=loads(row[2]);evidence['_cancelRequestedAtMs']=123
assert con.execute(update_sql,(dumps(evidence),123,'app-1','job-1','fp-1')).rowcount==1
# SQL phase stays RUNNING under Registry v3; the method's JSON marker
# guard makes a repeated cancellation idempotent before the SQL statement.
assert loads(con.execute(select_sql,('app-1','job-1')).fetchone()[2])['_cancelRequestedAtMs']==123
assert con.execute(update_sql,(dumps(evidence),124,'app-1','job-1','different')).rowcount==0
assert con.execute(select_sql,('app-2','job-1')).fetchone() is None
assert con.execute("SELECT phase FROM durable_operations").fetchone()[0]=='RUNNING' 
# Add only a synthetic `kind` for the exact production startup SQL model.
con.execute('ALTER TABLE durable_operations ADD COLUMN kind TEXT')
con.execute("UPDATE durable_operations SET kind='native-import'")
con.execute(recover_sql,(500,))
phase, result = con.execute('SELECT phase,result_json FROM durable_operations').fetchone()
assert phase=='RECONCILING'
assert loads(result)=={'spaceId':'one','requestId':'child-1','_cancelRequestedAtMs':123}
# The restart resolver retains the cancellation proof, not a fictitious ABORTED.
print('PASS CP14 cancellation persisted before signal, replay/owner/fingerprint fences, restart uncertainty (source-linked SQLite only)')

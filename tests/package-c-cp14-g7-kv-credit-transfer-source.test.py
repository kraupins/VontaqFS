#!/usr/bin/env python3
"""G7 KV escrow transfer: source guard and SQLite atomicity simulation.

This test does not compile/execute Rust. Native correctness remains NOT RUN.
"""
from pathlib import Path
import sqlite3
src=(Path(__file__).resolve().parents[1]/'crates/core/src/storage.rs').read_text()
ddl=(Path(__file__).resolve().parents[1]/'crates/core/src/capacity_escrow.sql').read_text()
rt=(Path(__file__).resolve().parents[1]/'crates/runtime/src/lib.rs').read_text()

# Ensure that only the private, lease-verified Core path uses the reserved
# capacity override. Public standalone paths continue through the baseline.
assert 'fn batch_kv_set_with_credit(' in src
assert 'context.verify(self,space_id)?;' in src[src.index('fn batch_kv_set_with_credit('):src.index('fn batch_delete_path_with_progress<')]
start=src.index('fn kv_set_outcome_using_lease_and_credit(')
end=src.index('    pub fn write_file(',start)
kv=src[start:end]
assert 'validate_batch_receipt_credit(&tx,proof,space_id,request_id,"kv-set")?' in kv
assert 'sql_atomic_receipt_with_reserved::<KvRecord>' in kv
assert 'conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?' in kv
assert 'verify_capacity_escrow_spend(&tx,' in kv
assert 'spend_capacity_escrow(&tx,' in kv
assert kv.index('spend_capacity_escrow(&tx,') < kv.index('sql_atomic_store(&tx,') < kv.index('tx.commit()?;')
assert 'return Err(StorageError::RecoveryRequired);' in kv
reader=src[src.index('fn sql_atomic_receipt_with_reserved<'):src.index('fn sql_atomic_store<')]
assert 'if reservations > 1 || (reserved_credit && reservations != 0)' in reader
assert 'ensure_v3_result_capacity_slots(conn, now, 0)?' in reader
assert 'fn cp14_g7_batch_kv_receipt_consumes_exactly_one_escrow_credit_atomically()' in src
assert 'fn cp14_g7_failed_batch_kv_precondition_cannot_consume_credit()' in src
assert 'state.storage.begin_batch_execution(' in rt  # not activated until ALL kinds have proof transfer

# Execute actual shipped SQLite triggers and the credit/receipt transaction
# boundary. These semantics are SQL evidence, NOT native Rust test evidence.
c=sqlite3.connect(':memory:',isolation_level=None)
c.execute('PRAGMA foreign_keys=ON')
c.executescript('''
CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,request_id TEXT,kind TEXT,
  owner_application_id TEXT, phase TEXT, PRIMARY KEY(scope_kind,scope_id,request_id));
CREATE TABLE durable_operation_items(scope_kind TEXT,scope_id TEXT,request_id TEXT,
  item_index INTEGER,item_fingerprint TEXT,phase TEXT,result_json TEXT);
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,request_fingerprint TEXT,
  PRIMARY KEY(space_id,request_id));
CREATE TABLE kv_entries(space_id TEXT,key TEXT,value_json TEXT,PRIMARY KEY(space_id,key));
''')
c.executescript(ddl)
c.execute("INSERT INTO durable_operations VALUES('space','s','root','BATCH','owner','RUNNING')")
c.execute("INSERT INTO durable_operation_items VALUES('space','s','root',0,'immutable-item','RUNNING',?)",
          ('{"childRequestId":"child","kind":"kv-set"}',))
c.execute("INSERT INTO durable_capacity_escrows VALUES('space','s','root','owner','root-fp',1,1,'HELD',1,1)")

def transfer():
    changed=c.execute('''UPDATE durable_capacity_escrows SET remaining_slots=remaining_slots-1
      WHERE scope_kind='space' AND scope_id='s' AND root_request_id='root'
      AND owner_application_id='owner' AND state='HELD' AND remaining_slots>=1''').rowcount
    assert changed==1
    c.execute("INSERT INTO durable_capacity_spends VALUES('space','s','root',0,'child','RECEIPT','child-fp',1,1)")
    c.execute("INSERT INTO kv_entries VALUES('s','k','123')")
    c.execute("INSERT INTO mutation_receipts VALUES('s','child','child-fp')")

c.execute('BEGIN IMMEDIATE');transfer();c.execute('ROLLBACK')
assert c.execute("SELECT remaining_slots FROM durable_capacity_escrows").fetchone()==(1,)
assert c.execute('SELECT COUNT(*) FROM kv_entries').fetchone()==(0,)
assert c.execute('SELECT COUNT(*) FROM mutation_receipts').fetchone()==(0,)
assert c.execute('SELECT COUNT(*) FROM durable_capacity_spends').fetchone()==(0,)
c.execute('BEGIN IMMEDIATE');transfer();c.execute('COMMIT')
assert c.execute("SELECT remaining_slots FROM durable_capacity_escrows").fetchone()==(0,)
assert c.execute('SELECT COUNT(*) FROM kv_entries').fetchone()==(1,)
assert c.execute('SELECT COUNT(*) FROM mutation_receipts').fetchone()==(1,)
assert c.execute('SELECT COUNT(*) FROM durable_capacity_spends').fetchone()==(1,)
assert c.execute('''SELECT i.item_fingerprint,i.phase,json_extract(i.result_json,'$.childRequestId'),
    json_extract(i.result_json,'$.kind'),p.owner_application_id,e.state
    FROM durable_operation_items i JOIN durable_operations p ON p.scope_kind=i.scope_kind AND p.scope_id=i.scope_id AND p.request_id=i.request_id
    JOIN durable_capacity_escrows e ON e.scope_kind=p.scope_kind AND e.scope_id=p.scope_id AND e.root_request_id=p.request_id
    WHERE i.scope_id='s' AND i.request_id='root' AND i.item_index=0''').fetchone()==(
      'immutable-item','RUNNING','child','kv-set','owner','HELD')
try:
    c.execute("INSERT INTO durable_capacity_spends VALUES('space','s','root',0,'child','RECEIPT','another-fp',1,2)")
    raise AssertionError('duplicate spend escaped immutable identity guard')
except sqlite3.IntegrityError:
    pass
assert c.execute("SELECT remaining_slots FROM durable_capacity_escrows").fetchone()==(0,)
print('PASS CP14 G7 KV: source transaction boundary + SQLite rollback/replay-proof semantics; Rust NOT RUN')

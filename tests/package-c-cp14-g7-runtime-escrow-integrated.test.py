#!/usr/bin/env python3
"""Source-linked quota and route regressions, NOT native Rust execution.

Exercises the exact shipped SELECT from the production capacity helper and
models the atomic row replacements of BATCH -> WRITE -> COPY -> COPY plus
standalone MOVE. The added Rust tests must be executed by Cargo on a suitable
host before declaring native/release readiness.
"""
from pathlib import Path
import re
import sqlite3

root=Path(__file__).resolve().parents[1]
s=(root/'crates/core/src/storage.rs').read_text()
rt=(root/'crates/runtime/src/lib.rs').read_text()
lib=(root/'crates/core/src/lib.rs').read_text()
assert 'pub use storage::{AtomicWriteFault, BatchCapacityOperation, BatchExecution,' in lib
for hook in ('begin_batch_execution(', 'execute_batch_write(', 'execute_batch_delete(',
             'execute_batch_copy(', 'execute_batch_move(', 'execute_batch_kv('):
    assert hook in rt or hook in s,hook
assert 'state.storage.begin_batch_execution(' in rt
assert 'state.storage.prepare_batch_items(' not in rt
for kind in ('write', 'delete','copy','move','kv'):
    assert f'.execute_batch_{kind}(' in rt
assert 'fn spend_scoped_proof(' in s
assert 'fn prepaid_child_proof(' in s
assert 'fn scoped_capacity_slots(' in s
assert 'fn cp14_g7_active_batch_write_copy_copy_exclusive_and_paid_terminal()' in s
assert 'fn cp14_g7_active_batch_nested_move_fallback_is_fully_paid()' in s
assert 'fn cp14_g7_standalone_move_fallback_uses_same_escrow_transfer()' in s
assert 'fn cp14_g7_unfinished_root_keeps_credits_after_restart()' in s
quota_part=s.split('fn ensure_v3_result_capacity_slots(',1)[1].split('/// The checksum identifies',1)[0]
m=re.search(r'let protected: i64 = conn\.query_row\(\s*"(SELECT .*?)",\s*params!\[now\]',quota_part,re.S)
assert m
quota=m.group(1)
con=sqlite3.connect(':memory:',isolation_level=None)
con.execute('PRAGMA foreign_keys=ON')
con.executescript('''
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_space_outcomes(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_application_outcomes(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_stream_outcomes(expires_at_ms INTEGER);
CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,request_id TEXT,kind TEXT,phase TEXT,expires_at_ms INTEGER,
 owner_application_id TEXT,request_fingerprint TEXT,PRIMARY KEY(scope_kind,scope_id,request_id));
CREATE TABLE durable_operation_items(scope_kind TEXT,scope_id TEXT,request_id TEXT,item_index INTEGER,phase TEXT,result_json TEXT);
CREATE TABLE filesystem_intents(scope_kind TEXT,scope_id TEXT,request_id TEXT,state TEXT);
CREATE TABLE pending_mutations(space_id TEXT,id TEXT);
''')
con.executescript((root/'crates/core/src/capacity_escrow.sql').read_text())
now=1000
occupied=lambda:con.execute(quota,(now,)).fetchone()[0]

def parent(id,kind='BATCH'):
    con.execute('INSERT INTO durable_operations VALUES(?,?,?,?,?,?,?,?)',('space','s',id,kind,'RUNNING',None,'owner','fingerprint'))
def grant(root_id,credit):
    con.execute("INSERT INTO durable_capacity_escrows VALUES('space','s',?,'owner','fingerprint',?,?,'HELD',?,?)",
                (root_id,credit,credit,now,now))
def spend(root_id,child,kind,credit,index=0):
    con.execute('UPDATE durable_capacity_escrows SET remaining_slots=remaining_slots-? WHERE root_request_id=?',(credit,root_id))
    con.execute("INSERT INTO durable_capacity_spends VALUES('space','s',?,?,?,?,?,?,?)",
                (root_id,index,child,kind,'fp',credit,now))
def receipt(child):
    con.execute('INSERT INTO mutation_receipts VALUES(?,?,3,?)',('s',child,now+99999))

def check(n):
    assert occupied()==n,(n,occupied())
# Entire plan funded before effect. 1 root + 1 write + (2+1)*2 COPYs.
con.execute('BEGIN IMMEDIATE')
parent('batch')
grant('batch',7)
con.execute('COMMIT')
check(8)
assert occupied()+1>8 # another space cannot steal its capacity
# WRITE changes escrow -> pending journal in one admission transaction.
con.execute('BEGIN IMMEDIATE')
spend('batch','write','RECEIPT',1)
con.execute("INSERT INTO pending_mutations VALUES('s','write')")
con.execute('COMMIT')
check(8)
con.execute('BEGIN IMMEDIATE')
con.execute("DELETE FROM pending_mutations WHERE id='write'")
receipt('write')
con.execute('COMMIT')
check(8)
# Nested COPY parent and immutable child slot are both funded atomically.
for copy_id,child in [('copy1','copy1:copy:0'),('copy2','copy2:copy:0')]:
    con.execute('BEGIN IMMEDIATE')
    spend('batch',copy_id,'PARENT',2,1)
    spend('batch',child,'PARENT',1,1)
    parent(copy_id,'COPY')
    con.execute("INSERT INTO durable_operation_items VALUES('space','s',?,0,'PENDING',?)",
                (copy_id,'{"childRequestId":"'+child+'"}'))
    con.execute('COMMIT')
    check(8)
    con.execute('BEGIN IMMEDIATE')
    receipt(child)   # replaces the pending copy-item slot in global count
    con.execute('COMMIT')
    check(8)
    con.execute("UPDATE durable_operation_items SET phase='COMMITTED',result_json='{}' WHERE request_id=?",(copy_id,))
    check(8)
    con.execute('BEGIN IMMEDIATE')
    con.execute("UPDATE durable_operations SET phase='COMMITTED',expires_at_ms=? WHERE request_id=?",(now+99999,copy_id))
    receipt(copy_id) # replaces COPY's extra preterminal parent slot
    con.execute('COMMIT')
    check(8)
assert con.execute('SELECT remaining_slots FROM durable_capacity_escrows WHERE root_request_id=\'batch\'').fetchone()[0]==0
# Standalone MOVE actual root + 6 reserved slots equals the 7-credit bound.
parent('move','MOVE_COMPOSITE')
grant('move',6)
assert occupied()==15
assert occupied()+1>15
# Reservation and debits are durable; a root with uncertain children remains HELD.
assert con.execute("SELECT state FROM durable_capacity_escrows WHERE root_request_id='move'").fetchone()[0]=='HELD'
print('PASS G7 integrated route/source and exact production-SQL quota conservation (RUST NOT RUN)')

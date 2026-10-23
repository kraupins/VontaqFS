"""Synthetic SQLite + source contract verification; NOT a Rust runtime test."""
from pathlib import Path
import sqlite3
import re

project=Path(__file__).resolve().parents[1]
core=(project/'crates/core/src/storage.rs').read_text()
rt=(project/'crates/runtime/src/lib.rs').read_text()
proto=(project/'crates/protocol/src/lib.rs').read_text()
script=(project/'crates/core/src/retained_outcomes.sql').read_text()
assert 'pub fn retained_sql_receipt(' in core
assert 'retained_sql_receipt(&owner,kind,&scope.id,&input.operation_id)' in rt
assert 'let (owner, _) = state.authenticate_retained_proof(proof)?;' in rt
assert 'state.storage.live_sql_receipt(&session.application_id,kind,&scope.id,&input.operation_id)' in rt
assert 'RetainedReceiptScopeKind' in proto and 'RetainedReceiptOutcome' in proto
assert 'StorageError::ResultExpired' in core
conn=sqlite3.connect(':memory:')
conn.executescript(script)
now=1_793_000_000_000
sql='''SELECT operation,result_json,identity_version,request_fingerprint,created_at_ms,terminal_at_ms,expires_at_ms
       FROM retained_space_outcomes WHERE owner_application_id=? AND space_id=? AND request_id=?'''
conn.execute('INSERT INTO retained_space_outcomes VALUES (?,?,?,?,?,?,?,?,?,?)',
             ('owner-original','space-old','request-1','kv-set','{"revision":1}',100,3,'fingerprint',101,now+700000))
assert conn.execute(sql,('owner-original','space-old','request-1')).fetchone()[0]=='kv-set'
assert conn.execute(sql,('owner-imposter','space-old','request-1')).fetchone() is None
assert conn.execute(sql,('owner-original','space-imposter','request-1')).fetchone() is None
conn.execute('INSERT INTO retained_application_outcomes VALUES (?,?,?,?,?,?,?,?,?)',
             ('owner-original','request-2','preset-upsert','{"id":"p1"}',100,0,None,None,now+100))
assert conn.execute('''SELECT operation FROM retained_application_outcomes
    WHERE owner_application_id=? AND request_id=?''',('owner-original','request-2')).fetchone()==('preset-upsert',)
assert conn.execute('''SELECT operation FROM retained_application_outcomes
    WHERE owner_application_id=? AND request_id=?''',('owner-imposter','request-2')).fetchone() is None
# Synthetic GC guard must reject early deletion. Clock in trigger is real-time.
try:
    conn.execute('DELETE FROM retained_space_outcomes WHERE space_id=?',('space-old',))
    assert False,'v3 retained receipt prematurely erased'
except sqlite3.IntegrityError:
    pass
print('retained receipt ownership, scoped lookup, schema and GC guards: PASS')

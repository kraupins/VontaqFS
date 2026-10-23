#!/usr/bin/env python3
"""CP14 source wiring + isolated SQL transaction invariant (NOT native Rust execution)."""
from pathlib import Path
import sqlite3
base=Path(__file__).resolve().parents[1]
core=(base/'crates/core/src/storage.rs').read_text()
runtime=(base/'crates/runtime/src/lib.rs').read_text()
route=runtime.split('("POST", "/v1/spaces/clear") => {',1)[1].split('("POST", "/v1/kv/get") => {',1)[0]
method=core.split('fn clear_disposable_space_inner(',1)[1].split('pub fn clear_cache_space(',1)[0]
replace=core.split('fn commit_staged_space_replace(',1)[1].split('fn row_directory_grant(',1)[0]
assert 'clear_disposable_space_with_request(&work_space_id,&work_request_id)' in route
assert 'state.storage.replayed_space_clear(' in route
assert route.index('state.storage.replayed_space_clear(') < route.index('state.begin_application_operation(')
assert 'outcome.disposition == MutationDisposition::Applied' in route
assert 'fs_operation_replay::<SpaceClearReport>' in method
assert '"SPACE_CLEAR"' in method
assert 'ensure_v3_result_capacity_slots(&tx,now,2)' in method
assert method.index("INSERT INTO durable_operations") < method.index('commit_staged_space_replace(')
assert 'public_clear: Option<(&str, &str, &SpaceClearReport)>' in core
assert replace.index('engine.complete_fs_intent(') < replace.index("kind='SPACE_CLEAR'") < replace.index('sql_atomic_store(&tx,false,space_id,public_request') < replace.index('tx.commit()?;')
assert 'fn public_space_clear_replays_original_report_after_restart_without_erasing_new_data()' in core
assert 'fn admitted_space_clear_is_not_reexecuted_without_verified_outcome()' in core
# Demonstrate rollback of the paired parent/receipt publication if the transaction aborts.
db=sqlite3.connect(':memory:')
db.executescript('''CREATE TABLE parent (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, phase TEXT NOT NULL);
CREATE TABLE receipt (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL);
INSERT INTO parent VALUES ('r1','hash','ADMITTED');''')
db.execute('BEGIN IMMEDIATE')
db.execute("UPDATE parent SET phase='COMMITTED' WHERE id='r1'")
db.execute("INSERT INTO receipt VALUES ('r1','hash')")
db.rollback()
assert db.execute("SELECT phase FROM parent WHERE id='r1'").fetchone()==('ADMITTED',)
assert db.execute('SELECT COUNT(*) FROM receipt').fetchone()[0]==0
db.execute('BEGIN IMMEDIATE')
db.execute("UPDATE parent SET phase='COMMITTED' WHERE id='r1'")
db.execute("INSERT INTO receipt VALUES ('r1','hash')")
db.commit()
assert db.execute("SELECT p.phase FROM parent p JOIN receipt r ON p.id=r.id AND p.fingerprint=r.fingerprint").fetchone()==('COMMITTED',)
print('PASS: public space-clear identity wiring and synthetic SQL atomicity (not native Rust)')

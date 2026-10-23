#!/usr/bin/env python3
"""CP14 /v1/spaces/open source wiring and isolated SQLite invariants.

Not a Rust compilation, native filesystem test, or real migration validation.
"""
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
core = (ROOT / 'crates/core/src/storage.rs').read_text()
runtime = (ROOT / 'crates/runtime/src/lib.rs').read_text()
section = core.split('pub fn open_space_with_request(', 1)[1].split('pub fn list_spaces(', 1)[0]
route = runtime.split('("POST", "/v1/spaces/open") => {', 1)[1].split('("POST", "/v1/spaces/list")', 1)[0]
assert 'state.storage.open_space_with_request(' in route
assert 'input.display_name.as_deref(), &input.request_id' in route
assert 'open_space_with_category(' not in route
assert 'sql_atomic_receipt::<SpaceRecord>' in section
assert 'semantic_fingerprint("space-open-existing"' in section
assert 'ensure_v3_result_capacity_slots(&tx, now, 2)' in section
assert section.index('INSERT INTO durable_operations') < section.index('open_space_with_category_context(')
assert section.index('open_space_with_category_context(') < section.index("SET phase='COMMITTED'")
assert section.index("SET phase='COMMITTED'") < section.index('sql_atomic_store(&tx,true,') < section.index('tx.commit()?;', section.index('sql_atomic_store(&tx,true,'))
assert 'RequestIdConflict' in section
assert 'RecoveryRequired' in section
assert 'ResultExpired' in section
assert 'fn public_space_open_replays_create_and_category_without_second_manifest_mutation()' in core
assert 'fn incomplete_public_space_open_is_not_reapplied()' in core
assert 'durable_replay: None' in runtime

# Simulate the core's *SQL visibility* invariant: an admitted parent may exist
# before filesystem mutation, while terminal parent + receipt publish together.
db = sqlite3.connect(':memory:')
db.executescript('''
CREATE TABLE parent(owner TEXT, request TEXT, fingerprint TEXT, phase TEXT, result TEXT,
                    PRIMARY KEY(owner,request));
CREATE TABLE receipt(owner TEXT, request TEXT, fingerprint TEXT, result TEXT,
                     PRIMARY KEY(owner,request));
INSERT INTO parent VALUES ('app','req','SHA-1','ADMITTED',NULL);
''')
db.execute('BEGIN IMMEDIATE')
db.execute("UPDATE parent SET phase='COMMITTED',result=? WHERE owner='app' AND request='req'", ('{"id":"space-1"}',))
db.execute('INSERT INTO receipt VALUES (?,?,?,?)', ('app','req','SHA-1','{"id":"space-1"}'))
db.rollback()
assert db.execute("SELECT phase FROM parent WHERE request='req'").fetchone() == ('ADMITTED',)
assert db.execute('SELECT COUNT(*) FROM receipt').fetchone()[0] == 0
# Repeated admissions cannot overwrite the parent; a different payload is a conflict.
try:
    db.execute('INSERT INTO parent VALUES (?,?,?,?,?)', ('app','req','SHA-2','ADMITTED',None))
    raise AssertionError('duplicate identity accepted')
except sqlite3.IntegrityError:
    db.rollback()
db.execute('BEGIN IMMEDIATE')
db.execute("UPDATE parent SET phase='COMMITTED',result=? WHERE owner='app' AND request='req'", ('{"id":"space-1"}',))
db.execute('INSERT INTO receipt VALUES (?,?,?,?)', ('app','req','SHA-1','{"id":"space-1"}'))
db.commit()
assert db.execute('SELECT p.result FROM parent p JOIN receipt r ON p.owner=r.owner AND p.request=r.request AND p.fingerprint=r.fingerprint').fetchone()[0] == '{"id":"space-1"}'
print('PASS: space-open all-path durable admission + terminal SQL atomicity (source-linked/synthetic only)')

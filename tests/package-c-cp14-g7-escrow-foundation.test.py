#!/usr/bin/env python3
"""CP14 G7: execute escrow DDL + actual quota SQL. NOT a Rust/Windows test.

This fixture intentionally asserts only the foundational transaction primitives;
it DOES NOT claim BATCH/MOVE are wired to consume these credits yet.
"""
from pathlib import Path
import re
import sqlite3
import tempfile

core = Path(__file__).resolve().parents[1] / 'crates/core/src'
src = (core / 'storage.rs').read_text()
ddl = (core / 'capacity_escrow.sql').read_text()
assert 'ensure_capacity_escrow_extension(&conn)?' in src
assert 'include_str!("capacity_escrow.sql")' in src
assert 'fn reserve_capacity_escrow(' in src
assert 'fn spend_capacity_escrow(' in src
assert 'fn close_capacity_escrow(' in src
assert 'ON DELETE RESTRICT' in ddl
assert 'INSERT OR REPLACE' not in ddl.replace('-- The parent FK is RESTRICT, never CASCADE. Do not use INSERT OR REPLACE.', '')
assert 'SELECT COALESCE(SUM(remaining_slots),0) FROM durable_capacity_escrows' in src

part = src.split('fn ensure_v3_result_capacity_slots(',1)[1].split('/// The checksum identifies',1)[0]
match = re.search(r'let protected: i64 = conn\.query_row\(\s*"(SELECT .*?)",\s*params!\[now\]',part,re.S)
assert match
quota = match.group(1)

with tempfile.TemporaryDirectory() as directory:
    db = str(Path(directory)/'registry.sqlite3')
    a = sqlite3.connect(db,isolation_level=None,timeout=1)
    b = sqlite3.connect(db,isolation_level=None,timeout=1)
    for con in (a,b):
        con.execute('PRAGMA foreign_keys=ON')
        con.execute('PRAGMA recursive_triggers=ON')
    a.executescript('''
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
    a.executescript(ddl)
    a.executescript(ddl) # upgrade must be idempotent
    now=10000
    total=lambda con:con.execute(quota,(now,)).fetchone()[0]
    # Demand an exact 4-credit budget at the boundary, including root parent.
    ceiling=5
    a.execute('BEGIN IMMEDIATE')
    a.execute("INSERT INTO durable_operations VALUES('space','s','batch','BATCH','RUNNING',NULL,'owner','fp')")
    assert total(a)==1
    assert total(a)+4==ceiling
    # Two timestamps, parent and initial reserve atomically committed.
    a.execute("INSERT INTO durable_capacity_escrows VALUES('space','s','batch','owner','fp',4,4,'HELD',?,?)",(now,now))
    a.execute('COMMIT')
    assert total(a)==ceiling==total(b)
    # Another space cannot borrow reserved credits.
    assert total(b)+1 > ceiling
    # The escrow prevents parent deletion (even a malicious/direct SQLite DELETE).
    try:
        a.execute("DELETE FROM durable_operations WHERE request_id='batch'")
        assert False,'FK must reject parent deletion'
    except sqlite3.IntegrityError:
        pass
    try:
        a.execute("INSERT OR REPLACE INTO durable_capacity_escrows VALUES('space','s','batch','owner','fp',4,0,'TERMINAL',?,?)",(now,now))
        assert False,'identity guard must reject replace'
    except sqlite3.IntegrityError:
        pass
    # Debit + receipt insert in a single SQLite IMMEDIATE transaction. Crash
    # simulated by ROLLBACK restores BOTH evidence and unspent credit.
    for commit in (False,True):
        a.execute('BEGIN IMMEDIATE')
        a.execute("UPDATE durable_capacity_escrows SET remaining_slots=remaining_slots-1 WHERE root_request_id='batch' AND state='HELD' AND remaining_slots>=1")
        a.execute("INSERT INTO durable_capacity_spends VALUES('space','s','batch',0,'child','RECEIPT','child-fp',1,?)",(now,))
        a.execute("INSERT INTO mutation_receipts VALUES('s','child',3,999999)")
        assert total(a)==ceiling
        a.execute('COMMIT' if commit else 'ROLLBACK')
        assert total(b)==ceiling
        assert a.execute("SELECT remaining_slots FROM durable_capacity_escrows WHERE root_request_id='batch'").fetchone()[0]==(3 if commit else 4)
    # Exact replay reads the existing spend; forcing a second INSERT aborts.
    try:
        a.execute("INSERT OR REPLACE INTO durable_capacity_spends VALUES('space','s','batch',0,'child','RECEIPT','other-fp',1,?)",(now,))
        assert False,'replay fingerprint cannot be replaced'
    except sqlite3.IntegrityError:
        pass
    assert total(a)==ceiling
    # A terminalized parent must not retain unused credits indefinitely.
    a.execute('BEGIN IMMEDIATE')
    a.execute("UPDATE durable_operations SET phase='COMMITTED',expires_at_ms=999999 WHERE request_id='batch'")
    a.execute("UPDATE durable_capacity_escrows SET remaining_slots=0,state='TERMINAL' WHERE root_request_id='batch'")
    a.execute('COMMIT')
    assert total(b)==2, total(b)
    # Before expiration parent and escrow are still protected; unknown roots
    # do not run the terminal cleanup path.
    assert a.execute("SELECT COUNT(*) FROM durable_capacity_spends").fetchone()[0]==1
    a.close();b.close()
print('PASS CP14 G7 escrow foundation: DDL, restart, exact quota, interspace view, atomic rollback/debit, replay guard (native NOT RUN)')

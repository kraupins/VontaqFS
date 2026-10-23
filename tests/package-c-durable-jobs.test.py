#!/usr/bin/env python3
"""Synthetic SQL test of the CP12/13 admission, immutable identities and
per-item evidence. Does not execute Core, Runtime, OS crash or Windows.
"""
from pathlib import Path
import sqlite3
import time

schema = (Path(__file__).parents[1] / 'crates/core/src/registry_v3.sql').read_text()
db = sqlite3.connect(':memory:')
db.execute('PRAGMA foreign_keys=ON')
db.execute('PRAGMA recursive_triggers=ON')
db.executescript('''
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY,owner_application_id TEXT NOT NULL);
CREATE TABLE schema_meta(version INTEGER NOT NULL);
INSERT INTO schema_meta VALUES(2);
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
CREATE TABLE stream_operations(id TEXT,space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT,physical_path TEXT,capability TEXT);
INSERT INTO applications VALUES('owner');
INSERT INTO spaces VALUES('space','owner');
''')
db.executescript(schema)
now = int(time.time() * 1000)
week = 7 * 86400 * 1000
app_insert = '''INSERT INTO durable_operations(scope_kind,scope_id,request_id,operation_id,kind,
 owner_application_id,identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms)
 VALUES('application','owner','operation-abc','operation-abc','native-import','owner',1,'fingerprint','ADMITTED',?,?,?)'''
db.execute(app_insert, (now,now,now))
db.commit()
for attempt in ('replace','insert'):
    try:
        if attempt == 'replace':
            db.execute(app_insert.replace('INSERT INTO','INSERT OR REPLACE INTO'), (now,now,now))
        else:
            db.execute(app_insert, (now,now,now))
    except sqlite3.DatabaseError:
        db.rollback()
    else:
        raise AssertionError('Duplicate durable worker identity replaced an existing operation')
assert db.execute("SELECT COUNT(*) FROM durable_operations WHERE scope_kind='application'").fetchone()[0] == 1
# Simulate startup recovery of orphaned nonterminal jobs.
db.execute("UPDATE durable_operations SET phase='RECONCILING' WHERE scope_kind='application' AND phase='ADMITTED'")
assert db.execute("SELECT phase FROM durable_operations WHERE request_id='operation-abc'").fetchone()[0] == 'RECONCILING'
# Terminal outcome remains after restart and cannot be removed within guarantee.
db.execute('''UPDATE durable_operations SET phase='COMMITTED',result_json='{"status":"completed"}',terminal_at_ms=?,expires_at_ms=?
 WHERE scope_kind='application' AND request_id='operation-abc' ''',(now,now+week))
db.commit()
for query in ("DELETE FROM durable_operations WHERE request_id='operation-abc'", "UPDATE durable_operations SET phase='RUNNING' WHERE request_id='operation-abc'"):
    try:
        db.execute(query)
    except sqlite3.DatabaseError:
        db.rollback()
    else:
        raise AssertionError('Unexpired terminal operation was mutated or deleted')
# Ordered child identity and journaled partial results.
db.execute('''INSERT INTO durable_operations(scope_kind,scope_id,request_id,operation_id,kind,owner_application_id,identity_version,
 request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms)
 VALUES('space','space','batch-1','batch-1','BATCH','owner',1,'batch-fingerprint','RUNNING',?,?,?)''',(now,now,now))
for index in range(3):
    db.execute("INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,updated_at_ms) VALUES('space','space','batch-1',?,?,'PENDING',?)",(index,f'item-{index}',now))
db.execute("UPDATE durable_operation_items SET phase='COMMITTED',result_json='{}' WHERE request_id='batch-1' AND item_index=0")
db.execute("UPDATE durable_operation_items SET phase='FAILED',result_json='{}' WHERE request_id='batch-1' AND item_index=1")
db.execute("UPDATE durable_operations SET phase='PARTIALLY_APPLIED',result_json='{}',terminal_at_ms=?,expires_at_ms=? WHERE request_id='batch-1'",(now,now+week))
db.commit()
try:
    db.execute("DELETE FROM durable_operations WHERE request_id='batch-1'")
except sqlite3.DatabaseError:
    db.rollback()
else:
    raise AssertionError('Lost unresolved batch item to parent deletion')
assert db.execute("SELECT phase FROM durable_operation_items WHERE request_id='batch-1' ORDER BY item_index").fetchall() == [('COMMITTED',),('FAILED',),('PENDING',)]
assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
print('PASS: synthetic long-job dedupe/reconciliation + retained result + durable batch partial evidence')

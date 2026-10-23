#!/usr/bin/env python3
"""Synthetic v3 owner-delete durability and retention regression.

Does not execute Rust or force a real process crash; checks SQL contract only.
"""
from pathlib import Path
import sqlite3, tempfile, time

DDL=(Path(__file__).parents[1]/'crates/core/src/registry_v3.sql').read_text()
with tempfile.TemporaryDirectory() as folder:
    con=sqlite3.connect(str(Path(folder)/'registry.db'))
    con.execute('PRAGMA foreign_keys=ON')
    con.execute('PRAGMA recursive_triggers=ON')
    con.executescript('''
        CREATE TABLE schema_meta(version INTEGER NOT NULL); INSERT INTO schema_meta VALUES (2);
        CREATE TABLE applications(id TEXT PRIMARY KEY);
        CREATE TABLE spaces(id TEXT PRIMARY KEY,owner_application_id TEXT NOT NULL REFERENCES applications(id));
        CREATE TABLE mutation_receipts(space_id TEXT NOT NULL REFERENCES spaces(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(space_id,request_id));
        CREATE TABLE application_mutation_receipts(application_id TEXT NOT NULL REFERENCES applications(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(application_id,request_id));
        CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
        CREATE TABLE stream_operations(stream_id TEXT);
        INSERT INTO applications VALUES ('app'); INSERT INTO spaces VALUES ('space','app');
    ''')
    con.executescript(DDL)
    now=int(time.time()*1000)
    week=7*86400*1000
    con.execute("INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES('space','retained','kv-set','{}',?,3,'fp',?,?)", (now,now,now+week))
    con.commit()
    try:
        con.execute("DELETE FROM mutation_receipts WHERE space_id='space'")
    except sqlite3.DatabaseError:
        con.rollback()
    else:
        raise AssertionError('v3 retained receipt deleted')
    # Deletion is safe to begin only after the receipt has legitimately expired.
    con.execute("UPDATE mutation_receipts SET terminal_at_ms=?,expires_at_ms=? WHERE request_id='retained'",(now-week-1,now-3600*1000))
    con.execute("DELETE FROM mutation_receipts WHERE space_id='space'")
    con.commit()
    con.execute("INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('application','app','delete-v3-x','APP_DELETE','app','fp','ADMITTED',?,?,?)",(now,now,now))
    for name in ('intent-a','intent-b'):
        con.execute("INSERT INTO filesystem_intents(intent_id,scope_kind,scope_id,request_id,intent_kind,state,payload_json,updated_at_ms) VALUES(?,'application','app','delete-v3-x','APP_DELETE','PREPARED','{}',?)",(name,now))
    con.commit()
    # The filesystem may be physically staged but the SQL owner is retained
    # until both intents transition to DB_COMMITTED in the same transaction.
    con.execute('BEGIN IMMEDIATE')
    con.execute("DELETE FROM spaces WHERE id='space'")
    con.execute("DELETE FROM applications WHERE id='app'")
    con.execute("UPDATE filesystem_intents SET state='DB_COMMITTED' WHERE request_id='delete-v3-x'")
    con.execute("UPDATE durable_operations SET phase='COMMITTED',result_json='[1,0,0]',terminal_at_ms=?,expires_at_ms=?,cleanup_pending=1 WHERE request_id='delete-v3-x'",(now,now+week))
    con.commit()
    assert con.execute('PRAGMA foreign_key_check').fetchall()==[]
    assert con.execute("SELECT owner_application_id FROM durable_operations WHERE request_id='delete-v3-x'").fetchone() == ('app',)
    con.execute("UPDATE filesystem_intents SET state='CLEANED' WHERE intent_id='intent-a'")
    assert con.execute("SELECT COUNT(*) FROM filesystem_intents WHERE state!='CLEANED'").fetchone()[0]==1
    con.execute("UPDATE filesystem_intents SET state='CLEANED' WHERE intent_id='intent-b'")
    con.execute("UPDATE durable_operations SET cleanup_pending=0 WHERE request_id='delete-v3-x'")
    con.commit()
    assert con.execute("SELECT phase,cleanup_pending FROM durable_operations WHERE request_id='delete-v3-x'").fetchone()==('COMMITTED',0)
    print('PASS: synthetic retained v3 receipt guard, durable app delete, two child intents, owner-independent result')

#!/usr/bin/env python3
"""SQLite v3 intent and parent transition checks against synthetic v2 schema.

Does NOT execute Rust storage code or simulate actual OS rename/crashes.
"""
import pathlib, sqlite3, tempfile, time
sql=(pathlib.Path(__file__).parents[1]/'crates/core/src/registry_v3.sql').read_text()
with tempfile.TemporaryDirectory() as tmp:
    c=sqlite3.connect(pathlib.Path(tmp)/'registry.sqlite3')
    c.executescript('''
    PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON;
    CREATE TABLE schema_meta(version INTEGER); INSERT INTO schema_meta VALUES(2);
    CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(space_id,request_id));
    CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(application_id,request_id));
    CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
    CREATE TABLE stream_operations(stream_id TEXT);
    ''')
    c.executescript(sql)
    now=int(time.time()*1000)
    c.execute("INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','s','r','DELETE','app','fp','ADMITTED',?,?,?)",(now,now,now))
    c.execute("INSERT INTO filesystem_intents(intent_id,scope_kind,scope_id,request_id,intent_kind,state,payload_json,updated_at_ms) VALUES('i','space','s','r','DELETE','PREPARED','{}',?)",(now,))
    c.commit()
    # Parent of PREPARED intent can't be deleted; malformed replacement rejected.
    for statement in ["DELETE FROM durable_operations WHERE request_id='r'", "DELETE FROM filesystem_intents WHERE intent_id='i'", "INSERT OR REPLACE INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','s','r','DELETE','app','different','ADMITTED',1,1,1)"]:
        try:
            c.execute(statement); c.commit(); raise AssertionError('unsafe operation was permitted')
        except sqlite3.DatabaseError: c.rollback()
    # Commit receipt and durable terminal parent together.
    c.execute('BEGIN IMMEDIATE')
    c.execute("UPDATE filesystem_intents SET state='DB_COMMITTED' WHERE intent_id='i'")
    c.execute("UPDATE durable_operations SET phase='COMMITTED',terminal_at_ms=?,expires_at_ms=?,cleanup_pending=1 WHERE request_id='r'",(now,now+7*24*3600*1000))
    c.execute("INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES('s','r','fs-delete','1',?,3,'fp',?,?)",(now,now,now+7*24*3600*1000))
    c.commit()
    c.execute('BEGIN IMMEDIATE')
    c.execute("UPDATE filesystem_intents SET state='CLEANED' WHERE intent_id='i'")
    c.execute("UPDATE durable_operations SET cleanup_pending=0 WHERE request_id='r'")
    c.commit()
    assert c.execute('PRAGMA foreign_key_check').fetchall()==[]
    assert c.execute("SELECT phase,cleanup_pending FROM durable_operations WHERE request_id='r'").fetchone()==('COMMITTED',0)
    assert c.execute("SELECT state FROM filesystem_intents WHERE intent_id='i'").fetchone()==('CLEANED',)
    print('PASS: synthetic v3 durable parent/intent/receipt transitions and forbidden deletion/replace')

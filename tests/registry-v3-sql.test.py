#!/usr/bin/env python3
"""Synthetic v2->v3 DDL invariant test, NOT a Rust production migration test."""
import pathlib, sqlite3, tempfile, time

SQL = (pathlib.Path(__file__).parents[1] / 'crates/core/src/registry_v3.sql').read_text()
with tempfile.TemporaryDirectory() as work:
    db = pathlib.Path(work) / 'registry.sqlite3'
    cx = sqlite3.connect(db)
    cx.execute('PRAGMA foreign_keys=ON')
    cx.execute('PRAGMA recursive_triggers=ON')
    cx.executescript('''
        CREATE TABLE schema_meta(version INTEGER);
        INSERT INTO schema_meta VALUES(2);
        CREATE TABLE applications(id TEXT PRIMARY KEY);
        CREATE TABLE spaces(id TEXT PRIMARY KEY);
        CREATE TABLE pairings(id TEXT PRIMARY KEY);
        CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(space_id,request_id));
        CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(application_id,request_id));
        CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
        CREATE TABLE stream_operations(stream_id TEXT);
        INSERT INTO applications VALUES('app');
        INSERT INTO spaces VALUES('space');
    ''')
    backup = pathlib.Path(work) / 'pre-v3.sqlite3'
    cx.execute('VACUUM INTO ?', (str(backup),))
    assert sqlite3.connect(backup).execute('SELECT version FROM schema_meta').fetchone()[0] == 2
    cx.execute('BEGIN IMMEDIATE')
    # Python sqlite3 executescript auto-commits, so exercise DDL+version as an
    # ordered sequence in a synthetic fixture; Rust owns the actual transaction.
    cx.executescript(SQL)
    cx.execute('UPDATE schema_meta SET version=3 WHERE version=2')
    cx.commit()
    assert cx.execute('PRAGMA quick_check').fetchone() == ('ok',)
    assert cx.execute('PRAGMA foreign_key_check').fetchall() == []
    assert cx.execute('SELECT version FROM schema_meta').fetchone()[0] == 3
    now = round(time.time() * 1000)
    base = ('space', 'space', 'req1', 'kv-set', 'app', 3, 'fp', 'RUNNING', now, now, now)
    cx.execute('''INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,identity_version,
        request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms)
        VALUES(?,?,?,?,?,?,?,?,?,?,?)''', base)
    cx.execute('INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,updated_at_ms) VALUES(?,?,?,?,?,?,?)',
               ('space','space','req1',0,'item-fp','RUNNING',now))
    cx.execute('INSERT INTO filesystem_intents(intent_id,scope_kind,scope_id,request_id,intent_kind,state,payload_json,updated_at_ms) VALUES(?,?,?,?,?,?,?,?)',
               ('intent1','space','space','req1','DELETE','PREPARED','{}',now))
    cx.commit()
    def must_fail(sql, args=()):
        try:
            cx.execute(sql,args)
            cx.commit()
        except sqlite3.DatabaseError:
            cx.rollback()
            return
        raise AssertionError(f'expected SQLite rejection: {sql}')
    must_fail("DELETE FROM durable_operations WHERE request_id='req1'")
    must_fail("INSERT OR REPLACE INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','space','req1','kv-set','app','different','ADMITTED',1,1,1)")
    must_fail("DELETE FROM filesystem_intents WHERE intent_id='intent1'")
    must_fail("DELETE FROM durable_operation_items WHERE request_id='req1'")
    cx.execute("UPDATE filesystem_intents SET state='CLEANED' WHERE intent_id='intent1'")
    cx.execute("UPDATE durable_operation_items SET phase='COMMITTED' WHERE request_id='req1'")
    cx.execute("UPDATE durable_operations SET phase='COMMITTED',terminal_at_ms=?,expires_at_ms=? WHERE request_id='req1'", (now,now + 1_000_000))
    cx.commit()
    must_fail("DELETE FROM durable_operations WHERE request_id='req1'")
    must_fail("UPDATE durable_operations SET phase='RUNNING' WHERE request_id='req1'")
    cx.execute("UPDATE durable_operations SET terminal_at_ms=?,expires_at_ms=? WHERE request_id='req1'", (now-10000, now-5000))
    cx.commit()
    cx.execute("DELETE FROM durable_operations WHERE request_id='req1'")
    cx.commit()
    assert cx.execute('SELECT count(*) FROM durable_operation_items').fetchone()[0] == 0
    assert cx.execute('SELECT count(*) FROM filesystem_intents').fetchone()[0] == 0
    print('PASS: SQLite v3 DDL parsed, backup v2, anti-REPLACE, anti-GC, FK and terminal guards')

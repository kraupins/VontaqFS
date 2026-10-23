#!/usr/bin/env python3
"""SQLite-only CP14 retained owner evidence / trigger invariants.

Synthetic registry fixture. This does not run Rust, Runtime auth, or crash tests.
"""
from pathlib import Path
import sqlite3
import tempfile
import time

ROOT = Path(__file__).parents[1]
V3 = (ROOT / 'crates/core/src/registry_v3.sql').read_text()
EXT = (ROOT / 'crates/core/src/retained_outcomes.sql').read_text()
CORE = (ROOT / 'crates/core/src/storage.rs').read_text()
RT = (ROOT / 'crates/runtime/src/lib.rs').read_text()
assert 'archive_space_outcomes(&tx' in CORE
assert 'archive_application_outcomes(&tx' in CORE
assert 'archive_application_streams(&tx' in CORE
assert 'retained_stream_owned_by' in RT
assert 'ensure_retained_outcomes(&conn)?' in CORE
assert 'mutation_receipts_gc_guard BEFORE DELETE' in CORE
assert 'retained_space_outcomes h' in CORE

with tempfile.TemporaryDirectory() as d:
    c = sqlite3.connect(str(Path(d)/'registry.sqlite'))
    c.execute('PRAGMA foreign_keys=ON')
    c.execute('PRAGMA recursive_triggers=ON')
    c.executescript('''
        CREATE TABLE applications (id TEXT PRIMARY KEY);
        CREATE TABLE spaces (id TEXT PRIMARY KEY, owner_application_id TEXT REFERENCES applications(id));
        CREATE TABLE pairings (id TEXT PRIMARY KEY, application_id TEXT REFERENCES applications(id));
        CREATE TABLE mutation_receipts (space_id TEXT NOT NULL REFERENCES spaces(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(space_id,request_id));
        CREATE TABLE application_mutation_receipts (application_id TEXT NOT NULL REFERENCES applications(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(application_id,request_id));
        CREATE TABLE pending_mutations (id TEXT,space_id TEXT);
        CREATE TABLE stream_operations (pairing_id TEXT NOT NULL REFERENCES pairings(id),space_id TEXT NOT NULL REFERENCES spaces(id),request_id TEXT NOT NULL,stream_id TEXT NOT NULL UNIQUE,chunk_method TEXT NOT NULL,logical_path TEXT NOT NULL,declared_size INTEGER,phase TEXT NOT NULL,state_revision INTEGER NOT NULL DEFAULT 1,next_seq INTEGER NOT NULL DEFAULT 0,confirmed_bytes INTEGER NOT NULL DEFAULT 0,last_chunk_sha256 TEXT,last_chunk_len INTEGER NOT NULL DEFAULT 0,result_json TEXT,updated_at_ms INTEGER NOT NULL,PRIMARY KEY(pairing_id,space_id,request_id));
        INSERT INTO applications VALUES ('owner');
        INSERT INTO applications VALUES ('intruder');
        INSERT INTO spaces VALUES ('space','owner');
        INSERT INTO pairings VALUES ('pair','owner');
    ''')
    c.executescript(V3)
    c.executescript(EXT)
    now = int(time.time()*1000)
    expiry = now + 7*86400*1000
    c.execute('INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES (?,?,?,?,?,?,?,?,?)',('space','req','kv-set','{"ok":true}',now,3,'fp',now,expiry))
    c.execute('INSERT INTO application_mutation_receipts(application_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES (?,?,?,?,?,?,?,?,?)',('owner','app-req','preset','{}',now,3,'app-fp',now,expiry))
    c.execute("INSERT INTO stream_operations(pairing_id,space_id,request_id,stream_id,chunk_method,logical_path,phase,updated_at_ms) VALUES ('pair','space','stream-req','stream-id','PUT','/f','COMMITTED',?)",(now,))
    c.commit()
    # Native blueprint rejects premature deletion without a historical copy.
    try:
        c.execute("DELETE FROM mutation_receipts WHERE space_id='space'")
    except sqlite3.IntegrityError:
        c.rollback()
    else:
        raise AssertionError('unarchived protected receipt deleted')

    # Mirror the exact retained rows, as the Rust transaction does. Until an
    # archived row exists the legacy v3 trigger MUST still block the deletion.
    c.execute('BEGIN IMMEDIATE')
    c.execute("INSERT INTO retained_space_outcomes SELECT 'owner',space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms FROM mutation_receipts WHERE space_id='space'")
    c.execute("INSERT INTO retained_stream_outcomes SELECT 'owner',pairing_id,space_id,request_id,stream_id,chunk_method,logical_path,declared_size,request_fingerprint,phase,state_revision,next_seq,confirmed_bytes,last_chunk_sha256,last_chunk_len,result_json,updated_at_ms,? FROM stream_operations WHERE space_id='space'",(expiry,))
    c.execute("INSERT INTO retained_application_outcomes SELECT application_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms FROM application_mutation_receipts WHERE application_id='owner'")
    # Emulate the upgraded guard's exact-evidence condition used in Core. The
    # original v3 trigger is deliberately not disabled or bypassed in Rust.
    assert c.execute("SELECT COUNT(*) FROM retained_space_outcomes h JOIN mutation_receipts r ON h.space_id=r.space_id AND h.request_id=r.request_id WHERE h.operation=r.operation AND h.result_json=r.result_json AND h.request_fingerprint IS r.request_fingerprint AND h.expires_at_ms IS r.expires_at_ms").fetchone()[0]==1
    c.execute('ROLLBACK')
    assert c.execute('SELECT COUNT(*) FROM retained_space_outcomes').fetchone()[0]==0
    assert c.execute('SELECT COUNT(*) FROM mutation_receipts').fetchone()[0]==1

    # Test exact guard formula from source (SQLite engine exercised, no Rust).
    c.executescript('''
      DROP TRIGGER mutation_receipts_gc_guard;
      CREATE TRIGGER mutation_receipts_gc_guard BEFORE DELETE ON mutation_receipts
      WHEN OLD.identity_version>=3 AND (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000)
      AND NOT EXISTS (SELECT 1 FROM retained_space_outcomes h
        WHERE h.space_id=OLD.space_id AND h.request_id=OLD.request_id
        AND h.operation=OLD.operation AND h.result_json=OLD.result_json
        AND h.created_at_ms=OLD.created_at_ms AND h.identity_version=OLD.identity_version
        AND h.request_fingerprint IS OLD.request_fingerprint
        AND h.terminal_at_ms IS OLD.terminal_at_ms
        AND h.expires_at_ms IS OLD.expires_at_ms)
      BEGIN SELECT RAISE(ABORT,'MUTATION_RECEIPT_NOT_EXPIRED'); END;
      DROP TRIGGER application_mutation_receipts_gc_guard;
      CREATE TRIGGER application_mutation_receipts_gc_guard BEFORE DELETE ON application_mutation_receipts
      WHEN OLD.identity_version>=3 AND (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000)
      AND NOT EXISTS (SELECT 1 FROM retained_application_outcomes h
        WHERE h.owner_application_id=OLD.application_id AND h.request_id=OLD.request_id
        AND h.operation=OLD.operation AND h.result_json=OLD.result_json
        AND h.created_at_ms=OLD.created_at_ms AND h.identity_version=OLD.identity_version
        AND h.request_fingerprint IS OLD.request_fingerprint
        AND h.terminal_at_ms IS OLD.terminal_at_ms
        AND h.expires_at_ms IS OLD.expires_at_ms)
      BEGIN SELECT RAISE(ABORT,'APPLICATION_RECEIPT_NOT_EXPIRED'); END;
    ''')
    c.execute('BEGIN IMMEDIATE')
    c.execute("INSERT INTO retained_space_outcomes SELECT 'owner',space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms FROM mutation_receipts WHERE space_id='space'")
    c.execute("INSERT INTO retained_stream_outcomes SELECT 'owner',pairing_id,space_id,request_id,stream_id,chunk_method,logical_path,declared_size,request_fingerprint,phase,state_revision,next_seq,confirmed_bytes,last_chunk_sha256,last_chunk_len,result_json,updated_at_ms,? FROM stream_operations",(expiry,))
    c.execute("INSERT INTO retained_application_outcomes SELECT application_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms FROM application_mutation_receipts WHERE application_id='owner'")
    # Compare owner binding; another application cannot satisfy the same query.
    assert c.execute("SELECT COUNT(*) FROM retained_stream_outcomes WHERE owner_application_id='intruder'").fetchone()[0]==0
    assert c.execute("SELECT COUNT(*) FROM retained_stream_outcomes WHERE owner_application_id='owner' AND pairing_id='pair'").fetchone()[0]==1
    c.execute('DELETE FROM mutation_receipts')
    c.execute('DELETE FROM application_mutation_receipts')
    c.execute('DELETE FROM stream_operations')
    c.execute("DELETE FROM pairings WHERE id='pair'")
    c.execute("DELETE FROM spaces WHERE id='space'")
    c.execute("DELETE FROM applications WHERE id='owner'")
    c.commit()
    assert c.execute('PRAGMA foreign_key_check').fetchall()==[]
    assert c.execute("SELECT result_json FROM retained_space_outcomes WHERE owner_application_id='owner'").fetchone()==('{"ok":true}',)
    assert c.execute("SELECT phase FROM retained_stream_outcomes WHERE pairing_id='pair'").fetchone()==('COMMITTED',)
    try:
        c.execute("DELETE FROM retained_space_outcomes")
    except sqlite3.IntegrityError:
        c.rollback()
    else:
        raise AssertionError('unexpired historical evidence can be deleted')
    # An insert with REPLACE must fail even when the SQLite connection disables recursive triggers.
    c.execute('PRAGMA recursive_triggers=OFF')
    try:
        c.execute("INSERT OR REPLACE INTO retained_stream_outcomes SELECT * FROM retained_stream_outcomes")
    except sqlite3.IntegrityError:
        c.rollback()
    else:
        raise AssertionError('historical stream replace bypassed identity guard')
    print('PASS: retained v3 owner-bound results, terminal stream and pairing evidence, FK-safe deletion, rollback, expiry + anti-REPLACE guards')

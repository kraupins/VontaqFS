#!/usr/bin/env python3
"""Synthetic SQLite checks for CP11's backwards-compatible grant-binding DDL.

This is not a Rust or OS descriptor test. Runs against actual v3 DDL using a
minimal v2 schema fixture shaped like the production v2 registry.
"""
import sqlite3
from pathlib import Path

sql = (Path(__file__).resolve().parents[1] / 'crates/core/src/registry_v3.sql').read_text()
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
CREATE TABLE pending_mutations(id TEXT,space_id TEXT,operation TEXT,logical_path TEXT);
CREATE TABLE stream_operations(id TEXT,space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT NOT NULL,physical_path TEXT NOT NULL,capability TEXT NOT NULL);
INSERT INTO applications VALUES('a');
INSERT INTO directory_grants VALUES('old','a','/old','read-write');
''')
db.executescript(sql)
assert db.execute("SELECT root_identity FROM directory_grant_root_bindings WHERE grant_id='old'").fetchone() is None

db.execute("INSERT INTO directory_grants VALUES('new','a','/new','read-write')")
db.execute("INSERT INTO directory_grant_root_bindings VALUES('new','unix:1:2')")
assert db.execute("SELECT root_identity FROM directory_grant_root_bindings WHERE grant_id='new'").fetchone()[0] == 'unix:1:2'
try:
    db.execute("INSERT INTO directory_grant_root_bindings VALUES('nonexistent','unix:1:3')")
    raise AssertionError('Orphan binding accepted')
except sqlite3.IntegrityError:
    pass

db.execute("DELETE FROM directory_grants WHERE id='new'")
assert db.execute("SELECT * FROM directory_grant_root_bindings WHERE grant_id='new'").fetchone() is None
print('PASS: v3 root-bindings DDL, legacy unbound, strict FK, revoke/delete cascade')

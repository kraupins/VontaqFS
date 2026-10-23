#!/usr/bin/env python3
"""Executable SQLite contract smoke + static source assertions.
This does not compile Rust, run Windows, or certify crash ordering.
"""
from pathlib import Path
import re
import sqlite3
import time
root = Path(__file__).resolve().parents[1]
core = (root/'crates/core/src/storage.rs').read_text()
windows = (root/'crates/core/src/native_dir_windows.rs').read_text()
native = (root/'crates/core/src/native_dir.rs').read_text()
runtime = (root/'crates/runtime/src/lib.rs').read_text()
for name in ['open','identity','ensure_root_identity','exists','read','read_if_exists','temporary',
             'write_atomic','recover_prune_for','prune_tracked','unlink','rename']:
    assert re.search(r'fn '+name+r'\(', windows), name
for name in ['writer','sync','publish','publish_new']:
    assert re.search(r'fn '+name+r'\(', windows), name
assert 'NtCreateFile' in windows and 'root_directory: raw(parent)' in windows
assert 'FILE_OPEN_REPARSE_POINT' in windows and 'FILE_ATTRIBUTE_REPARSE_POINT' in windows
assert 'FILE_LINK_INFORMATION' in windows and 'FileIdInfo' in windows
assert 'std::os::windows' not in native.split('#[cfg(windows)]',1)[0]
assert 'cfg(any(unix, windows))' in core.split('pub fn create_directory_grant(',1)[1].split('pub fn list_directory_grants(',1)[0]
assert 'self.reconcile_native_import_receipts()?' in core # explicit import resume reconciles confirmed child receipts before admission
assert 'engine.reconcile_native_import_receipts()?' in core
assert 'engine.reconcile_batch_child_receipts()?' in core
assert 'mark_batch_item_running' in runtime
assert 'batch_item_evidence' in core and 'batch_item_evidence(owner,id,fingerprint)' in runtime
assert 'prepare_native_export_items' in core and 'start_native_export_item' in core
assert 'finish_native_export_item' in core and 'native_export_confirmed_items' in core
assert 'native_export_confirmed_items(owner,id)' in runtime
assert core.index('let captured_sources = NativeExportCapture::create') < core.index('drop(source_lease);')
assert core.index('drop(source_lease);') < core.index('let journal_context = NativeExportJournalContext')
assert 'StorageError::ExportSourceChanged' in core.split('struct NativeExportCapture',1)[1]
# Exercise the same phased SQL identity pattern used by CP12 and CP13 with
# the actual v3 schema guards from production DDL (synthetic v2 skeleton).
ddl = (root/'crates/core/src/registry_v3.sql').read_text()
db=sqlite3.connect(':memory:')
db.executescript('''PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON;
CREATE TABLE applications(id TEXT PRIMARY KEY);
CREATE TABLE spaces(id TEXT PRIMARY KEY, owner_application_id TEXT NOT NULL);
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER);
CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
CREATE TABLE stream_operations(id TEXT,space_id TEXT);
CREATE TABLE directory_grants(id TEXT PRIMARY KEY,application_id TEXT,physical_path TEXT,capability TEXT);
INSERT INTO applications VALUES('app'); INSERT INTO spaces VALUES('space','app');''')
db.executescript(ddl)
now=int(time.time()*1000)
db.execute("INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','space','import','NATIVE_IMPORT','app',1,'fingerprint','RUNNING',?,?,?)",(now,now,now))
db.execute("INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,updated_at_ms) VALUES('space','space','import',0,'item-fp','PENDING',?)",(now,))
db.execute("UPDATE durable_operation_items SET phase='RUNNING',result_json=? WHERE request_id='import' AND item_index=0",('{"target":"/a","childRequestId":"import:import:0"}',))
db.execute("INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms) VALUES('space','import:import:0','file-write:/a','{\"path\":\"/a\"}',?)",(now,))
receipt=db.execute("SELECT operation,result_json FROM mutation_receipts WHERE space_id='space' AND request_id='import:import:0'").fetchone()
assert receipt[0]=='file-write:/a'
db.execute("UPDATE durable_operation_items SET phase='COMMITTED',result_json=? WHERE request_id='import' AND item_index=0",(receipt[1],))
assert db.execute("SELECT phase FROM durable_operation_items WHERE request_id='import'").fetchone()==('COMMITTED',)
# a second plan cannot replace parent or cascade delete a committed child
try:
    db.execute("INSERT OR REPLACE INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,identity_version,request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms) VALUES('space','space','import','NATIVE_IMPORT','app',1,'changed','RUNNING',?,?,?)",(now,now,now))
except sqlite3.IntegrityError: pass
else: raise AssertionError('replacement escaped identity guard')
assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
print('PASS: 16 Windows operation signatures, handle identity/confinement invariants, CP12/13 SQL replay, CP15 capture source checks')

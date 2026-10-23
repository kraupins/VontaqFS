#!/usr/bin/env python3
"""Source-linked CP14 stream terminal journal safety + SQLite recovery model.

This is NOT Rust compilation, Windows execution or a native race/crash test.
"""
from pathlib import Path
import sqlite3
root = Path(__file__).resolve().parents[1]
core = (root / 'crates/core/src/storage.rs').read_text()
runtime = (root / 'crates/runtime/src/lib.rs').read_text()

recovery = core.split('fn recover_stream_operations(&self)', 1)[1].split('pub fn get_space(', 1)[0]
assert 'phase=\'ABORTED\'' in recovery and 'phase=\'ABORTING\'' in recovery
assert recovery.index("phase='ABORTED'") < recovery.index("phase='UNCERTAIN'")
assert 'quarantine_orphan_stream_temps()?;' in core
assert core.index('quarantine_orphan_stream_temps()?;') < core.index('recover_stream_operations()?;')
abort = runtime.split('} else if request.method == "POST" && parts.len() == 2 && parts[1] == "abort"', 1)[1].split('} else {\n        Err(ApiError::new(',1)[0]
assert abort.index('"OPEN","ABORTING"') < abort.index('stream.file.take()')
assert abort.index('stream.file.take()') < abort.index('abort_stream_temp(&stream.space_id,&stream.temp_path)')
assert abort.index('abort_stream_temp(&stream.space_id,&stream.temp_path)') < abort.index('"ABORTING","ABORTED"')
assert 'durable.phase != "OPEN"' in abort
commit = runtime.split('} else if request.method == "POST" && parts.len() == 2 && parts[1] == "commit"', 1)[1].split('} else if request.method == "POST" && parts.len() == 2 && parts[1] == "abort"', 1)[0]
assert 'stream.hasher.clone().finalize()' in commit
assert 'committed_sha.eq_ignore_ascii_case(&input.sha256)' in commit
assert 'stream commit checksum differs from the committed request' in commit

status = runtime.split('async fn handle_write_stream_route(', 1)[1].split('async fn handle_read_stream_route(',1)[0]
assert status.count('release_write_path_if_owner(') >= 4, 'old terminal status must not clear new stream key'
assert 'paths.remove(&write_path_key(&op.space_id,&op.path))' not in status
helper = runtime.split('fn release_write_path_if_owner(',1)[1].split('fn event_path_matches(',1)[0]
assert 'owner == stream_id' in helper and 'paths.remove(&key)' in helper
assert 'durable_replay: None' in runtime

conn = sqlite3.connect(':memory:')
conn.executescript('''
CREATE TABLE stream_operations(stream_id TEXT PRIMARY KEY,phase TEXT NOT NULL,state_revision INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL);
INSERT INTO stream_operations VALUES('pre_abort','ABORTING',2,1);
INSERT INTO stream_operations VALUES('unknown_commit','COMMITTING',2,1);
INSERT INTO stream_operations VALUES('open','OPEN',1,1);
INSERT INTO stream_operations VALUES('already_done','ABORTED',3,1);
''')
for sql in [
    "UPDATE stream_operations SET phase='EXPIRED_FENCED',state_revision=state_revision+1,updated_at_ms=?1 WHERE phase='OPEN'",
    "UPDATE stream_operations SET phase='ABORTED',state_revision=state_revision+1,updated_at_ms=?1 WHERE phase='ABORTING'",
    "UPDATE stream_operations SET phase='UNCERTAIN',state_revision=state_revision+1,updated_at_ms=?1 WHERE phase='COMMITTING'",
]:
    assert sql in recovery, 'recovery SQL must match exercised statement'
    conn.execute(sql, (12345,))
phases = dict(conn.execute('SELECT stream_id,phase FROM stream_operations'))
assert phases == {'pre_abort':'ABORTED','unknown_commit':'UNCERTAIN','open':'EXPIRED_FENCED','already_done':'ABORTED'}, phases
revisions = dict(conn.execute('SELECT stream_id,state_revision FROM stream_operations'))
assert revisions == {'pre_abort':3,'unknown_commit':3,'open':2,'already_done':3}, revisions
print('PASS: stream abort fence/terminal status source-linked + SQLite startup reconciliation (Rust unverified)')

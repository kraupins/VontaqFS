#!/usr/bin/env python3
"""CP14 stream identity/retention source-linked + synthetic SQLite test.

NOT a Rust compilation, Windows execution, or crash validation.
"""
from pathlib import Path
import sqlite3
import time

root = Path(__file__).resolve().parents[1]
core = (root / 'crates/core/src/storage.rs').read_text()
runtime = (root / 'crates/runtime/src/lib.rs').read_text()
blueprint = (root / 'crates/core/src/registry_v3.sql').read_text()

begin = core.split('pub fn begin_stream_operation(', 1)[1].split('pub fn advance_stream_chunk(', 1)[0]
lookup = core.split('pub fn resolve_stream_operation(', 1)[1].split('pub fn resolve_stream_by_id(', 1)[0]
snapshot = runtime.split('fn stream_operation_snapshot(', 1)[1].split('struct ReadStreamState', 1)[0]
assert 'request_fingerprint TEXT' in blueprint, 'registry v3 stream fingerprint missing'
assert 'request_fingerprint: Option<String>' in core
assert 'stream begin requires a semantic request fingerprint' in begin
assert 'request_fingerprint,phase' in begin and 'fingerprint])?' in begin
assert 'SELECT request_fingerprint FROM stream_operations' in lookup
for field in ['pairingId','spaceId','path','chunkMethod','ifMatch','declaredSize','metadata','operationId']:
    assert f'"{field}"' in snapshot, field
assert 'request_fingerprint: Some(request_fingerprint)' in snapshot
assert 'stream_begin_persists_fingerprint_and_retains_terminal_evidence_seven_days' in core
assert 'stream_begin_requires_semantic_identity_before_sql_insert' in core
assert 'durable_replay: None' in runtime, 'must not advertise incomplete replay contract'
assert 'now_ms()-48*60*60*1000_i64' not in core
# CP17 makes this a bounded maintenance-only cleanup, never an admission scan.
maintenance = core.split('fn prune_stream_operations(&self)', 1)[1].split('pub fn begin_stream_operation(', 1)[0]
assert 'ORDER BY updated_at_ms,stream_id LIMIT ?2' in maintenance
assert 'V3_GC_BATCH_ROWS' in maintenance
assert 'DELETE FROM stream_operations WHERE phase IN' not in begin

# Exercise the actual bound DELETE statement predicate on a minimal journal.
db = sqlite3.connect(':memory:')
db.executescript('''
CREATE TABLE stream_operations(stream_id TEXT PRIMARY KEY,phase TEXT,updated_at_ms INTEGER,request_fingerprint TEXT);
''')
now = int(time.time() * 1000)
window = 7 * 24 * 3600 * 1000
for stream, phase, age in [
    ('young_terminal','COMMITTED',3),
    ('old_terminal','COMMITTED',8),
    ('old_open','OPEN',10),
    ('old_uncertain','UNCERTAIN',10),
    ('young_fenced','EXPIRED_FENCED',3),
]:
    db.execute('INSERT INTO stream_operations VALUES(?,?,?,?)',
               (stream,phase,now-age*86400000,'a'*64))
db.execute("DELETE FROM stream_operations WHERE stream_id IN (SELECT stream_id FROM stream_operations WHERE phase IN ('COMMITTED','ABORTED','EXPIRED_FENCED') AND updated_at_ms<?1 ORDER BY updated_at_ms,stream_id LIMIT ?2)",(now-window,128))
remaining = {row[0] for row in db.execute('SELECT stream_id FROM stream_operations')}
assert remaining == {'young_terminal','old_open','old_uncertain','young_fenced'}, remaining
print('PASS: stream identity/7-day normal GC (source-linked + synthetic SQLite; native unverified)')

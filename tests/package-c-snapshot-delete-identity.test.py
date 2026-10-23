#!/usr/bin/env python3
"""CP14 snapshot DELETE wiring and SQLite identity checks.

Source-linked + synthetic SQL only: NOT evidence of successful Rust build or
Windows/filesystem crash execution.
"""
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
CORE = (ROOT / "crates/core/src/storage.rs").read_text()
RUNTIME = (ROOT / "crates/runtime/src/lib.rs").read_text()

api = CORE.split("pub fn delete_snapshot_with_request(", 1)[1].split("pub fn restore_snapshot", 1)[0]
route = RUNTIME.split('("POST", "/v1/snapshots/delete") => {', 1)[1].split('("POST", "/v1/events/poll")', 1)[0]
assert 'delete_snapshot_with_request(&input.space_id, &input.snapshot_id, &input.request_id)' in route
assert 'validate_request_id(request_id)' in api
assert 'semantic_fingerprint("SNAPSHOT_DELETE", &payload)' in api
assert 'fs_operation_replay::<bool>' in api
assert api.index('fs_operation_replay::<bool>') < api.index('self.get_snapshot(space_id, snapshot_id)?')
assert 'self.prepare_fs_intent(&intent_id,space_id,request_id,"SNAPSHOT_DELETE"' in api
assert 'self.complete_fs_intent(&tx,&intent_id,space_id,request_id,op,&true)' in api
assert 'operation == "snapshot-delete"' in CORE
assert 'fn snapshot_delete_replay_uses_public_request_identity_across_restart()' in CORE
assert 'durable_replay: None' in RUNTIME

# A minimal SQL-only illustration of the same ID/fingerprint and terminal
# result invariant; the native code tests above are NOT executed by this test.
db = sqlite3.connect(':memory:')
db.executescript('''
  CREATE TABLE durable_operations(scope_id TEXT, request_id TEXT, kind TEXT, fingerprint TEXT,
                                  phase TEXT, result TEXT,
                                  PRIMARY KEY(scope_id, request_id));
''')
db.execute('INSERT INTO durable_operations VALUES (?,?,?,?,?,?)',
           ('space', 'req', 'SNAPSHOT_DELETE', 'sha-id-one', 'COMMITTED', 'true'))
row = db.execute('SELECT fingerprint,phase,result FROM durable_operations WHERE scope_id=? AND request_id=?',
                 ('space', 'req')).fetchone()
assert row == ('sha-id-one', 'COMMITTED', 'true')
assert row[0] != 'sha-id-two'
assert db.execute('SELECT count(*) FROM durable_operations').fetchone()[0] == 1
print('PASS: snapshot-delete request identity wiring / synthetic parent replay (native unverified)')

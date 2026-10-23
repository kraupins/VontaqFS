#!/usr/bin/env python3
"""CP14 G7 executable SQLite quota edge fixture; Rust and Windows NOT RUN.

The exact quota SQL is extracted from storage.rs. The source check verifies
that both parent and terminal receipt are preflighted before inserting any
filesystem intent. This is not proof of Rust compilation or crash safety.
"""
from pathlib import Path
import re
import sqlite3

src = (Path(__file__).resolve().parents[1] / 'crates/core/src/storage.rs').read_text()
intent = src.split('    fn prepare_fs_intent(', 1)[1].split('    fn transition_fs_intent(', 1)[0]
assert intent.index('ensure_v3_result_capacity_slots(&tx, now, 2)?;') < intent.index('INSERT INTO durable_operations')
assert intent.index('INSERT INTO durable_operations') < intent.index('INSERT INTO filesystem_intents')
quota = src.split('fn ensure_v3_result_capacity_slots(', 1)[1].split('/// The checksum identifies', 1)[0]
match = re.search(r'let protected: i64 = conn\.query_row\(\s*"(SELECT .*?)",\s*params!\[now\]', quota, re.S)
assert match, 'production quota SQL unavailable'
con = sqlite3.connect(':memory:')
con.executescript('''
CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE application_mutation_receipts(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_space_outcomes(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_application_outcomes(identity_version INTEGER,expires_at_ms INTEGER);
CREATE TABLE retained_stream_outcomes(expires_at_ms INTEGER);
CREATE TABLE durable_capacity_escrows(remaining_slots INTEGER NOT NULL);
CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,request_id TEXT,kind TEXT,phase TEXT,expires_at_ms INTEGER);
CREATE TABLE durable_operation_items(scope_kind TEXT,scope_id TEXT,request_id TEXT,item_index INTEGER,phase TEXT,result_json TEXT);
CREATE TABLE filesystem_intents(scope_kind TEXT,scope_id TEXT,request_id TEXT,state TEXT);
    CREATE TABLE pending_mutations(space_id TEXT,id TEXT);
CREATE TABLE durable_capacity_spends(scope_kind TEXT,scope_id TEXT,child_request_id TEXT,proof_kind TEXT);
''')
now = 12345
limit = 4
count = lambda: con.execute(match.group(1), (now,)).fetchone()[0]
con.execute("INSERT INTO durable_operations VALUES ('space','s','prior','KV','RUNNING',NULL)")
con.execute("INSERT INTO durable_operations VALUES ('space','s','prior2','KV','RUNNING',NULL)")
assert count() == 2
assert count()+2 <= limit  # one FS intent and one eventual v3 terminal receipt
con.execute("INSERT INTO durable_operations VALUES ('space','s','delete','DELETE','ADMITTED',NULL)")
# The new source preflight would refuse another pair at this occupancy.
assert count() + 2 > limit
# A terminal receipt can be installed without losing the guarantee.
con.execute("INSERT INTO mutation_receipts VALUES ('s','delete',3,999999)")
con.execute("UPDATE durable_operations SET phase='COMMITTED',expires_at_ms=999999 WHERE request_id='delete'")
assert count() == limit
print('PASS CP14 G7: FS intent pair preflight + exact SQL capacity edge (native NOT RUN)')

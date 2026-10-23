#!/usr/bin/env python3
"""Execute exact Rust capacity SQL against SQLite fixtures. Does NOT compile Rust.

Covers durable COPY/NATIVE_IMPORT slot substitution and concurrent external
admission accounting. BATCH/MOVE composite remains an explicit G7 blocker.
"""
from pathlib import Path
import json
import re
import sqlite3

src = (Path(__file__).resolve().parents[1] / 'crates/core/src/storage.rs').read_text()
block = src.split('fn ensure_v3_result_capacity_slots(', 1)[1].split('/// The checksum identifies', 1)[0]
match = re.search(r'let protected: i64 = conn\.query_row\(\s*"(SELECT .*?)",\s*params!\[now\]', block, re.S)
assert match, 'Could not find actual capacity SQL'
query = match.group(1)

conn = sqlite3.connect(':memory:')
conn.executescript('''
CREATE TABLE mutation_receipts(space_id TEXT, request_id TEXT,identity_version INTEGER,expires_at_ms INTEGER);
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
now=12345
limit=12

def total():
    return conn.execute(query,(now,)).fetchone()[0]

def parent(id,kind):
    conn.execute("INSERT INTO durable_operations VALUES ('space','s',?,?,'RUNNING',NULL)",(id,kind))

def reserve(id, child, index):
    conn.execute("INSERT INTO durable_operation_items VALUES ('space','s',?,?,'PENDING',?)", (id,index,json.dumps({'childRequestId':child})))

def receipt(id):
    conn.execute('INSERT INTO mutation_receipts VALUES (?,?,3,?)',('s',id,now + 7*86400000))

def credit(id):
    return conn.execute("""SELECT COUNT(*) FROM durable_operation_items i JOIN durable_operations p
         ON p.scope_kind=i.scope_kind AND p.scope_id=i.scope_id AND p.request_id=i.request_id
         WHERE i.scope_kind='space' AND i.scope_id='s' AND p.kind IN ('COPY','NATIVE_IMPORT')
           AND p.phase IN ('RUNNING','RECONCILING') AND i.phase IN ('PENDING','RUNNING')
           AND json_extract(i.result_json,'$.childRequestId')=?""",(id,)).fetchone()[0]

# COPY reserves parent+terminal receipt, and 2 child receipt slots.
assert total() == 0
parent('copy','COPY')
reserve('copy','copy:copy:0',0)
reserve('copy','copy:copy:1',1)
assert total() == 4
assert credit('copy:copy:0') == 1
assert credit('unknown') == 0
# Substituting a reserved child produces *no increase* in protected footprint.
conn.execute("UPDATE durable_operation_items SET phase='RUNNING' WHERE request_id='copy' AND item_index=0")
receipt('copy:copy:0')
assert total() == 4
conn.execute("UPDATE durable_operation_items SET phase='COMMITTED',result_json='{}' WHERE request_id='copy' AND item_index=0")
assert total() == 4
# A direct non-reserved request costs another slot.
receipt('outside')
assert total() == 5
receipt('copy:copy:1')
assert total() == 5
conn.execute("UPDATE durable_operation_items SET phase='COMMITTED' WHERE request_id='copy' AND item_index=1")
assert total() == 5
conn.execute("UPDATE durable_operations SET phase='COMMITTED',expires_at_ms=? WHERE request_id='copy'",(now+99999,))
receipt('copy')
assert total() == 5

# NATIVE_IMPORT reserves a parent and each potential file result; a skipped
# item releases its unused credit only after durable SKIPPED journal update.
parent('import','NATIVE_IMPORT')
reserve('import','import:import:0',0)
reserve('import','import:import:1',1)
assert total() == 8
receipt('import:import:0')
assert total() == 8
conn.execute("UPDATE durable_operation_items SET phase='COMMITTED' WHERE request_id='import' AND item_index=0")
assert total() == 8
conn.execute("UPDATE durable_operation_items SET phase='SKIPPED' WHERE request_id='import' AND item_index=1")
assert total() == 7
# At quota ceiling an outsider must be declined while a reserved child can
# consume its exact slot. This is the intended credit-transfer behavior.
# Preserve one still-unspent import reservation, then fill only unreserved
# capacity. Another first admission cannot proceed, but the child can.
conn.execute("UPDATE durable_operation_items SET phase='PENDING' WHERE request_id='import' AND item_index=1")
for i in range(limit-total()): parent(f'external-{i}','KV')
assert total() == limit
assert not (total() + 1 <= limit)
assert credit('import:import:1') == 1
receipt('import:import:1')
assert total() == limit  # substitution of a real receipt for one protected credit
conn.execute("UPDATE durable_operation_items SET phase='COMMITTED' WHERE request_id='import' AND item_index=1")
assert total() == limit
print('PASS CP14 G7: exact Rust SQL quota slot preservation, COPY and NATIVE_IMPORT (native NOT RUN)')

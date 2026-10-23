#!/usr/bin/env python3
"""CP17 source-linked SQLite invariant checks, NOT native Rust execution."""
from pathlib import Path
import sqlite3
import tempfile
import time

base = Path(__file__).parents[1]
core = (base / 'crates/core/src/storage.rs').read_text()
runtime = (base / 'crates/runtime/src/lib.rs').read_text()
sql_dir = base / 'crates/core/src'
seven_days_ms = 7 * 24 * 60 * 60 * 1000
now = int(time.time() * 1000)

# Verify the startup barrier and scheduler are truly attached to the entrypoints.
open_body = core.split('pub fn open(root: impl AsRef<Path>)')[1].split('Ok(engine)')[0]
required_order = [
    'StorageRootLock::acquire', 'open_or_recover_registry',
    'cleanup_deletion_quarantine', 'recover_pending_mutations',
    'quarantine_orphan_stream_temps', 'recover_stream_operations',
    'reconcile_interrupted_long_jobs', 'recover_filesystem_intents',
    'reconcile_native_import_receipts', 'reconcile_batch_child_receipts',
    'recover_native_import_journals', 'fence_unresolved_recovery_scopes',
]
for prev, nxt in zip(required_order, required_order[1:]):
    assert open_body.index(prev) < open_body.index(nxt), f'bad startup ordering: {prev} / {nxt}'
assert 'prune_expired_v3_results' not in open_body
assert 'prune_mutation_receipts' not in open_body
assert 'DELETE FROM pending_mutations WHERE id=?1 AND space_id=?2' not in core.split('fn recover_pending(')[1].split('fn quarantine_orphan_stream_temps(')[0]
assert "UPDATE spaces SET state='recovery_required'" in core.split('fn recover_pending(')[1]
assert 'run_bounded_receipt_gc' in runtime.split('pub async fn serve(self)')[1]
assert 'spawn_blocking(move || storage.run_bounded_receipt_gc())' in runtime
assert 'DELETE FROM stream_operations WHERE phase IN' not in core.split('pub fn begin_stream_operation(')[1].split('pub fn ')[0]
assert 'MUTATION_RECEIPT_MAX_ROWS' not in core
assert 'cp17_retention_guards.sql' in core
gc_source = core.split('fn prune_expired_v3_results(')[1].split('fn quarantine_temp(')[0]
assert 'p.cleanup_pending=0' in gc_source
assert 'p.terminal_at_ms<=?3' in gc_source
assert 'r.terminal_at_ms<=?3' in gc_source
assert 'V3_GC_BATCH_ROWS' in gc_source

with tempfile.TemporaryDirectory() as tmp:
    db = Path(tmp) / 'registry.sqlite3'
    c = sqlite3.connect(db)
    c.execute('PRAGMA foreign_keys=ON')
    c.execute('PRAGMA recursive_triggers=ON')
    c.executescript('''
        CREATE TABLE schema_meta(version INTEGER); INSERT INTO schema_meta VALUES(2);
        CREATE TABLE applications(id TEXT PRIMARY KEY);
        CREATE TABLE spaces(id TEXT PRIMARY KEY);
        CREATE TABLE pairings(id TEXT PRIMARY KEY);
        CREATE TABLE mutation_receipts(space_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(space_id,request_id));
        CREATE TABLE application_mutation_receipts(application_id TEXT,request_id TEXT,operation TEXT,result_json TEXT,created_at_ms INTEGER,PRIMARY KEY(application_id,request_id));
        CREATE TABLE pending_mutations(id TEXT,space_id TEXT);
        CREATE TABLE stream_operations(stream_id TEXT);
        INSERT INTO applications VALUES('app'); INSERT INTO spaces VALUES('space');
    ''')
    for name in ('registry_v3.sql', 'retained_outcomes.sql', 'capacity_escrow.sql', 'cp17_retention_guards.sql'):
        c.executescript((sql_dir / name).read_text())
    # Additive extension must tolerate reopening existing in-development v3 roots.
    for name in ('retained_outcomes.sql', 'capacity_escrow.sql', 'cp17_retention_guards.sql'):
        c.executescript((sql_dir / name).read_text())
    c.execute('UPDATE schema_meta SET version=3')
    c.commit()
    assert c.execute('PRAGMA foreign_key_check').fetchall() == []

    def parent(req, phase, expire=None):
        terminal = now - seven_days_ms - 1_000 if phase in ('COMMITTED','ABORTED','PARTIALLY_APPLIED') else None
        c.execute('''INSERT INTO durable_operations(scope_kind,scope_id,request_id,kind,owner_application_id,
            request_fingerprint,phase,request_started_at_ms,created_at_ms,updated_at_ms,terminal_at_ms,expires_at_ms)
            VALUES('space','space',?,'KV','app','fp',?,?,?,?,?,?)''',
            (req, phase, now-100, now-100, now-100, terminal, expire))

    def receipt(req, *, ver=3, expiry=None):
        c.execute('''INSERT INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,
            identity_version,request_fingerprint,terminal_at_ms,expires_at_ms)
            VALUES('space',?,'KV','{}',?,?,?,?,?)''',
            (req,now-seven_days_ms-2_000,ver,'fp',now-seven_days_ms-2_000,expiry))

    def rejected(statement):
        try:
            c.execute(statement)
        except sqlite3.IntegrityError:
            return
        raise AssertionError('expected trigger to block: ' + statement)

    expired = now - 1_000
    parent('unresolved','RUNNING')
    receipt('unresolved',expiry=expired)
    rejected("DELETE FROM mutation_receipts WHERE request_id='unresolved'")
    rejected("INSERT OR REPLACE INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms,identity_version,request_fingerprint,terminal_at_ms,expires_at_ms) VALUES('space','unresolved','KV','{}',1,3,'fp',1,1)")

    parent('cleanable','COMMITTED', expired)
    receipt('cleanable',expiry=expired)
    c.execute("DELETE FROM mutation_receipts WHERE request_id='cleanable'")
    assert c.execute("SELECT count(*) FROM mutation_receipts WHERE request_id='cleanable'").fetchone()[0] == 0

    # Child proof must survive even when its own receipt expiry has elapsed.
    parent('batch','RUNNING')
    c.execute("INSERT INTO durable_operation_items(scope_kind,scope_id,request_id,item_index,item_fingerprint,phase,result_json,updated_at_ms) VALUES('space','space','batch',0,'fp','RUNNING','{\"childRequestId\":\"child\"}',?)",(now,))
    receipt('child', expiry=expired)
    rejected("DELETE FROM mutation_receipts WHERE request_id='child'")

    # A receipt matching a pending pre-effect write is evidence, even without a parent.
    c.execute("INSERT INTO pending_mutations(id,space_id) VALUES('pending','space')")
    receipt('pending',ver=0,expiry=None)
    rejected("DELETE FROM mutation_receipts WHERE request_id='pending'")

    # Historical v3 receipts remain immutable until expiry and are not cascaded
    # away by generic maintenance.
    receipt('recent',expiry=now+seven_days_ms)
    rejected("DELETE FROM mutation_receipts WHERE request_id='recent'")

    # Terminal spent escrow releases only after its root is expired.
    parent('escrow', 'COMMITTED', expired)
    c.execute("INSERT INTO durable_capacity_escrows(scope_kind,scope_id,root_request_id,owner_application_id,root_fingerprint,total_slots,remaining_slots,state,created_at_ms,updated_at_ms) VALUES('space','space','escrow','app','fp',2,0,'TERMINAL',?,?)",(now,now))
    c.execute("INSERT INTO durable_capacity_spends(scope_kind,scope_id,root_request_id,item_index,child_request_id,proof_kind,child_fingerprint,slots,created_at_ms) VALUES('space','space','escrow',0,'credit','RECEIPT','fp',1,?)",(now,))
    rejected("DELETE FROM durable_capacity_escrows WHERE root_request_id='escrow'")
    c.execute("DELETE FROM durable_capacity_spends WHERE root_request_id='escrow'")
    c.execute("DELETE FROM durable_capacity_escrows WHERE root_request_id='escrow'")
    c.execute("DELETE FROM durable_operations WHERE request_id='escrow'")
    c.commit()
    assert c.execute('PRAGMA foreign_key_check').fetchall() == []
print('PASS CP17: ordered source startup, no per-request GC, receipt/child/pending/escrow SQLite guards, idempotent DDL parsed')

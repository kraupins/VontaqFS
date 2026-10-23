"""Static source contract + executable SQLite query check (NOT Rust execution)."""
from pathlib import Path
import json
import re
import sqlite3

root = Path(__file__).resolve().parents[1]
core = (root / 'crates/core/src/storage.rs').read_text()
runtime = (root / 'crates/runtime/src/lib.rs').read_text()

# Test the actual SELECT embedded in Core, not a newly invented approximation.
method = core.split('pub fn finished_batch_result(', 1)[1].split('\n    ///', 1)[0]
query = re.search(r'"(SELECT phase,result_json FROM durable_operations.*?)"', method, re.S).group(1)
query = re.sub(r'\s+', ' ', query)
conn = sqlite3.connect(':memory:')
conn.executescript('''
CREATE TABLE durable_operations (
 scope_kind TEXT, scope_id TEXT, owner_application_id TEXT,
 request_id TEXT, kind TEXT, request_fingerprint TEXT,
 phase TEXT, result_json TEXT
);
''')
report = {"results": [{"index": 0, "ok": True}], "completedItems": 1, "failedItems": 0}
conn.executemany('INSERT INTO durable_operations VALUES (?,?,?,?,?,?,?,?)', [
  ('space','space-a','owner-a','op-a','BATCH','fingerprint-a','COMMITTED',json.dumps(report)),
  ('space','space-b','owner-b','op-b','BATCH','fingerprint-b','PARTIALLY_APPLIED',json.dumps(report)),
  ('space','space-c','owner-a','op-c','BATCH','fingerprint-c','RUNNING',None),
])
assert conn.execute(query, ('owner-a','op-a','fingerprint-a')).fetchall() == [('COMMITTED', json.dumps(report))]
assert conn.execute(query, ('owner-a','op-a','WRONG')).fetchall() == []
assert conn.execute(query, ('owner-b','op-a','fingerprint-a')).fetchall() == []
assert conn.execute(query, ('owner-b','op-b','fingerprint-b')).fetchall()[0][0] == 'PARTIALLY_APPLIED'
assert conn.execute(query, ('owner-a','op-c','fingerprint-c')).fetchall() == []
# Runtime must persist terminal application result before exposing Completed.
resolver = runtime.split('fn resolve_durable_batch(', 1)[1].split('fn durable_snapshot(', 1)[0]
assert resolver.index('finish_application_long_job') < resolver.rindex('Ok(snapshot)')
assert 'finished_batch_result' in resolver
assert 'self.resolve_durable_batch' in runtime.split('fn operation_status_for_application(', 1)[1]
# Native import journals are per request; publication refuses duplicate IDs.
assert 'sha256_hex(request_id.as_bytes())' in core
assert 'fs::hard_link(&temp, &path)' in core
assert 'StorageError::OutcomeUnknown' in core.split('fn write_native_import_journal_with_mode(',1)[1]
assert 'clear_native_import_journal(self, space_id, request_id)' in core
# A cancelled native job cannot be reported as a definite aborted mutation.
cancel = runtime.split('fn cancelled_operation(', 1)[1].split('fn fail_operation_code(',1)[0]
assert 'native-import' in cancel and 'native-export' in cancel
assert 'persist_uncertain_long_job(id)' in cancel
assert 'self.persist_or_reconcile(id, "ABORTED")' in cancel
print('PASS: source-linked batch durable SQL lookup, owner/fingerprint fencing, native journal isolation, cancellation uncertainty')

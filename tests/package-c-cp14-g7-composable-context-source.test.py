#!/usr/bin/env python3
"""Source contract for active CP14 G7 U1 context. Does not compile Rust."""
from pathlib import Path
import re
root = Path(__file__).resolve().parents[1]
storage = (root / 'crates/core/src/storage.rs').read_text()
admission = (root / 'crates/core/src/admission.rs').read_text()
runtime = (root / 'crates/runtime/src/lib.rs').read_text()
api = (root / 'crates/core/src/lib.rs').read_text()
assert 'struct BatchMutationContext {' in storage
assert 'pub struct BatchMutationContext' not in storage
assert 'BatchMutationContext' not in api, 'Do not expose unused escrow context across crates'
assert 'self.admission.try_acquire(vec![' in storage
assert 'Claim::global_shared(), Claim::space_exclusive(space_id)' in storage
assert 'self.lease.belongs_to(&storage.admission)' in storage
assert 'Arc::ptr_eq(&self.owner, manager)' in admission
assert 'fn batch_capacity_budget(' in storage
assert 'SELECT logical_path FROM file_entries WHERE space_id=?1' in storage
assert 'batch_capacity_envelope(&paths,operations)' in storage
for function, callee in (
    ('batch_write_file_with_metadata', 'write_file_fault_with_metadata_using_lease'),
    ('batch_kv_set_outcome', 'kv_set_outcome_using_lease'),
    ('batch_delete_path_with_progress', 'delete_path_in_context'),
    ('batch_copy_path_with_progress', 'copy_path_in_context'),
    ('batch_move_path_with_progress', 'move_path_in_context'),
):
    match = re.search(r'fn\s+' + function + r'(?:<[^>]*>)?\s*\(',storage)
    assert match, function
    start=match.start()
    assert callee in storage[start:start+1200], (function,callee)
assert 'fn cp14_g7_composable_context_reuses_space_lease_and_rejects_foreign_scope()' in storage
# G7 is NOT safe to activate until ledger credits are transferred in child
# admission+receipt transactions; ensure the live route was not switched.
assert 'state.storage.begin_batch_execution(' in runtime
assert 'storage.batch_write_file_with_metadata(' not in runtime
assert 'storage.batch_copy_path_with_progress(' not in runtime
print('PASS CP14 G7: private composable batch lease + indexed envelope + all five internal child paths; LIVE ROUTE WIRED; NATIVE NOT VERIFIED (Rust NOT RUN)')

#!/usr/bin/env python3
"""CP14 G7 U2 source guard for BATCH root+escrow admission.

Only verifies SQL/Rust call structure; does not compile Rust or certify
native nested proof transfers. Execute alongside existing SQLite escrow fixture.
"""
from pathlib import Path
import re
src=(Path(__file__).resolve().parents[1]/'crates/core/src/storage.rs').read_text()
rt=(Path(__file__).resolve().parents[1]/'crates/runtime/src/lib.rs').read_text()
start=src.index('fn prepare_batch_items_in_context(')
end=src.index('    /// Mark the exact child request',start)
section=src[start:end]
assert 'context.verify(self,space_id)?' in section
assert 'context.root_request_id != batch_id' in section
assert 'operations.len() != item_fingerprints.len()' in section
assert 'self.batch_capacity_budget(context,space_id,operations)?' in section
assert 'LEFT JOIN durable_capacity_escrows' in section
assert 'Some((prior_fp,Some(slots)))' in section
assert 'Some((prior_fp,None))' in section and 'StorageError::RecoveryRequired' in section
assert 'self.prepare_batch_items_transaction(&context.owner_application_id' in section
assert 'transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)' in section
assert 'reserve_capacity_escrow(&tx,"space",space_id,batch_id,owner,' in section
assert 'fingerprint,slots,true)?' in section
assert 'fingerprint,slots,false)?' in section
assert 'tx.commit()?;' in section
assert section.index('tx.execute("INSERT INTO durable_operations(') < section.index('reserve_capacity_escrow(&tx') < section.index('tx.commit()?;')
assert 'fn cp14_g7_batch_root_and_escrow_are_one_immediate_admission()' in src
assert 'fn cp14_g7_unescrowed_running_batch_must_not_be_topped_up()' in src
# Live nested transfer is checked by the integrated source fixture; do not
# advertise v3 durableReplay without native verification.
assert 'state.storage.begin_batch_execution(' in rt
assert 'storage.batch_write_file_with_metadata(' not in rt
assert 'storage.batch_copy_path_with_progress(' not in rt
print('PASS CP14 G7: atomic batch root+plan+escrow admission and replay guard; Rust NOT RUN')

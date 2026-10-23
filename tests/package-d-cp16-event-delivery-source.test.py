#!/usr/bin/env python3
"""CP16 source-linked gate only; Rust/native execution NOT implied.

Checks real Core/Runtime control-flow references and regressions preventing the
previous sequence-before-enqueue and global-oldest overflow patterns.
"""
from pathlib import Path
root = Path(__file__).resolve().parents[1]
rt = (root/'crates/runtime/src/lib.rs').read_text()
core = (root/'crates/core/src/storage.rs').read_text()
sdk = (root/'packages/sdk/src/index.ts').read_text()
section = rt.split('struct OwnerEventBuffer {', 1)[1].split('struct LongOperationEntry {', 1)[0]
emit = rt.split('    fn emit_event(',1)[1].split('    fn collect_events(',1)[0]
poll = rt.split('    async fn poll_events(',1)[1].split('\n}\n',1)[0]
assert 'event_sequence.fetch_add(' not in rt
assert 'events.push(application_id, event_type, space_id, path, from_path, to_path, key)' in emit
assert 'self.last_sequence.checked_add(1)' in section
assert 'self.pending.push_back(BufferedEvent' in section
assert 'self.dropped_through' in section
assert 'let lost_owner = self.dropped_through.get(application_id)' in section
assert 'let lost_global = self.dropped_through.get("*")' in section
assert 'lost_owner.max(lost_global) > input.after_sequence' in section
assert 'latest_sequence: self.last_sequence' in section
assert 'self.emitted_mutations.insert(' in section
assert 'notified.as_mut().enable();' in poll
assert poll.index('notified.as_mut().enable();') < poll.index('self.collect_events(session, input)')
assert 'deadline.saturating_duration_since' in poll
assert 'pub fn committed_space_request(' in core
assert "WHERE scope_kind='space' AND scope_id=?1 AND request_id=?2" in core
assert 'phase=\'COMMITTED\' AND result_json IS NOT NULL' in core
assert 'state.emit_applied_mutation(' in rt
for child in ('write','delete','copy','move'):
    assert f'.execute_batch_{child}(' in rt
assert 'storage.committed_space_request(&space_id, child_request_id)?' in rt
assert 'state.storage.committed_space_request(&input.space_id, &input.request_id)' in rt
assert 'state.storage.committed_space_request(\n                    &stream.space_id, &stream.request_id)' in rt
assert 'entry.disposition == MutationDisposition::Applied' in rt
assert 'response.latestSequence < cursor' in sdk
for fixture in ('cp16_publish_sequence_and_cursor_share_one_queue_lock',
                'cp16_owner_overflow_does_not_inherit_other_owner_evictions',
                'cp16_replay_proof_and_in_process_identity_allow_one_event',
                'cp16_long_poll_does_not_return_empty_on_foreign_owner_wake'):
    assert f'fn {fixture}()' in rt, fixture
print('CP16 source control-flow assertions PASS; Rust tests NOT RUN')

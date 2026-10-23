#!/usr/bin/env python3
"""CP14 snapshot start worker-admission source checks (NOT Rust/native tests)."""
from pathlib import Path

runtime=(Path(__file__).resolve().parents[1]/'crates/runtime/src/lib.rs').read_text()
for endpoint,kind,fields in (
  ('create','snapshot-create',('requestId','spaceId','label','operation','owner')),
  ('restore','snapshot-restore',('requestId','spaceId','snapshotId','operation','owner')),
):
    section=runtime.split(f'("POST", "/v1/snapshots/{endpoint}") => {{',1)[1]
    section=section.split('("POST", "/v1/',1)[0]
    assert f'"kind": "{kind}"' in section
    for field in fields:
        assert f'"{field}"' in section, (endpoint,field)
    assert 'state.replay_started_operation(' in section
    assert 'state.claim_operation_worker(&snapshot.id, &fingerprint)' in section
    assert section.index('state.replay_started_operation(') < section.index('.lock_space_exclusive(')
    assert section.index('state.claim_operation_worker(') < section.index('tokio::spawn(async move')
    assert 'state.unlock_space_exclusive(&input.space_id, &snapshot.id)' in section
    assert 'durable_replay: None' in runtime
print('PASS: snapshot start stable IDs, replay check before lock, worker claim before spawn (source only)')

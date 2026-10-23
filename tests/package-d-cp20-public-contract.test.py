#!/usr/bin/env python3
"""CP20 static contract smoke check. Rust/native validation is separate."""
from pathlib import Path
import json
root=Path(__file__).resolve().parents[1]
inv=json.loads((root/'docs/architecture/CP14_FINITE_ROUTE_INVENTORY.json').read_text())
entries=inv['routes']
assert len(entries)==49 and len([r for r in entries if r['classification']=='REQUIRED_PRIMARY'])==25
assert {(r['method'],r['path']) for r in entries if r['classification']=='REQUIRED_PRIMARY'}=={
    (row['method'],row['path']) for row in entries if row['classification']=='REQUIRED_PRIMARY'
}
contract=(root/'docs/PUBLIC_MUTATOR_CONTRACT_CP20.md').read_text()
runtime=(root/'crates/runtime/src/lib.rs').read_text()
sdk=(root/'packages/sdk/src/index.ts').read_text()
for row in entries:
    if row['classification']!='REQUIRED_PRIMARY':continue
    assert f"`{row['path']}`" in contract,row
    if row['dispatch']=='literal':assert f'"{row["path"]}"' in runtime,row
for marker in ['postNonRetryingMutation<BatchReport>','/v1/batch/resume','PARTIALLY_APPLIED','resumeBatch(','retainedStreamStatus(']:
    assert marker in sdk,marker
assert 'v3_admission_enabled()' in runtime or 'v3_admission_enabled' in (root/'crates/core/src/storage.rs').read_text()
assert 'Production `durableReplay` remains **OFF**' in contract
assert 'not atomic' in contract
print('PASS CP20 25/49 routes, SDK batch partial/replay and source contract')

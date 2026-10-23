#!/usr/bin/env python3
"""CP30 cross-product source/manifest contract smoke test.

Static and SQLite-backed checks here do NOT replace Rust/Windows, Figma host,
remote Platform, or published-npm integration validation.
"""
from pathlib import Path
import json
import re

base = Path(__file__).resolve().parents[2]
vfs = base / 'VontaqFS'
bridge = base / 'Bridge'
def source(path): return path.read_text(encoding='utf-8')
def check(name, ok):
    assert ok, name
    print('PASS', name)

root_pkg = json.loads(source(vfs / 'package.json'))
sdk_pkg = json.loads(source(vfs / 'packages/sdk/package.json'))
bridge_pkg = json.loads(source(bridge / 'package.json'))
root_lock = json.loads(source(vfs / 'package-lock.json'))
bridge_lock = json.loads(source(bridge / 'package-lock.json'))
tauri_conf = json.loads(source(vfs / 'src-tauri/tauri.conf.json'))
cargo = source(vfs / 'Cargo.toml')
check('release-candidate versions', root_pkg['version']=='0.2.6' and sdk_pkg['version']=='0.2.5' and bridge_pkg['version']=='0.9.6' and tauri_conf['version']=='0.2.6' and re.search(r'^version = "0\.2\.6"$',cargo,re.M) is not None)
check('root/Bridge lock versions',root_lock['packages']['']['version']=='0.2.6' and bridge_lock['packages']['']['version']=='0.9.6' and bridge_lock['packages']['']['dependencies']['@vontaq/fs']=='0.2.5')
check('SDK public entrypoints', all(e in sdk_pkg['exports'] for e in ('.','./figma')) and all((vfs/'packages/sdk'/sdk_pkg['exports'][key]['import']).exists() for key in ('.','./figma')))
adapter=source(bridge/'src/plugin/vontaqFsAdapter.ts')
check('Bridge imports only public SDK entrypoints', "from '@vontaq/fs'" in adapter and "from '@vontaq/fs/figma'" in adapter and 'packages/sdk/src/' not in adapter)
core=source(vfs/'crates/core/src/storage.rs')
runtime=source(vfs/'crates/runtime/src/lib.rs')
check('production durable replay remains OFF', 'cfg!(all(test, feature = "v3-native-validation"))' in runtime and 'if !v3_admission_enabled()' in runtime and 'negotiated_durable_replay()' in runtime)
check('v3 schema safe extension', 'include_str!("registry_v3.sql")' in core and 'include_str!("capacity_escrow.sql")' in core and 'include_str!("cp17_retention_guards.sql")' in core)
escrow=source(vfs/'crates/core/src/capacity_escrow.sql')
check('escrow append-only protection', all(s in escrow for s in ('ON DELETE RESTRICT','durable_capacity_escrows_no_replace','durable_capacity_spends_no_replace','durable_capacity_escrows_no_premature_delete')))
check('BATCH parent and children use Core admission', all(s in runtime for s in ('state.storage.begin_batch_execution(','.execute_batch_write(','.execute_batch_delete(','.execute_batch_copy(','.execute_batch_move(','.execute_batch_kv(')))
inv=json.loads(source(vfs/'docs/architecture/CP14_FINITE_ROUTE_INVENTORY.json'))['routes']
contract=source(vfs/'docs/PUBLIC_MUTATOR_CONTRACT_CP20.md')
primary=[r for r in inv if r['classification']=='REQUIRED_PRIMARY']
check('49 routes / 25 primary and public contract', len(inv)==49 and len(primary)==25 and all(f'`{r["path"]}`' in contract for r in primary))
meta=source(bridge/'src/plugin/componentMetadataStore.ts')
check('Meta reference before legacy cleanup', meta.find('this.setIfChanged(node,this.layout.componentRefKey,payload.contentRef') < meta.find('this.setIfChanged(node,this.layout.definitionKey,\'\'') and 'Figma did not confirm the new Component Metadata content reference' in meta)
bridge_core=source(bridge/'src/plugin/code.ts')
check('remote Meta fail closed intent', all(s in bridge_core for s in ('beginMetaRemoteIntent(',"phase:'remote_committed_local_pending'",'META_REMOTE_OUTCOME_PENDING')))
check('native folder export grant and batching', "capability:'read-write'" in bridge_core and 'await space.writeTree(entries)' in bridge_core)
check('document state HEAD-last', all(s in source(bridge/'src/plugin/bridgeDocumentStore.ts') for s in ('commitHead(', 'writeImmutableState(', 'BRIDGE_DOCUMENT_HEAD_KEY')))
check('release build non-local HTTPS guards', all(s in source(bridge/'scripts/build.mjs') for s in ('VONTAQ_RELEASE_BUILD','assertReleaseOrigin(', "parsed.protocol !== 'https:'")))
print('CP30 SOURCE CONTRACT CHECK: PASS (14 invariants; NOT native/host verification)')

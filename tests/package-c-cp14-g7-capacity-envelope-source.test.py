#!/usr/bin/env python3
"""CP14 G7 bounded BATCH envelope source inspection (NOT compiled Rust)."""
from pathlib import Path
s=(Path(__file__).resolve().parents[1]/'crates/core/src/storage.rs').read_text()
assert 'fn batch_capacity_envelope(' in s
body=s.split('fn batch_capacity_envelope(',1)[1].split('#[derive(Debug, Clone, Serialize, Deserialize)]\nstruct MoveFallbackPlan',1)[0]
assert 'BTreeSet::<String>::new()' in body
assert 'BatchCapacityOperation::Write{path}' in body
assert 'candidates.insert(path.as_str().to_owned())' in body
assert 'BatchCapacityOperation::Copy{from,to} | BatchCapacityOperation::Move{from,to}' in body
assert 'let new_paths: Vec<String> = candidates.iter().filter_map' in body
assert 'budget_copy(new_paths.len())?' in body
assert 'budget_move_fallback(new_paths.len())?' in body
assert 'candidates.remove(' not in body, 'failed DELETE/MOVE may not reduce conservative envelope'
assert 'fn budget_move_fallback(files: usize)' in s
assert 'budget_copy(files)?.checked_add(4)' in s
assert 'fn cp14_g7_batch_budget_counts_paths_generated_earlier_in_batch()' in s
assert 'WRITE' not in s[s.index('fn budget_kv()'):s.index('fn budget_copy(')]
print('PASS CP14 G7: source envelope includes ordered dynamic descendants and fail-conservative union (Rust NOT RUN)')

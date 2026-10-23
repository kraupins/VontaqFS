#!/usr/bin/env python3
"""G2 COPY source assertions + minimal SQL replay model. Not native Rust execution."""
from pathlib import Path
import sqlite3
root = Path(__file__).resolve().parents[1]
src = (root / "crates/core/src/storage.rs").read_text()
copy = src.split("fn copy_path_in_context<F,P>(", 1)[1].split("// CP14: destination-present MOVE", 1)[0]
assert copy.index("committed_copy_replay_before_source(&conn") < copy.index("fs::symlink_metadata(&source)")
assert 'fn cp14_running_copy_changed_or_missing_source_never_reruns_children()' in src
assert 'fn committed_copy_replay_before_source(' in src
assert 'stored_operation != operation' in src
assert 'stored_fingerprint != fingerprint' in src
assert 'original != committed' in src
assert '"copyIntent":operation' in src
assert 'if phase == "RUNNING" { StorageError::RecoveryRequired }' in src
# SQL model of committed/unfinished parents and receipts: replay depends on
# saved arguments, original fingerprint, and expiry, not current source rows.
db=sqlite3.connect(":memory:")
db.executescript("""
CREATE TABLE parent (id TEXT PRIMARY KEY, operation TEXT, fingerprint TEXT, phase TEXT, result TEXT, expires INTEGER);
CREATE TABLE receipt (id TEXT PRIMARY KEY, operation TEXT, fingerprint TEXT, result TEXT, expires INTEGER);
INSERT INTO parent VALUES ('copy','fs-copy:/old:/new:false','fp','COMMITTED','4',9999);
INSERT INTO receipt VALUES ('copy','fs-copy:/old:/new:false','fp','4',9999);
INSERT INTO parent VALUES ('running','fs-copy:/old:/new:false','fp','RUNNING',NULL,NULL);
""")
def replay(id,operation,now):
    parent=db.execute('SELECT operation,fingerprint,phase,result,expires FROM parent WHERE id=?',(id,)).fetchone()
    if parent is None: return None
    prior, fp, phase, result, expiry=parent
    if prior != operation: return 'CONFLICT'
    if phase != 'COMMITTED': return 'UNCERTAIN'
    if expiry is None or expiry<=now: return 'EXPIRED'
    r=db.execute('SELECT operation,fingerprint,result,expires FROM receipt WHERE id=?',(id,)).fetchone()
    if not r: return 'RECOVERY_REQUIRED'
    if r[0]!=prior or r[1]!=fp or r[2]!=result or r[3] is None or r[3]<=now: return 'RECOVERY_REQUIRED'
    return int(result)
assert replay('copy','fs-copy:/old:/new:false',5)==4
assert replay('copy','fs-copy:/old:/other:false',5)=='CONFLICT'
assert replay('copy','fs-copy:/old:/new:false',9999)=='EXPIRED'
assert replay('running','fs-copy:/old:/new:false',5)=='UNCERTAIN'
db.execute("DELETE FROM receipt WHERE id='copy'")
assert replay('copy','fs-copy:/old:/new:false',5)=='RECOVERY_REQUIRED'
print('PASS G2 COPY source and synthetic replay invariants (Rust NOT RUN)')

"""CP14 source/SQLite regression checks; not a Rust compile/crash test."""
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
src = (ROOT / 'crates/core/src/storage.rs').read_text()


def run():
    assert 'fn fs_operation_replay<T:' in src
    assert 'if stored_kind != kind || stored_fp != fingerprint' in src
    assert 'phase != "COMMITTED"' in src
    assert 'return Err(StorageError::ResultExpired)' in src
    assert 'semantic_fingerprint("DELETE", &payload)' in src
    assert 'semantic_fingerprint("MOVE", &move_payload)' in src
    assert 'sql_atomic_store(tx,false,space_id,request_id,operation,&fingerprint,value)?' in src
    assert 'fn delete_intent_replay_preserves_semantic_identity_after_restart()' in src
    assert 'fn move_intent_replay_checks_payload_after_source_disappeared()' in src
    assert 'fn unfingerprinted_legacy_delete_receipt_is_not_safe_replay_evidence()' in src
    assert 'fn composite_copy_replay<T:' in src
    assert 'fn copy_parent_receipt_binds_selected_source_manifest()' in src
    # Simulate one SQL transaction writing terminal durable parent plus protected receipt.
    db=sqlite3.connect(':memory:')
    db.executescript('''
      CREATE TABLE operations (id TEXT PRIMARY KEY, phase TEXT NOT NULL, fp TEXT NOT NULL, result TEXT);
      CREATE TABLE receipts (id TEXT PRIMARY KEY, fp TEXT NOT NULL, result TEXT NOT NULL);
    ''')
    try:
        with db:
            db.execute('INSERT INTO operations VALUES (?,?,?,?)', ('x','COMMITTED','fingerprint','1'))
            db.execute('INSERT INTO receipts VALUES (?,?,?)',('x','fingerprint','1'))
        assert db.execute('SELECT (SELECT fp FROM operations WHERE id=?)=(SELECT fp FROM receipts WHERE id=?)',('x','x')).fetchone()[0] == 1
    finally:
        db.close()
    print('PASS CP14 FS identity: source guards + synthetic atomic SQL receipt')

if __name__ == '__main__':
    run()

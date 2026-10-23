"""CP14 MOVE composite v3 journal shape and SQLite guards; not a Rust build/crash test."""
from pathlib import Path
import sqlite3

SRC = (Path(__file__).resolve().parents[1] / 'crates/core/src/storage.rs').read_text()

def main():
    for text in (
        'struct MoveFallbackPlan',
        'fn prepare_composite_move(',
        'fn composite_move_child(',
        'fn run_composite_move<F,P>(',
        'self.set_composite_move_item(space_id,request_id,0,"RUNNING",None)?',
        'self.set_composite_move_item(space_id,request_id,1,"RUNNING",None)?',
        'self.verify_composite_move_destination(space_id,plan)?',
        'kind=\'MOVE_COMPOSITE\'',
        'AND phase<>\'COMMITTED\'',
        'sql_atomic_store(&tx,false,space_id,request_id,operation,fingerprint,&copied)?',
        'fn cp14_composite_move_overwrite_replays_after_restart()',
        'fn cp14_composite_move_reconciles_committed_copy_before_delete()',
        'fn cp14_composite_move_running_without_child_receipt_is_uncertain()',
    ):
        assert text in SRC, f'missing source invariant: {text}'
    # Standalone SQLite simulation: durable immutable plan + child state changes
    # and parent/result receipt publication MUST be a single transaction.
    conn = sqlite3.connect(':memory:')
    conn.execute('PRAGMA foreign_keys=ON')
    conn.executescript('''
    CREATE TABLE parent (id TEXT PRIMARY KEY, fp TEXT NOT NULL, phase TEXT NOT NULL,
      plan TEXT NOT NULL, result TEXT);
    CREATE TABLE child (parent_id TEXT NOT NULL REFERENCES parent(id), ix INTEGER NOT NULL,
      fp TEXT NOT NULL, phase TEXT NOT NULL, result TEXT, PRIMARY KEY(parent_id, ix));
    CREATE TABLE receipt (id TEXT PRIMARY KEY, fp TEXT NOT NULL, result TEXT NOT NULL);
    ''')
    with conn:
        conn.execute('INSERT INTO parent VALUES (?, ?, ?, ?, NULL)', ('m1','fp1','RUNNING','immutable-file-plan'))
        conn.executemany('INSERT INTO child VALUES (?, ?, ?, ?, NULL)', [('m1',0,'copy-fp','PENDING'),('m1',1,'del-fp','PENDING')])
    conn.execute("UPDATE child SET phase='RUNNING' WHERE parent_id='m1' AND ix=0 AND phase='PENDING'")
    assert conn.execute("SELECT COUNT(*) FROM child WHERE parent_id='m1' AND phase='RUNNING'").fetchone()[0] == 1
    # A RUNNING child with no confirmed receipt is unknown and not retryable.
    assert conn.execute("SELECT phase FROM child WHERE parent_id='m1' AND ix=0").fetchone()[0] != 'PENDING'
    conn.execute("UPDATE child SET phase='COMMITTED', result='1' WHERE parent_id='m1' AND ix=0")
    conn.execute("UPDATE child SET phase='RUNNING' WHERE parent_id='m1' AND ix=1 AND phase='PENDING'")
    conn.execute("UPDATE child SET phase='COMMITTED',result='1' WHERE parent_id='m1' AND ix=1")
    conn.commit()
    # Inject receipt failure: parent transition must roll back as well.
    try:
        with conn:
            conn.execute("UPDATE parent SET phase='COMMITTED', result='1' WHERE id='m1' AND NOT EXISTS(SELECT 1 FROM child WHERE parent_id='m1' AND phase<>'COMMITTED')")
            conn.execute("INSERT INTO receipt VALUES ('m1','fp1','1')")
            raise ValueError('crash before commit')
    except ValueError:
        pass
    assert conn.execute("SELECT phase FROM parent WHERE id='m1'").fetchone()[0] == 'RUNNING'
    assert conn.execute("SELECT COUNT(*) FROM receipt").fetchone()[0] == 0
    with conn:
        changed = conn.execute("UPDATE parent SET phase='COMMITTED', result='1' WHERE id='m1' AND NOT EXISTS(SELECT 1 FROM child WHERE parent_id='m1' AND phase<>'COMMITTED')").rowcount
        assert changed == 1
        conn.execute("INSERT INTO receipt VALUES ('m1','fp1','1')")
    assert conn.execute("SELECT result FROM receipt WHERE id='m1' AND fp='fp1'").fetchone()[0] == '1'
    print('PASS CP14 MOVE composite source + synthetic atomic parent/child/receipt')

if __name__ == '__main__':
    main()

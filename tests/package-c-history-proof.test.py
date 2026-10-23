#!/usr/bin/env python3
"""CP14 retained proof regression. SQLite fixture, not a Rust/Windows test."""
import sqlite3
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).parents[1]
SQL = (ROOT / "crates/core/src/retained_outcomes.sql").read_text()
CORE = (ROOT / "crates/core/src/storage.rs").read_text()
RUNTIME = (ROOT / "crates/runtime/src/lib.rs").read_text()
PROTOCOL = (ROOT / "crates/protocol/src/lib.rs").read_text()
SDK = (ROOT / "packages/sdk/src/index.ts").read_text()
assert "retained_status_owner" in CORE
assert "retained_terminal_operation" in CORE
assert "history_proof: Option<HistoricalStatusProof>" in PROTOCOL
assert "authenticate_retained_proof" in RUNTIME
assert "retainedOperationStatus" in SDK
assert "retainedStreamStatus" in SDK
assert "lookupRetainedOperation" in SDK
assert "lookupRetainedStream" in SDK
assert "history_only" in PROTOCOL
assert "retained_challenge_hash" in CORE
assert "if input.history_only" in RUNTIME
assert "application_external_id" in CORE
assert "AND NOT EXISTS (SELECT 1 FROM applications" in CORE
assert "CASE WHEN p.revoked_at_ms IS NULL THEN p.credential_hash ELSE NULL END" in CORE
assert "CREATE INDEX IF NOT EXISTS retained_pairing_owners_proof" in CORE

with tempfile.TemporaryDirectory() as directory:
    db = sqlite3.connect(str(Path(directory)/"registry.sqlite"))
    db.executescript("""CREATE TABLE applications(id TEXT PRIMARY KEY);
    CREATE TABLE pairings(id TEXT PRIMARY KEY,application_id TEXT,client_instance_id TEXT,credential_hash TEXT,revoked_at_ms INTEGER);
    CREATE TABLE durable_operations(scope_kind TEXT,scope_id TEXT,owner_application_id TEXT,request_id TEXT,
      phase TEXT,expires_at_ms INTEGER,result_json TEXT);""")
    # Older v3 registry has a four-column proof-less table.
    db.execute("CREATE TABLE retained_pairing_owners(pairing_id TEXT PRIMARY KEY,owner_application_id TEXT NOT NULL,deleted_at_ms INTEGER NOT NULL,expires_at_ms INTEGER NOT NULL)")
    db.executescript(SQL)
    for col in ("client_instance_id", "credential_hash", "application_kind", "application_external_id"):
        if col not in {r[1] for r in db.execute("PRAGMA table_info(retained_pairing_owners)")}:
            db.execute(f"ALTER TABLE retained_pairing_owners ADD COLUMN {col} TEXT")
    db.execute("CREATE INDEX IF NOT EXISTS retained_pairing_owners_proof ON retained_pairing_owners(client_instance_id,credential_hash,expires_at_ms)")
    now = int(time.time()*1000)
    db.execute("INSERT INTO applications VALUES('owner')")
    db.executemany("INSERT INTO pairings VALUES(?,?,?,?,?)",[("old","owner","client","oldhash",now-1),("active","owner","client","activehash",None)])
    db.execute("INSERT INTO retained_pairing_owners(pairing_id,owner_application_id,deleted_at_ms,expires_at_ms,client_instance_id,credential_hash,application_kind,application_external_id) SELECT id,application_id,?, ?,CASE WHEN revoked_at_ms IS NULL THEN client_instance_id END,CASE WHEN revoked_at_ms IS NULL THEN credential_hash END,CASE WHEN revoked_at_ms IS NULL THEN 'figma-plugin' END,CASE WHEN revoked_at_ms IS NULL THEN 'source-app' END FROM pairings",(now,now+604800000))
    db.execute("INSERT INTO durable_operations VALUES('application','owner','owner','job','COMMITTED',?,?)",(now+604800000,'{"id":"job"}'))
    db.execute("DELETE FROM pairings")
    db.execute("DELETE FROM applications")
    proof_sql="SELECT owner_application_id,pairing_id FROM retained_pairing_owners h WHERE client_instance_id=? AND credential_hash=? AND expires_at_ms>? AND NOT EXISTS(SELECT 1 FROM applications a WHERE a.id=h.owner_application_id) AND NOT EXISTS(SELECT 1 FROM pairings p WHERE p.id=h.pairing_id)"
    assert db.execute(proof_sql,("client","activehash",now)).fetchone()==('owner','active')
    assert db.execute(proof_sql,("client","oldhash",now)).fetchone() is None
    assert db.execute(proof_sql,("other-client","activehash",now)).fetchone() is None
    assert db.execute(proof_sql,("client","activehash",now+604800001)).fetchone() is None
    challenge_sql="SELECT credential_hash FROM retained_pairing_owners h WHERE h.client_instance_id=? AND h.application_kind=? AND h.application_external_id=? AND h.expires_at_ms>? AND NOT EXISTS(SELECT 1 FROM applications a WHERE a.id=h.owner_application_id)"
    assert db.execute(challenge_sql,('client','figma-plugin','source-app',now)).fetchone()==('activehash',)
    assert db.execute(challenge_sql,('client','figma-plugin','another-app',now)).fetchone() is None
    status_sql="SELECT result_json FROM durable_operations WHERE scope_kind='application' AND scope_id=? AND owner_application_id=? AND request_id=? AND phase IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED') AND expires_at_ms>? AND result_json IS NOT NULL"
    assert db.execute(status_sql,('owner','owner','job',now)).fetchone()==('{"id":"job"}',)
    assert db.execute(status_sql,('intruder','intruder','job',now)).fetchone() is None
    db.execute("UPDATE durable_operations SET phase='RECONCILING'")
    assert db.execute(status_sql,('owner','owner','job',now)).fetchone() is None
print('PASS: legacy v3 additive history-proof columns, revoked pairings denied, exact owner and terminal expiry')

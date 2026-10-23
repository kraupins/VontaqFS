-- CP17 additive Registry-v3 safety guards, installed under the OS root lock
-- and the same BEGIN IMMEDIATE migration/upgrade as the capacity escrow.
-- An expired receipt can still be the only proof of a child awaiting recovery.
-- BEFORE DELETE also fences INSERT OR REPLACE's implicit DELETE when SQLite
-- recursive_triggers=ON. Nonterminal evidence must never be made forgettable.
CREATE TRIGGER IF NOT EXISTS cp17_space_receipt_unresolved_guard
BEFORE DELETE ON mutation_receipts
WHEN EXISTS (SELECT 1 FROM pending_mutations m WHERE m.space_id=OLD.space_id AND m.id=OLD.request_id)
 OR EXISTS (SELECT 1 FROM durable_operations p WHERE p.scope_kind='space'
   AND p.scope_id=OLD.space_id AND p.request_id=OLD.request_id
   AND p.phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED'))
 OR EXISTS (SELECT 1 FROM filesystem_intents i WHERE i.scope_kind='space'
   AND i.scope_id=OLD.space_id AND i.request_id=OLD.request_id AND i.state<>'CLEANED')
 OR EXISTS (SELECT 1 FROM durable_capacity_spends c WHERE c.scope_kind='space'
   AND c.scope_id=OLD.space_id AND c.child_request_id=OLD.request_id
   AND EXISTS (SELECT 1 FROM durable_capacity_escrows e WHERE e.scope_kind=c.scope_kind
     AND e.scope_id=c.scope_id AND e.root_request_id=c.root_request_id AND e.state<>'TERMINAL'))
 OR EXISTS (SELECT 1 FROM durable_operation_items i
   JOIN durable_operations p ON p.scope_kind=i.scope_kind AND p.scope_id=i.scope_id AND p.request_id=i.request_id
   WHERE i.scope_kind='space' AND i.scope_id=OLD.space_id AND i.phase IN ('PENDING','RUNNING')
     AND CASE WHEN json_valid(i.result_json) THEN json_extract(i.result_json,'$.childRequestId') ELSE NULL END=OLD.request_id)
BEGIN SELECT RAISE(ABORT,'CP17_SPACE_RECEIPT_REQUIRED_FOR_RECOVERY'); END;

CREATE TRIGGER IF NOT EXISTS cp17_application_receipt_unresolved_guard
BEFORE DELETE ON application_mutation_receipts
WHEN EXISTS (SELECT 1 FROM durable_operations p WHERE p.scope_kind='application'
   AND p.scope_id=OLD.application_id AND p.request_id=OLD.request_id
   AND p.phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED'))
 OR EXISTS (SELECT 1 FROM filesystem_intents i WHERE i.scope_kind='application'
   AND i.scope_id=OLD.application_id AND i.request_id=OLD.request_id AND i.state<>'CLEANED')
BEGIN SELECT RAISE(ABORT,'CP17_APPLICATION_RECEIPT_REQUIRED_FOR_RECOVERY'); END;

CREATE TRIGGER IF NOT EXISTS cp17_retained_space_unresolved_guard
BEFORE DELETE ON retained_space_outcomes
WHEN EXISTS (SELECT 1 FROM durable_operations p WHERE p.scope_kind='space'
   AND p.scope_id=OLD.space_id AND p.request_id=OLD.request_id
   AND p.phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED'))
 OR EXISTS (SELECT 1 FROM filesystem_intents i WHERE i.scope_kind='space'
   AND i.scope_id=OLD.space_id AND i.request_id=OLD.request_id AND i.state<>'CLEANED')
BEGIN SELECT RAISE(ABORT,'CP17_RETAINED_SPACE_REQUIRED_FOR_RECOVERY'); END;

CREATE TRIGGER IF NOT EXISTS cp17_retained_application_unresolved_guard
BEFORE DELETE ON retained_application_outcomes
WHEN EXISTS (SELECT 1 FROM durable_operations p WHERE p.scope_kind='application'
   AND p.scope_id=OLD.owner_application_id AND p.request_id=OLD.request_id
   AND p.phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED'))
 OR EXISTS (SELECT 1 FROM filesystem_intents i WHERE i.scope_kind='application'
   AND i.scope_id=OLD.owner_application_id AND i.request_id=OLD.request_id AND i.state<>'CLEANED')
BEGIN SELECT RAISE(ABORT,'CP17_RETAINED_APPLICATION_REQUIRED_FOR_RECOVERY'); END;

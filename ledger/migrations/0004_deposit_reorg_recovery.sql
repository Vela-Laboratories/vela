ALTER TABLE ledger_deposits
ADD COLUMN reversal_transaction_id UUID UNIQUE REFERENCES ledger_journal_transactions(id);

ALTER TABLE ledger_indexer_halts
DROP CONSTRAINT ledger_indexer_halts_reason_check;

ALTER TABLE ledger_indexer_halts
ADD CONSTRAINT ledger_indexer_halts_reason_check CHECK (reason IN (
    'provider_chain_mismatch',
    'provider_stale',
    'provider_disagreement',
    'finalized_regression',
    'vault_code_mismatch',
    'malformed_log',
    'unregistered_asset',
    'ledger_rejection',
    'lease_lost',
    'credited_deposit_contradiction',
    'historical_deposit_appeared'
));

CREATE TABLE ledger_asset_halts (
    id UUID PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    deployment_id UUID NOT NULL REFERENCES ledger_chain_deployments(id),
    source_deposit_id UUID REFERENCES ledger_deposits(id),
    reason TEXT NOT NULL CHECK (reason IN (
        'credited_deposit_contradiction',
        'historical_deposit_appeared'
    )),
    deposit_credits_halted BOOLEAN NOT NULL DEFAULT TRUE,
    trading_halted BOOLEAN NOT NULL DEFAULT TRUE,
    withdrawals_halted BOOLEAN NOT NULL DEFAULT TRUE,
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    detected_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    resolution_evidence JSONB CHECK (
        resolution_evidence IS NULL OR jsonb_typeof(resolution_evidence) = 'object'
    ),
    resolution_initiated_by TEXT,
    resolved_at TIMESTAMPTZ,
    resolved_by TEXT,
    CHECK (deposit_credits_halted OR trading_halted OR withdrawals_halted),
    CHECK (
        (resolution_evidence IS NULL AND resolution_initiated_by IS NULL
            AND resolved_at IS NULL AND resolved_by IS NULL)
        OR
        (resolution_evidence IS NOT NULL AND length(resolution_initiated_by) > 0
            AND resolved_at IS NOT NULL AND length(resolved_by) > 0
            AND resolution_initiated_by <> resolved_by)
    )
);

CREATE UNIQUE INDEX ledger_asset_halts_one_active_per_asset
ON ledger_asset_halts (asset_id)
WHERE resolved_at IS NULL;

CREATE INDEX ledger_asset_halts_active
ON ledger_asset_halts (asset_id, detected_at)
WHERE resolved_at IS NULL;

CREATE TABLE ledger_indexer_audit_checkpoints (
    deployment_id UUID PRIMARY KEY REFERENCES ledger_chain_deployments(id),
    next_block BIGINT NOT NULL CHECK (next_block >= 0),
    cycle BIGINT NOT NULL DEFAULT 0 CHECK (cycle >= 0),
    last_scanned_block_number BIGINT CHECK (
        last_scanned_block_number IS NULL OR last_scanned_block_number >= 0
    ),
    last_scanned_block_hash TEXT CHECK (
        last_scanned_block_hash IS NULL OR last_scanned_block_hash ~ '^0x[0-9a-f]{64}$'
    ),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK ((last_scanned_block_number IS NULL) = (last_scanned_block_hash IS NULL))
);

CREATE FUNCTION ledger_guard_asset_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'asset halt evidence cannot be deleted';
    END IF;
    IF NEW.id <> OLD.id
       OR NEW.asset_id <> OLD.asset_id
       OR NEW.deployment_id <> OLD.deployment_id
       OR NEW.source_deposit_id IS DISTINCT FROM OLD.source_deposit_id
       OR NEW.reason <> OLD.reason
       OR NEW.deposit_credits_halted <> OLD.deposit_credits_halted
       OR NEW.trading_halted <> OLD.trading_halted
       OR NEW.withdrawals_halted <> OLD.withdrawals_halted
       OR NEW.evidence <> OLD.evidence
       OR NEW.detected_at <> OLD.detected_at
       OR OLD.resolved_at IS NOT NULL
       OR NEW.resolved_at IS NULL
       OR NEW.resolution_evidence IS NULL
       OR length(NEW.resolution_initiated_by) = 0
       OR length(NEW.resolved_by) = 0
       OR NEW.resolution_initiated_by = NEW.resolved_by THEN
        RAISE EXCEPTION 'asset halt evidence is immutable except for dual-control resolution';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_asset_halt_guard
BEFORE UPDATE OR DELETE ON ledger_asset_halts
FOR EACH ROW EXECUTE FUNCTION ledger_guard_asset_halt();

CREATE FUNCTION ledger_block_halted_asset_operation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    operation TEXT;
BEGIN
    SELECT operation_type INTO STRICT operation
    FROM ledger_journal_transactions
    WHERE id = NEW.transaction_id;

    IF EXISTS (
        SELECT 1
        FROM ledger_asset_halts halt
        WHERE halt.asset_id = NEW.asset_id
          AND halt.resolved_at IS NULL
          AND (
              (halt.deposit_credits_halted AND operation = 'deposit_credit')
              OR (halt.trading_halted AND operation IN (
                  'order_reservation', 'trade', 'fee', 'rebate'
              ))
              OR (halt.withdrawals_halted AND operation IN (
                  'withdrawal_reservation', 'withdrawal_completion'
              ))
          )
    ) THEN
        RAISE EXCEPTION 'asset operation is halted for %', operation
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_journal_leg_asset_halt_guard
BEFORE INSERT ON ledger_journal_legs
FOR EACH ROW EXECUTE FUNCTION ledger_block_halted_asset_operation();

CREATE FUNCTION ledger_guard_indexer_audit_checkpoint()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    deployment_start BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'indexer audit checkpoints cannot be deleted';
    END IF;
    IF NEW.deployment_id <> OLD.deployment_id OR NEW.cycle < OLD.cycle THEN
        RAISE EXCEPTION 'indexer audit checkpoint cannot regress';
    END IF;
    IF NEW.cycle = OLD.cycle AND NEW.next_block < OLD.next_block THEN
        RAISE EXCEPTION 'indexer audit checkpoint cannot move backwards within a cycle';
    END IF;
    IF NEW.cycle > OLD.cycle THEN
        SELECT start_block INTO STRICT deployment_start
        FROM ledger_chain_deployments
        WHERE id = NEW.deployment_id;
        IF NEW.cycle <> OLD.cycle + 1 OR NEW.next_block <> deployment_start THEN
            RAISE EXCEPTION 'indexer audit cycle reset is invalid';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_indexer_audit_checkpoint_guard
BEFORE UPDATE OR DELETE ON ledger_indexer_audit_checkpoints
FOR EACH ROW EXECUTE FUNCTION ledger_guard_indexer_audit_checkpoint();

CREATE OR REPLACE FUNCTION ledger_enforce_deposit_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.customer_reference <> OLD.customer_reference
       OR NEW.destination_account_id <> OLD.destination_account_id
       OR NEW.asset_id <> OLD.asset_id
       OR NEW.amount <> OLD.amount
       OR NEW.created_at <> OLD.created_at THEN
        RAISE EXCEPTION 'deposit identity and amount are immutable';
    END IF;

    IF NEW.credit_transaction_id IS NOT NULL
       AND NEW.state NOT IN ('credited', 'reversed', 'quarantined') THEN
        RAISE EXCEPTION 'deposit credit transaction is only valid after credit';
    END IF;
    IF OLD.credit_transaction_id IS NOT NULL
       AND NEW.credit_transaction_id IS DISTINCT FROM OLD.credit_transaction_id THEN
        RAISE EXCEPTION 'deposit credit transaction is immutable';
    END IF;
    IF OLD.reversal_transaction_id IS NOT NULL
       AND NEW.reversal_transaction_id IS DISTINCT FROM OLD.reversal_transaction_id THEN
        RAISE EXCEPTION 'deposit reversal transaction is immutable';
    END IF;
    IF NEW.reversal_transaction_id IS NOT NULL
       AND NEW.state NOT IN ('reversed', 'quarantined') THEN
        RAISE EXCEPTION 'deposit reversal transaction is only valid for reversed or quarantined deposits';
    END IF;

    IF NEW.state <> OLD.state AND NOT (
        (OLD.state = 'observed' AND NEW.state IN ('confirming', 'rejected', 'orphaned', 'quarantined'))
        OR (OLD.state = 'confirming' AND NEW.state IN ('finalized', 'rejected', 'orphaned', 'quarantined'))
        OR (OLD.state = 'finalized' AND NEW.state IN ('credited', 'orphaned', 'quarantined'))
        OR (OLD.state = 'credited' AND NEW.state IN ('reversed', 'quarantined'))
    ) THEN
        RAISE EXCEPTION 'invalid deposit transition from % to %', OLD.state, NEW.state;
    END IF;

    IF NEW.state = 'credited' AND NEW.credit_transaction_id IS NULL THEN
        RAISE EXCEPTION 'credited deposit requires a journal transaction';
    END IF;
    IF NEW.state = 'reversed' AND (
        NEW.credit_transaction_id IS NULL OR NEW.reversal_transaction_id IS NULL
    ) THEN
        RAISE EXCEPTION 'reversed deposit requires credit and reversal journal transactions';
    END IF;

    RETURN NEW;
END;
$$;

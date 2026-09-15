DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM ledger_custody_snapshots)
       OR EXISTS (SELECT 1 FROM ledger_reconciliation_results) THEN
        RAISE EXCEPTION
            'cannot enable verified reconciliation while legacy reconciliation rows exist';
    END IF;
END;
$$;

ALTER TABLE ledger_custody_snapshots
DROP CONSTRAINT ledger_custody_snapshots_chain_id_vault_address_block_hash_key,
ADD COLUMN deployment_id UUID NOT NULL REFERENCES ledger_chain_deployments(id),
ADD COLUMN primary_provider TEXT NOT NULL CHECK (length(primary_provider) BETWEEN 1 AND 255),
ADD COLUMN secondary_provider TEXT NOT NULL CHECK (length(secondary_provider) BETWEEN 1 AND 255),
ADD COLUMN block_timestamp TIMESTAMPTZ NOT NULL,
ADD COLUMN evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
ADD CONSTRAINT ledger_custody_snapshots_distinct_providers
    CHECK (lower(primary_provider) <> lower(secondary_provider));

CREATE UNIQUE INDEX ledger_custody_snapshots_input_identity
ON ledger_custody_snapshots (input_hash);

ALTER TABLE ledger_reconciliation_results
ADD COLUMN evidence_hash BYTEA NOT NULL CHECK (octet_length(evidence_hash) = 32),
ADD CONSTRAINT ledger_reconciliation_result_semantics CHECK (
    (status = 'balanced' AND difference = 0)
    OR (status = 'surplus' AND difference > 0)
    OR (status = 'deficit' AND difference < 0)
    OR status = 'indeterminate'
);

CREATE UNIQUE INDEX ledger_reconciliation_results_evidence_identity
ON ledger_reconciliation_results (evidence_hash);

ALTER TABLE ledger_asset_halts
DROP CONSTRAINT ledger_asset_halts_reason_check;

ALTER TABLE ledger_asset_halts
ADD CONSTRAINT ledger_asset_halts_reason_check CHECK (reason IN (
    'credited_deposit_contradiction',
    'historical_deposit_appeared',
    'reconciliation_deficit',
    'reconciliation_indeterminate'
));

CREATE TABLE ledger_reconciliation_halt_links (
    result_id UUID PRIMARY KEY REFERENCES ledger_reconciliation_results(id),
    halt_id UUID NOT NULL UNIQUE REFERENCES ledger_asset_halts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TRIGGER ledger_reconciliation_halt_links_immutable
BEFORE UPDATE OR DELETE ON ledger_reconciliation_halt_links
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TABLE ledger_reconciliation_failures (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES ledger_chain_deployments(id),
    reason TEXT NOT NULL CHECK (reason IN (
        'provider_chain_mismatch',
        'provider_disagreement',
        'provider_stale',
        'finalized_regression',
        'vault_code_mismatch',
        'malformed_rpc',
        'ledger_indeterminate',
        'lease_lost'
    )),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    evidence_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(evidence_hash) = 32),
    detected_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_reconciliation_failure_halts (
    failure_id UUID NOT NULL REFERENCES ledger_reconciliation_failures(id),
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    halt_id UUID NOT NULL REFERENCES ledger_asset_halts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (failure_id, asset_id)
);

CREATE TRIGGER ledger_reconciliation_failures_immutable
BEFORE UPDATE OR DELETE ON ledger_reconciliation_failures
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_reconciliation_failure_halts_immutable
BEFORE UPDATE OR DELETE ON ledger_reconciliation_failure_halts
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE FUNCTION ledger_require_reconciliation_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.status IN ('deficit', 'indeterminate') AND NOT EXISTS (
        SELECT 1
        FROM ledger_reconciliation_halt_links link
        JOIN ledger_asset_halts halt ON halt.id = link.halt_id
        WHERE link.result_id = NEW.id
          AND halt.asset_id = NEW.asset_id
          AND halt.resolved_at IS NULL
          AND halt.deposit_credits_halted
          AND halt.trading_halted
          AND halt.withdrawals_halted
    ) THEN
        RAISE EXCEPTION 'unsafe reconciliation result requires an active asset halt';
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_reconciliation_result_halt_guard
AFTER INSERT ON ledger_reconciliation_results
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_require_reconciliation_halt();

CREATE TABLE ledger_reconciliation_worker_checkpoints (
    deployment_id UUID PRIMARY KEY REFERENCES ledger_chain_deployments(id),
    lease_owner UUID,
    lease_expires_at TIMESTAMPTZ,
    cycle BIGINT NOT NULL DEFAULT 0 CHECK (cycle >= 0),
    last_finalized_block_number BIGINT CHECK (
        last_finalized_block_number IS NULL OR last_finalized_block_number >= 0
    ),
    last_finalized_block_hash TEXT CHECK (
        last_finalized_block_hash IS NULL OR last_finalized_block_hash ~ '^0x[0-9a-f]{64}$'
    ),
    last_snapshot_id UUID REFERENCES ledger_custody_snapshots(id),
    last_completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((last_finalized_block_number IS NULL) = (last_finalized_block_hash IS NULL)),
    CHECK ((last_snapshot_id IS NULL) = (last_completed_at IS NULL))
);

CREATE FUNCTION ledger_guard_reconciliation_checkpoint()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'reconciliation checkpoints cannot be deleted';
    END IF;
    IF NEW.deployment_id <> OLD.deployment_id OR NEW.cycle < OLD.cycle THEN
        RAISE EXCEPTION 'reconciliation checkpoint identity and cycle are monotonic';
    END IF;
    IF OLD.last_finalized_block_number IS NOT NULL AND (
        NEW.last_finalized_block_number IS NULL
        OR NEW.last_finalized_block_number < OLD.last_finalized_block_number
        OR (
            NEW.last_finalized_block_number = OLD.last_finalized_block_number
            AND NEW.last_finalized_block_hash <> OLD.last_finalized_block_hash
        )
    ) THEN
        RAISE EXCEPTION 'reconciliation finalized checkpoint cannot regress or change hash';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_reconciliation_checkpoint_guard
BEFORE UPDATE OR DELETE ON ledger_reconciliation_worker_checkpoints
FOR EACH ROW EXECUTE FUNCTION ledger_guard_reconciliation_checkpoint();

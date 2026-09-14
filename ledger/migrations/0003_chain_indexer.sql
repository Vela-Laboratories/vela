CREATE UNIQUE INDEX ledger_assets_chain_contract_identity
ON ledger_assets (
    chain_id,
    COALESCE(lower(contract_address), '0x0000000000000000000000000000000000000000')
)
WHERE chain_id IS NOT NULL;

CREATE TABLE ledger_chain_deployments (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL CHECK (chain_id > 0),
    network TEXT NOT NULL CHECK (length(network) BETWEEN 1 AND 64),
    vault_address TEXT NOT NULL CHECK (vault_address ~ '^0x[0-9a-f]{40}$'),
    deployment_version TEXT NOT NULL CHECK (length(deployment_version) BETWEEN 1 AND 64),
    runtime_code_hash TEXT NOT NULL CHECK (runtime_code_hash ~ '^0x[0-9a-f]{64}$'),
    deposit_event_signature TEXT NOT NULL,
    deposit_event_topic TEXT NOT NULL CHECK (deposit_event_topic ~ '^0x[0-9a-f]{64}$'),
    finality_policy TEXT NOT NULL CHECK (finality_policy = 'rpc_finalized_two_provider'),
    start_block BIGINT NOT NULL CHECK (start_block >= 0),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (chain_id, vault_address, deployment_version),
    UNIQUE (chain_id, vault_address, deposit_event_topic)
);

CREATE UNIQUE INDEX ledger_chain_deployments_one_enabled_vault
ON ledger_chain_deployments (chain_id, vault_address)
WHERE enabled;

CREATE TABLE ledger_indexer_checkpoints (
    deployment_id UUID PRIMARY KEY REFERENCES ledger_chain_deployments(id),
    next_block BIGINT NOT NULL CHECK (next_block >= 0),
    last_finalized_block_number BIGINT CHECK (last_finalized_block_number IS NULL OR last_finalized_block_number >= 0),
    last_finalized_block_hash TEXT CHECK (
        last_finalized_block_hash IS NULL OR last_finalized_block_hash ~ '^0x[0-9a-f]{64}$'
    ),
    lease_owner UUID,
    lease_expires_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK ((last_finalized_block_number IS NULL) = (last_finalized_block_hash IS NULL)),
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL))
);

CREATE TABLE ledger_indexer_halts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES ledger_chain_deployments(id),
    reason TEXT NOT NULL CHECK (reason IN (
        'provider_chain_mismatch',
        'provider_stale',
        'provider_disagreement',
        'finalized_regression',
        'vault_code_mismatch',
        'malformed_log',
        'unregistered_asset',
        'ledger_rejection',
        'lease_lost'
    )),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    detected_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    resolved_at TIMESTAMPTZ,
    resolved_by TEXT,
    CHECK ((resolved_at IS NULL) = (resolved_by IS NULL))
);

CREATE INDEX ledger_indexer_halts_unresolved
ON ledger_indexer_halts (deployment_id, detected_at)
WHERE resolved_at IS NULL;

CREATE FUNCTION ledger_guard_chain_deployment()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'chain deployment registry rows are append-only';
    END IF;
    IF NEW.id <> OLD.id
       OR NEW.chain_id <> OLD.chain_id
       OR NEW.network <> OLD.network
       OR NEW.vault_address <> OLD.vault_address
       OR NEW.deployment_version <> OLD.deployment_version
       OR NEW.runtime_code_hash <> OLD.runtime_code_hash
       OR NEW.deposit_event_signature <> OLD.deposit_event_signature
       OR NEW.deposit_event_topic <> OLD.deposit_event_topic
       OR NEW.finality_policy <> OLD.finality_policy
       OR NEW.start_block <> OLD.start_block
       OR NEW.created_at <> OLD.created_at
       OR (NOT OLD.enabled AND NEW.enabled) THEN
        RAISE EXCEPTION 'chain deployment identity is immutable';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_chain_deployment_guard
BEFORE UPDATE OR DELETE ON ledger_chain_deployments
FOR EACH ROW EXECUTE FUNCTION ledger_guard_chain_deployment();

CREATE FUNCTION ledger_guard_indexer_checkpoint()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'indexer checkpoints cannot be deleted';
    END IF;
    IF NEW.deployment_id <> OLD.deployment_id OR NEW.next_block < OLD.next_block THEN
        RAISE EXCEPTION 'indexer checkpoint cannot move backwards';
    END IF;
    IF OLD.last_finalized_block_number IS NOT NULL AND (
        NEW.last_finalized_block_number IS NULL
        OR NEW.last_finalized_block_number < OLD.last_finalized_block_number
        OR (
            NEW.last_finalized_block_number = OLD.last_finalized_block_number
            AND NEW.last_finalized_block_hash <> OLD.last_finalized_block_hash
        )
    ) THEN
        RAISE EXCEPTION 'agreed finalized checkpoint cannot regress or change hash';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_indexer_checkpoint_guard
BEFORE UPDATE OR DELETE ON ledger_indexer_checkpoints
FOR EACH ROW EXECUTE FUNCTION ledger_guard_indexer_checkpoint();

CREATE FUNCTION ledger_guard_indexer_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'indexer halt evidence cannot be deleted';
    END IF;
    IF NEW.id <> OLD.id
       OR NEW.deployment_id <> OLD.deployment_id
       OR NEW.reason <> OLD.reason
       OR NEW.evidence <> OLD.evidence
       OR NEW.detected_at <> OLD.detected_at
       OR OLD.resolved_at IS NOT NULL
       OR NEW.resolved_at IS NULL
       OR length(NEW.resolved_by) = 0 THEN
        RAISE EXCEPTION 'indexer halt evidence is immutable except for first resolution';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_indexer_halt_guard
BEFORE UPDATE OR DELETE ON ledger_indexer_halts
FOR EACH ROW EXECUTE FUNCTION ledger_guard_indexer_halt();

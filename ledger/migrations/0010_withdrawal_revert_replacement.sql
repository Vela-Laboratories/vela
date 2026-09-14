ALTER TABLE ledger_withdrawal_chain_observations
DROP CONSTRAINT ledger_withdrawal_chain_observations_observation_type_check;

ALTER TABLE ledger_withdrawal_chain_observations
ADD CONSTRAINT ledger_withdrawal_chain_observations_observation_type_check CHECK (
    observation_type IN (
        'dropped', 'finalized_execution', 'finalized_revert', 'finalized_non_execution'
    )
);

CREATE TABLE ledger_withdrawal_replacement_evidence (
    attempt_id UUID PRIMARY KEY REFERENCES ledger_withdrawal_broadcast_attempts(id),
    predecessor_attempt_id UUID NOT NULL REFERENCES ledger_withdrawal_broadcast_attempts(id),
    primary_provider TEXT NOT NULL CHECK (length(primary_provider) BETWEEN 1 AND 255),
    secondary_provider TEXT NOT NULL CHECK (length(secondary_provider) BETWEEN 1 AND 255),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    evidence_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(evidence_hash) = 32),
    observed_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK (attempt_id <> predecessor_attempt_id),
    CHECK (lower(primary_provider) <> lower(secondary_provider))
);

CREATE TABLE ledger_withdrawal_attempt_reverts (
    attempt_id UUID PRIMARY KEY REFERENCES ledger_withdrawal_broadcast_attempts(id),
    observation_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_chain_observations(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM ledger_withdrawal_broadcast_attempts
        WHERE replacement_of IS NOT NULL
    ) THEN
        RAISE EXCEPTION
            'cannot enable verified replacement lineage while legacy replacement attempts exist';
    END IF;
END;
$$;

CREATE FUNCTION ledger_require_replacement_evidence()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.replacement_of IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM ledger_withdrawal_replacement_evidence evidence
        WHERE evidence.attempt_id = NEW.id
          AND evidence.predecessor_attempt_id = NEW.replacement_of
    ) THEN
        RAISE EXCEPTION 'replacement broadcast attempt requires two-provider evidence';
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_withdrawal_replacement_evidence_guard
AFTER INSERT ON ledger_withdrawal_broadcast_attempts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_require_replacement_evidence();

CREATE FUNCTION ledger_validate_replacement_evidence()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    attempt_row ledger_withdrawal_broadcast_attempts%ROWTYPE;
BEGIN
    SELECT * INTO attempt_row FROM ledger_withdrawal_broadcast_attempts
    WHERE id = NEW.attempt_id FOR SHARE;
    IF attempt_row.id IS NULL
       OR attempt_row.replacement_of IS NULL
       OR attempt_row.replacement_of <> NEW.predecessor_attempt_id THEN
        RAISE EXCEPTION 'replacement evidence does not match attempt lineage';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_replacement_evidence_validate
BEFORE INSERT ON ledger_withdrawal_replacement_evidence
FOR EACH ROW EXECUTE FUNCTION ledger_validate_replacement_evidence();

CREATE TRIGGER ledger_withdrawal_replacement_evidence_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_replacement_evidence
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_attempt_reverts_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_attempt_reverts
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE FUNCTION ledger_require_finalized_revert_attempt_link()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.observation_type = 'finalized_revert' AND NOT EXISTS (
        SELECT 1
        FROM ledger_withdrawal_attempt_reverts revert
        JOIN ledger_withdrawal_broadcast_attempts attempt
          ON attempt.id = revert.attempt_id
        WHERE revert.observation_id = NEW.id
          AND attempt.withdrawal_id = NEW.withdrawal_id
          AND lower(attempt.transaction_hash) = lower(NEW.transaction_hash)
    ) THEN
        RAISE EXCEPTION
            'finalized withdrawal revert requires an exact broadcast-attempt link';
    END IF;
    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_withdrawal_finalized_revert_link_guard
AFTER INSERT ON ledger_withdrawal_chain_observations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_require_finalized_revert_attempt_link();

CREATE OR REPLACE FUNCTION ledger_validate_withdrawal_chain_observation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    withdrawal_row ledger_withdrawals%ROWTYPE;
    authorization_row ledger_withdrawal_authorizations%ROWTYPE;
    deployment_row ledger_chain_deployments%ROWTYPE;
    expected_asset_address TEXT;
    required_confirmations BIGINT;
BEGIN
    SELECT * INTO withdrawal_row FROM ledger_withdrawals WHERE id = NEW.withdrawal_id FOR SHARE;
    SELECT * INTO authorization_row FROM ledger_withdrawal_authorizations
    WHERE withdrawal_id = NEW.withdrawal_id FOR SHARE;
    SELECT * INTO deployment_row FROM ledger_chain_deployments
    WHERE id = NEW.deployment_id AND enabled FOR SHARE;
    SELECT COALESCE(contract_address, '0x0000000000000000000000000000000000000000')
    INTO expected_asset_address FROM ledger_assets WHERE id = withdrawal_row.asset_id;

    IF withdrawal_row.id IS NULL OR authorization_row.id IS NULL OR deployment_row.id IS NULL
       OR authorization_row.envelope_version <> 'vela.withdrawal.authorization.eip712.v2'
       OR NEW.request_id <> withdrawal_row.id
       OR NEW.chain_id <> withdrawal_row.chain_id
       OR deployment_row.chain_id <> withdrawal_row.chain_id
       OR lower(NEW.vault_address) <> lower(withdrawal_row.vault_address)
       OR lower(deployment_row.vault_address) <> lower(withdrawal_row.vault_address)
       OR lower(NEW.customer_reference) <> lower(withdrawal_row.customer_reference)
       OR lower(NEW.destination_address) <> lower(withdrawal_row.destination_address)
       OR lower(NEW.asset_address) <> lower(expected_asset_address)
       OR NEW.amount <> withdrawal_row.amount
       OR NEW.withdrawal_nonce IS DISTINCT FROM withdrawal_row.nonce THEN
        RAISE EXCEPTION 'withdrawal chain observation does not match executable authorization facts';
    END IF;

    IF NEW.observation_type IN ('finalized_execution', 'finalized_revert', 'finalized_non_execution') THEN
        SELECT min_confirmations INTO required_confirmations
        FROM ledger_withdrawal_confirmation_policies
        WHERE deployment_id = NEW.deployment_id
          AND policy_version = NEW.confirmation_policy_version
          AND effective_at <= NEW.block_timestamp;
        IF required_confirmations IS NULL OR NEW.confirmations < required_confirmations THEN
            RAISE EXCEPTION 'withdrawal chain observation does not satisfy confirmation policy';
        END IF;
    END IF;
    IF NEW.observation_type IN ('finalized_execution', 'finalized_revert')
       AND NEW.transaction_hash IS NULL THEN
        RAISE EXCEPTION 'finalized withdrawal transaction evidence requires a transaction hash';
    END IF;
    IF NEW.observation_type = 'finalized_non_execution'
       AND NEW.block_timestamp < authorization_row.expires_at THEN
        RAISE EXCEPTION 'withdrawal non-execution cannot be final before authorization expiry';
    END IF;
    RETURN NEW;
END;
$$;

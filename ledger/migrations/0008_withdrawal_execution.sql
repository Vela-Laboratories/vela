ALTER TABLE ledger_withdrawal_authorizations
DROP CONSTRAINT ledger_withdrawal_authorizations_envelope_version_check;

ALTER TABLE ledger_withdrawal_authorizations
ADD CONSTRAINT ledger_withdrawal_authorizations_envelope_version_check CHECK (
    envelope_version IN (
        'vela.withdrawal.authorization.v1',
        'vela.withdrawal.authorization.eip712.v2'
    )
);

ALTER TABLE ledger_withdrawal_authorizations
ADD COLUMN domain_separator BYTEA CHECK (
    domain_separator IS NULL OR octet_length(domain_separator) = 32
),
ADD COLUMN struct_hash BYTEA CHECK (
    struct_hash IS NULL OR octet_length(struct_hash) = 32
),
ADD COLUMN policy_hash BYTEA CHECK (
    policy_hash IS NULL OR octet_length(policy_hash) = 32
),
ADD COLUMN approver_hash BYTEA CHECK (
    approver_hash IS NULL OR octet_length(approver_hash) = 32
),
ADD CONSTRAINT ledger_withdrawal_authorizations_v2_hashes CHECK (
    (envelope_version = 'vela.withdrawal.authorization.v1'
        AND domain_separator IS NULL AND struct_hash IS NULL
        AND policy_hash IS NULL AND approver_hash IS NULL)
    OR
    (envelope_version = 'vela.withdrawal.authorization.eip712.v2'
        AND domain_separator IS NOT NULL AND struct_hash IS NOT NULL
        AND policy_hash IS NOT NULL AND approver_hash IS NOT NULL)
);

CREATE TABLE ledger_withdrawal_contract_capabilities (
    deployment_id UUID PRIMARY KEY REFERENCES ledger_chain_deployments(id),
    authorization_version TEXT NOT NULL CHECK (authorization_version = 'eip712_v2'),
    execution_event_signature TEXT NOT NULL CHECK (
        execution_event_signature = 'WithdrawalExecuted(bytes16,address,address,address,uint256,uint256,uint256,bytes32,bytes32)'
    ),
    execution_event_topic TEXT NOT NULL CHECK (execution_event_topic ~ '^0x[0-9a-f]{64}$'),
    configured_by TEXT NOT NULL CHECK (length(configured_by) BETWEEN 1 AND 255),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_withdrawal_confirmation_policies (
    deployment_id UUID NOT NULL REFERENCES ledger_withdrawal_contract_capabilities(deployment_id),
    policy_version TEXT NOT NULL,
    min_confirmations BIGINT NOT NULL CHECK (min_confirmations > 0),
    require_finalized BOOLEAN NOT NULL CHECK (require_finalized),
    required_provider_count SMALLINT NOT NULL CHECK (required_provider_count = 2),
    effective_at TIMESTAMPTZ NOT NULL,
    configured_by TEXT NOT NULL CHECK (length(configured_by) BETWEEN 1 AND 255),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (deployment_id, policy_version),
    UNIQUE (deployment_id, effective_at)
);

CREATE TABLE ledger_withdrawal_broadcast_attempts (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL REFERENCES ledger_withdrawals(id),
    attempt_number BIGINT NOT NULL CHECK (attempt_number > 0),
    transaction_hash TEXT NOT NULL UNIQUE CHECK (transaction_hash ~ '^0x[0-9a-f]{64}$'),
    sender_address TEXT NOT NULL CHECK (sender_address ~ '^0x[0-9a-f]{40}$'),
    sender_nonce NUMERIC NOT NULL CHECK (sender_nonce >= 0 AND sender_nonce = trunc(sender_nonce)),
    vault_address TEXT NOT NULL CHECK (vault_address ~ '^0x[0-9a-f]{40}$'),
    calldata_hash BYTEA NOT NULL CHECK (octet_length(calldata_hash) = 32),
    replacement_of UUID REFERENCES ledger_withdrawal_broadcast_attempts(id),
    provider_name TEXT NOT NULL CHECK (length(provider_name) BETWEEN 1 AND 255),
    provider_reference TEXT NOT NULL CHECK (length(provider_reference) BETWEEN 1 AND 1000),
    submitted_by TEXT NOT NULL CHECK (length(submitted_by) BETWEEN 1 AND 255),
    submitted_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (withdrawal_id, attempt_number),
    CHECK (replacement_of IS NULL OR attempt_number > 1)
);

CREATE TABLE ledger_withdrawal_chain_observations (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL REFERENCES ledger_withdrawals(id),
    deployment_id UUID NOT NULL REFERENCES ledger_withdrawal_contract_capabilities(deployment_id),
    observation_type TEXT NOT NULL CHECK (observation_type IN (
        'dropped', 'finalized_execution', 'finalized_non_execution'
    )),
    transaction_hash TEXT CHECK (
        transaction_hash IS NULL OR transaction_hash ~ '^0x[0-9a-f]{64}$'
    ),
    chain_id BIGINT NOT NULL CHECK (chain_id > 0),
    vault_address TEXT NOT NULL CHECK (vault_address ~ '^0x[0-9a-f]{40}$'),
    request_id UUID NOT NULL,
    customer_reference TEXT NOT NULL CHECK (customer_reference ~ '^0x[0-9a-f]{40}$'),
    destination_address TEXT NOT NULL CHECK (destination_address ~ '^0x[0-9a-f]{40}$'),
    asset_address TEXT NOT NULL CHECK (asset_address ~ '^0x[0-9a-f]{40}$'),
    amount NUMERIC NOT NULL CHECK (amount > 0 AND amount = trunc(amount)),
    withdrawal_nonce NUMERIC NOT NULL CHECK (
        withdrawal_nonce >= 0 AND withdrawal_nonce = trunc(withdrawal_nonce)
    ),
    block_number BIGINT CHECK (block_number IS NULL OR block_number >= 0),
    block_hash TEXT CHECK (block_hash IS NULL OR block_hash ~ '^0x[0-9a-f]{64}$'),
    block_timestamp TIMESTAMPTZ,
    transaction_index INTEGER CHECK (transaction_index IS NULL OR transaction_index >= 0),
    log_index INTEGER CHECK (log_index IS NULL OR log_index >= 0),
    confirmations BIGINT CHECK (confirmations IS NULL OR confirmations > 0),
    finalized BOOLEAN NOT NULL DEFAULT FALSE,
    request_id_unused BOOLEAN,
    withdrawal_nonce_unused BOOLEAN,
    no_execution_event BOOLEAN,
    all_attempts_terminal BOOLEAN,
    primary_provider TEXT NOT NULL CHECK (length(primary_provider) BETWEEN 1 AND 255),
    secondary_provider TEXT NOT NULL CHECK (length(secondary_provider) BETWEEN 1 AND 255),
    confirmation_policy_version TEXT,
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    evidence_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(evidence_hash) = 32),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK (lower(primary_provider) <> lower(secondary_provider)),
    CHECK (
        observation_type = 'dropped'
        OR (block_number IS NOT NULL AND block_hash IS NOT NULL
            AND block_timestamp IS NOT NULL AND confirmations IS NOT NULL
            AND confirmation_policy_version IS NOT NULL AND finalized)
    ),
    CHECK (
        observation_type <> 'finalized_non_execution'
        OR (request_id_unused AND withdrawal_nonce_unused
            AND no_execution_event AND all_attempts_terminal)
    )
);

CREATE TABLE ledger_withdrawal_executions (
    withdrawal_id UUID PRIMARY KEY REFERENCES ledger_withdrawals(id),
    observation_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_chain_observations(id),
    completion_transaction_id UUID NOT NULL UNIQUE REFERENCES ledger_journal_transactions(id),
    transaction_hash TEXT NOT NULL UNIQUE CHECK (transaction_hash ~ '^0x[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_withdrawal_failure_releases (
    withdrawal_id UUID PRIMARY KEY REFERENCES ledger_withdrawals(id),
    observation_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_chain_observations(id),
    release_transaction_id UUID NOT NULL UNIQUE REFERENCES ledger_journal_transactions(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'withdrawal execution evidence is immutable';
END;
$$;

CREATE FUNCTION ledger_validate_withdrawal_chain_observation()
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

    IF NEW.observation_type IN ('finalized_execution', 'finalized_non_execution') THEN
        SELECT min_confirmations INTO required_confirmations
        FROM ledger_withdrawal_confirmation_policies
        WHERE deployment_id = NEW.deployment_id
          AND policy_version = NEW.confirmation_policy_version
          AND effective_at <= NEW.block_timestamp;
        IF required_confirmations IS NULL OR NEW.confirmations < required_confirmations THEN
            RAISE EXCEPTION 'withdrawal chain observation does not satisfy confirmation policy';
        END IF;
    END IF;
    IF NEW.observation_type = 'finalized_execution' AND NEW.transaction_hash IS NULL THEN
        RAISE EXCEPTION 'finalized withdrawal execution requires a transaction hash';
    END IF;
    IF NEW.observation_type = 'finalized_non_execution'
       AND NEW.block_timestamp < authorization_row.expires_at THEN
        RAISE EXCEPTION 'withdrawal non-execution cannot be final before authorization expiry';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_chain_observation_guard
BEFORE INSERT ON ledger_withdrawal_chain_observations
FOR EACH ROW EXECUTE FUNCTION ledger_validate_withdrawal_chain_observation();

CREATE TRIGGER ledger_withdrawal_contract_capabilities_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_contract_capabilities
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_confirmation_policies_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_confirmation_policies
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_broadcast_attempts_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_broadcast_attempts
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_chain_observations_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_chain_observations
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_executions_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_executions
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE TRIGGER ledger_withdrawal_failure_releases_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_failure_releases
FOR EACH ROW EXECUTE FUNCTION ledger_reject_withdrawal_execution_evidence_mutation();

CREATE FUNCTION ledger_validate_executable_withdrawal_authorization()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.envelope_version <> 'vela.withdrawal.authorization.eip712.v2'
       OR NOT EXISTS (
           SELECT 1
           FROM ledger_chain_deployments deployment
           JOIN ledger_withdrawal_contract_capabilities capability
             ON capability.deployment_id = deployment.id
           WHERE deployment.enabled
             AND deployment.chain_id = NEW.chain_id
             AND lower(deployment.vault_address) = lower(NEW.vault_address)
             AND capability.authorization_version = 'eip712_v2'
       ) THEN
        RAISE EXCEPTION 'withdrawal authorization requires a registered EIP-712 V2 deployment';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_executable_withdrawal_authorization_guard
BEFORE INSERT ON ledger_withdrawal_authorizations
FOR EACH ROW EXECUTE FUNCTION ledger_validate_executable_withdrawal_authorization();

CREATE FUNCTION ledger_require_withdrawal_terminal_evidence()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state = 'confirmed' AND NOT EXISTS (
        SELECT 1 FROM ledger_withdrawal_executions execution
        WHERE execution.withdrawal_id = NEW.id
          AND execution.completion_transaction_id = NEW.completion_transaction_id
    ) THEN
        RAISE EXCEPTION 'confirmed withdrawal requires finalized execution evidence';
    END IF;
    IF NEW.state = 'broadcast_failed' AND (
        NEW.release_transaction_id IS NULL
        OR NOT EXISTS (
            SELECT 1 FROM ledger_withdrawal_failure_releases failure
            WHERE failure.withdrawal_id = NEW.id
              AND failure.release_transaction_id = NEW.release_transaction_id
        )
    ) THEN
        RAISE EXCEPTION 'permanently failed withdrawal requires finalized non-execution evidence and release';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_terminal_evidence_guard
BEFORE UPDATE ON ledger_withdrawals
FOR EACH ROW EXECUTE FUNCTION ledger_require_withdrawal_terminal_evidence();

CREATE OR REPLACE FUNCTION ledger_enforce_withdrawal_transition()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.customer_reference <> OLD.customer_reference
       OR NEW.source_account_id <> OLD.source_account_id
       OR NEW.asset_id <> OLD.asset_id
       OR NEW.destination_address <> OLD.destination_address
       OR NEW.amount <> OLD.amount
       OR NEW.chain_id <> OLD.chain_id
       OR NEW.vault_address <> OLD.vault_address
       OR NEW.nonce IS DISTINCT FROM OLD.nonce
       OR NEW.created_at <> OLD.created_at THEN
        RAISE EXCEPTION 'withdrawal identity and amount are immutable';
    END IF;
    IF OLD.reservation_transaction_id IS NOT NULL
       AND NEW.reservation_transaction_id IS DISTINCT FROM OLD.reservation_transaction_id THEN
        RAISE EXCEPTION 'withdrawal reservation transaction is immutable';
    END IF;
    IF OLD.completion_transaction_id IS NOT NULL
       AND NEW.completion_transaction_id IS DISTINCT FROM OLD.completion_transaction_id THEN
        RAISE EXCEPTION 'withdrawal completion transaction is immutable';
    END IF;
    IF OLD.release_transaction_id IS NOT NULL
       AND NEW.release_transaction_id IS DISTINCT FROM OLD.release_transaction_id THEN
        RAISE EXCEPTION 'withdrawal release transaction is immutable';
    END IF;
    IF NEW.state <> OLD.state AND NOT (
        (OLD.state = 'requested' AND NEW.state IN ('reserved', 'rejected', 'cancelled'))
        OR (OLD.state = 'reserved' AND NEW.state IN ('approved', 'cancelled', 'signing_failed', 'reversed'))
        OR (OLD.state = 'approved' AND NEW.state IN ('signing', 'cancelled', 'signing_failed', 'reversed'))
        OR (OLD.state = 'signing' AND NEW.state IN ('signed', 'signing_failed'))
        OR (OLD.state = 'signed' AND NEW.state IN ('broadcast', 'broadcast_failed', 'confirmed'))
        OR (OLD.state = 'broadcast' AND NEW.state IN ('confirmed', 'replaced', 'dropped', 'broadcast_failed'))
        OR (OLD.state = 'replaced' AND NEW.state IN ('confirmed', 'replaced', 'dropped', 'broadcast_failed'))
        OR (OLD.state = 'dropped' AND NEW.state IN ('broadcast', 'replaced', 'confirmed', 'broadcast_failed'))
    ) THEN
        RAISE EXCEPTION 'invalid withdrawal transition from % to %', OLD.state, NEW.state;
    END IF;
    IF NEW.state NOT IN ('requested', 'rejected') AND NEW.reservation_transaction_id IS NULL THEN
        RAISE EXCEPTION 'withdrawal state requires a reservation transaction';
    END IF;
    IF NEW.state IN ('approved', 'signing', 'signed', 'broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed')
       AND NOT EXISTS (SELECT 1 FROM ledger_withdrawal_authorizations WHERE withdrawal_id = NEW.id) THEN
        RAISE EXCEPTION 'withdrawal state requires a durable authorization';
    END IF;
    IF NEW.state IN ('approved', 'signing', 'signed', 'broadcast')
       AND NOT EXISTS (
           SELECT 1 FROM ledger_withdrawal_authorizations
           WHERE withdrawal_id = NEW.id AND expires_at > transaction_timestamp()
       ) THEN
        RAISE EXCEPTION 'pre-confirmation withdrawal state requires an unexpired authorization';
    END IF;
    IF NEW.state IN ('signed', 'broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed')
       AND NOT EXISTS (SELECT 1 FROM ledger_withdrawal_signatures WHERE withdrawal_id = NEW.id) THEN
        RAISE EXCEPTION 'withdrawal state requires immutable custody signature evidence';
    END IF;
    IF NEW.state = 'confirmed' AND NEW.completion_transaction_id IS NULL THEN
        RAISE EXCEPTION 'confirmed withdrawal requires a completion transaction';
    END IF;
    IF NEW.state IN ('cancelled', 'signing_failed', 'reversed') AND NEW.release_transaction_id IS NULL THEN
        RAISE EXCEPTION 'released withdrawal state requires a release transaction';
    END IF;
    IF NEW.transaction_hash IS NOT NULL
       AND NEW.state NOT IN ('broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed') THEN
        RAISE EXCEPTION 'withdrawal transaction hash is only valid after broadcast';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_block_global_withdrawal_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    operation TEXT;
BEGIN
    SELECT operation_type INTO STRICT operation
    FROM ledger_journal_transactions
    WHERE id = NEW.transaction_id;
    IF operation = 'withdrawal_reservation'
       AND EXISTS (
           SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL
       ) THEN
        RAISE EXCEPTION 'global withdrawal halt blocks %', operation
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION ledger_block_halted_asset_operation()
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
              OR (halt.withdrawals_halted AND operation = 'withdrawal_reservation')
          )
    ) THEN
        RAISE EXCEPTION 'asset operation is halted for %', operation
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

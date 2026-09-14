CREATE TABLE ledger_withdrawal_policy_limits (
    policy_version TEXT NOT NULL,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    max_single_amount NUMERIC NOT NULL CHECK (
        max_single_amount > 0 AND max_single_amount = trunc(max_single_amount)
    ),
    max_customer_window_amount NUMERIC NOT NULL CHECK (
        max_customer_window_amount > 0 AND max_customer_window_amount = trunc(max_customer_window_amount)
    ),
    max_customer_window_count BIGINT NOT NULL CHECK (max_customer_window_count > 0),
    customer_window_seconds BIGINT NOT NULL CHECK (customer_window_seconds BETWEEN 1 AND 2592000),
    max_platform_window_amount NUMERIC NOT NULL CHECK (
        max_platform_window_amount > 0 AND max_platform_window_amount = trunc(max_platform_window_amount)
    ),
    platform_window_seconds BIGINT NOT NULL CHECK (platform_window_seconds BETWEEN 1 AND 2592000),
    effective_at TIMESTAMPTZ NOT NULL,
    configured_by TEXT NOT NULL CHECK (length(configured_by) BETWEEN 1 AND 255),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (policy_version, asset_id),
    UNIQUE (asset_id, effective_at),
    CHECK (length(policy_version) BETWEEN 1 AND 255)
);

CREATE INDEX ledger_withdrawal_policy_limits_effective
ON ledger_withdrawal_policy_limits (asset_id, effective_at DESC);

CREATE TABLE ledger_global_withdrawal_halts (
    id UUID PRIMARY KEY,
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1000),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    activated_by TEXT NOT NULL CHECK (length(activated_by) BETWEEN 1 AND 255),
    activated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    resolution_evidence JSONB CHECK (
        resolution_evidence IS NULL OR jsonb_typeof(resolution_evidence) = 'object'
    ),
    resolution_initiated_by TEXT,
    resolved_by TEXT,
    resolved_at TIMESTAMPTZ,
    CHECK (
        (resolution_evidence IS NULL AND resolution_initiated_by IS NULL
            AND resolved_by IS NULL AND resolved_at IS NULL)
        OR
        (resolution_evidence IS NOT NULL AND length(resolution_initiated_by) > 0
            AND length(resolved_by) > 0 AND resolved_at IS NOT NULL
            AND lower(activated_by) <> lower(resolution_initiated_by)
            AND lower(resolution_initiated_by) <> lower(resolved_by)
            AND lower(activated_by) <> lower(resolved_by))
    )
);

CREATE UNIQUE INDEX ledger_global_withdrawal_halts_one_active
ON ledger_global_withdrawal_halts ((true))
WHERE resolved_at IS NULL;

CREATE FUNCTION ledger_block_global_withdrawal_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    operation TEXT;
BEGIN
    SELECT operation_type INTO STRICT operation
    FROM ledger_journal_transactions
    WHERE id = NEW.transaction_id;
    IF operation IN ('withdrawal_reservation', 'withdrawal_completion')
       AND EXISTS (
           SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL
       ) THEN
        RAISE EXCEPTION 'global withdrawal halt blocks %', operation
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_journal_leg_global_withdrawal_halt_guard
BEFORE INSERT ON ledger_journal_legs
FOR EACH ROW EXECUTE FUNCTION ledger_block_global_withdrawal_halt();

CREATE TABLE ledger_withdrawal_policy_decisions (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawals(id),
    policy_version TEXT NOT NULL,
    disposition TEXT NOT NULL CHECK (disposition IN ('approved', 'rejected', 'review')),
    compliance_provider TEXT NOT NULL CHECK (length(compliance_provider) BETWEEN 1 AND 255),
    compliance_reference TEXT NOT NULL CHECK (length(compliance_reference) BETWEEN 1 AND 1000),
    compliance_disposition TEXT NOT NULL CHECK (
        compliance_disposition IN ('clear', 'reject', 'review', 'unavailable')
    ),
    reason_codes JSONB NOT NULL CHECK (jsonb_typeof(reason_codes) = 'array'),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    evidence_hash BYTEA NOT NULL CHECK (octet_length(evidence_hash) = 32),
    evaluated_by TEXT NOT NULL CHECK (length(evaluated_by) BETWEEN 1 AND 255),
    evaluated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE FUNCTION ledger_guard_withdrawal_policy_limit()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'withdrawal policy limits are immutable; publish a new version';
END;
$$;

CREATE TRIGGER ledger_withdrawal_policy_limits_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_policy_limits
FOR EACH ROW EXECUTE FUNCTION ledger_guard_withdrawal_policy_limit();

CREATE FUNCTION ledger_guard_global_withdrawal_halt()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'global withdrawal halt evidence cannot be deleted';
    END IF;
    IF NEW.id <> OLD.id
       OR NEW.reason <> OLD.reason
       OR NEW.evidence <> OLD.evidence
       OR NEW.activated_by <> OLD.activated_by
       OR NEW.activated_at <> OLD.activated_at
       OR OLD.resolved_at IS NOT NULL
       OR NEW.resolution_evidence IS NULL
       OR NEW.resolution_initiated_by IS NULL
       OR NEW.resolved_by IS NULL
       OR NEW.resolved_at IS NULL
       OR lower(NEW.activated_by) = lower(NEW.resolution_initiated_by)
       OR lower(NEW.resolution_initiated_by) = lower(NEW.resolved_by)
       OR lower(NEW.activated_by) = lower(NEW.resolved_by) THEN
        RAISE EXCEPTION 'global withdrawal halt is immutable except for three-party resolution';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_global_withdrawal_halts_guard
BEFORE UPDATE OR DELETE ON ledger_global_withdrawal_halts
FOR EACH ROW EXECUTE FUNCTION ledger_guard_global_withdrawal_halt();

CREATE FUNCTION ledger_validate_withdrawal_policy_decision()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    withdrawal ledger_withdrawals%ROWTYPE;
    active_policy_version TEXT;
BEGIN
    SELECT * INTO withdrawal
    FROM ledger_withdrawals
    WHERE id = NEW.withdrawal_id
    FOR SHARE;

    IF NOT FOUND OR withdrawal.state <> 'reserved' THEN
        RAISE EXCEPTION 'withdrawal policy decision requires a reserved withdrawal';
    END IF;

    SELECT policy_version INTO active_policy_version
    FROM ledger_withdrawal_policy_limits
    WHERE asset_id = withdrawal.asset_id
      AND effective_at <= transaction_timestamp()
    ORDER BY effective_at DESC
    LIMIT 1;

    IF active_policy_version IS NULL OR NEW.policy_version <> active_policy_version THEN
        RAISE EXCEPTION 'withdrawal policy decision does not use the active asset policy';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_policy_decision_guard
BEFORE INSERT ON ledger_withdrawal_policy_decisions
FOR EACH ROW EXECUTE FUNCTION ledger_validate_withdrawal_policy_decision();

CREATE TRIGGER ledger_withdrawal_policy_decisions_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_policy_decisions
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE OR REPLACE FUNCTION ledger_validate_withdrawal_authorization()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    withdrawal ledger_withdrawals%ROWTYPE;
    decision ledger_withdrawal_policy_decisions%ROWTYPE;
    expected_asset_address TEXT;
    active_policy_version TEXT;
BEGIN
    SELECT * INTO withdrawal
    FROM ledger_withdrawals
    WHERE id = NEW.withdrawal_id
    FOR SHARE;

    IF NOT FOUND OR withdrawal.state <> 'reserved' THEN
        RAISE EXCEPTION 'withdrawal authorization requires a reserved withdrawal';
    END IF;
    SELECT COALESCE(contract_address, '0x0000000000000000000000000000000000000000')
    INTO expected_asset_address
    FROM ledger_assets
    WHERE id = withdrawal.asset_id;
    IF NEW.chain_id <> withdrawal.chain_id
       OR lower(NEW.vault_address) <> lower(withdrawal.vault_address)
       OR lower(NEW.customer_reference) <> lower(withdrawal.customer_reference)
       OR lower(NEW.destination_address) <> lower(withdrawal.destination_address)
       OR NEW.asset_id <> withdrawal.asset_id
       OR lower(NEW.asset_address) <> lower(expected_asset_address)
       OR NEW.amount <> withdrawal.amount
       OR NEW.nonce IS DISTINCT FROM withdrawal.nonce THEN
        RAISE EXCEPTION 'withdrawal authorization does not match immutable withdrawal facts';
    END IF;
    IF NEW.expires_at <= transaction_timestamp() THEN
        RAISE EXCEPTION 'withdrawal authorization approval or expiry is invalid';
    END IF;

    SELECT * INTO decision
    FROM ledger_withdrawal_policy_decisions
    WHERE withdrawal_id = NEW.withdrawal_id
    FOR SHARE;
    SELECT policy_version INTO active_policy_version
    FROM ledger_withdrawal_policy_limits
    WHERE asset_id = withdrawal.asset_id
      AND effective_at <= transaction_timestamp()
    ORDER BY effective_at DESC
    LIMIT 1;
    IF decision.id IS NULL
       OR decision.disposition <> 'approved'
       OR decision.policy_version <> NEW.policy_version
       OR decision.evaluated_by <> NEW.approved_by
       OR active_policy_version IS NULL
       OR decision.policy_version <> active_policy_version THEN
        RAISE EXCEPTION 'withdrawal authorization requires current approved policy evidence';
    END IF;
    IF EXISTS (SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL)
       OR EXISTS (
           SELECT 1 FROM ledger_asset_halts
           WHERE asset_id = withdrawal.asset_id
             AND withdrawals_halted
             AND resolved_at IS NULL
       ) THEN
        RAISE EXCEPTION 'withdrawal authorization is halted';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION ledger_enforce_withdrawal_policy_state()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state IN ('approved', 'signing', 'signed') AND (
        EXISTS (SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL)
        OR EXISTS (
            SELECT 1 FROM ledger_asset_halts
            WHERE asset_id = NEW.asset_id
              AND withdrawals_halted
              AND resolved_at IS NULL
        )
    ) THEN
        RAISE EXCEPTION 'withdrawal signing is halted';
    END IF;
    IF NEW.state IN ('approved', 'signing', 'signed') AND NOT EXISTS (
        SELECT 1
        FROM ledger_withdrawal_policy_decisions decision
        JOIN ledger_withdrawal_authorizations authz
          ON authz.withdrawal_id = decision.withdrawal_id
         AND authz.policy_version = decision.policy_version
         AND authz.approved_by = decision.evaluated_by
        WHERE decision.withdrawal_id = NEW.id
          AND decision.disposition = 'approved'
          AND decision.policy_version = (
              SELECT policy_version
              FROM ledger_withdrawal_policy_limits
              WHERE asset_id = NEW.asset_id
                AND effective_at <= transaction_timestamp()
              ORDER BY effective_at DESC
              LIMIT 1
          )
    ) THEN
        RAISE EXCEPTION 'withdrawal state requires approved policy evidence';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_policy_state_guard
BEFORE UPDATE ON ledger_withdrawals
FOR EACH ROW EXECUTE FUNCTION ledger_enforce_withdrawal_policy_state();

CREATE OR REPLACE FUNCTION ledger_validate_withdrawal_signature()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    authorization_record ledger_withdrawal_authorizations%ROWTYPE;
    withdrawal ledger_withdrawals%ROWTYPE;
BEGIN
    SELECT * INTO authorization_record
    FROM ledger_withdrawal_authorizations
    WHERE id = NEW.authorization_id
    FOR SHARE;

    SELECT * INTO withdrawal
    FROM ledger_withdrawals
    WHERE id = NEW.withdrawal_id
    FOR SHARE;

    IF NOT FOUND OR withdrawal.state <> 'signing' THEN
        RAISE EXCEPTION 'withdrawal signature requires a signing withdrawal';
    END IF;
    IF authorization_record.id IS NULL
       OR authorization_record.withdrawal_id <> NEW.withdrawal_id
       OR authorization_record.digest <> NEW.authorization_digest THEN
        RAISE EXCEPTION 'withdrawal signature does not match its authorization';
    END IF;
    IF authorization_record.expires_at <= NEW.signed_at THEN
        RAISE EXCEPTION 'withdrawal signature time is invalid';
    END IF;
    IF EXISTS (SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL)
       OR EXISTS (
           SELECT 1 FROM ledger_asset_halts
           WHERE asset_id = withdrawal.asset_id
             AND withdrawals_halted
             AND resolved_at IS NULL
       ) THEN
        RAISE EXCEPTION 'withdrawal signature is halted';
    END IF;
    RETURN NEW;
END;
$$;

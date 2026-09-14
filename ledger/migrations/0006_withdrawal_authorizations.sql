CREATE TABLE ledger_withdrawal_authorizations (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawals(id),
    envelope_version TEXT NOT NULL CHECK (envelope_version = 'vela.withdrawal.authorization.v1'),
    chain_id BIGINT NOT NULL,
    vault_address TEXT NOT NULL,
    customer_reference TEXT NOT NULL,
    destination_address TEXT NOT NULL,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    asset_address TEXT NOT NULL,
    amount NUMERIC NOT NULL CHECK (amount > 0 AND amount = trunc(amount)),
    nonce NUMERIC NOT NULL CHECK (nonce >= 0 AND nonce = trunc(nonce)),
    expires_at TIMESTAMPTZ NOT NULL,
    canonical_payload TEXT NOT NULL,
    digest BYTEA NOT NULL CHECK (octet_length(digest) = 32),
    policy_version TEXT NOT NULL,
    approved_by TEXT NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (digest),
    CHECK (length(policy_version) BETWEEN 1 AND 255),
    CHECK (length(approved_by) BETWEEN 1 AND 255),
    CHECK (expires_at = date_trunc('second', expires_at)),
    CHECK (expires_at > approved_at)
);

CREATE TABLE ledger_withdrawal_signatures (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawals(id),
    authorization_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_authorizations(id),
    authorization_digest BYTEA NOT NULL CHECK (octet_length(authorization_digest) = 32),
    signer_key_id TEXT NOT NULL,
    signer_public_key BYTEA NOT NULL CHECK (octet_length(signer_public_key) IN (33, 65)),
    algorithm TEXT NOT NULL CHECK (algorithm = 'secp256k1_recoverable'),
    signature BYTEA NOT NULL CHECK (octet_length(signature) = 65),
    signed_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK (length(signer_key_id) BETWEEN 1 AND 255)
);

CREATE FUNCTION ledger_validate_withdrawal_authorization()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    withdrawal ledger_withdrawals%ROWTYPE;
    expected_asset_address TEXT;
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
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_authorization_guard
BEFORE INSERT ON ledger_withdrawal_authorizations
FOR EACH ROW EXECUTE FUNCTION ledger_validate_withdrawal_authorization();

CREATE FUNCTION ledger_validate_withdrawal_signature()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    authorization_record ledger_withdrawal_authorizations%ROWTYPE;
    withdrawal_state TEXT;
BEGIN
    SELECT * INTO authorization_record
    FROM ledger_withdrawal_authorizations
    WHERE id = NEW.authorization_id
    FOR SHARE;

    SELECT state INTO withdrawal_state
    FROM ledger_withdrawals
    WHERE id = NEW.withdrawal_id
    FOR SHARE;

    IF NOT FOUND OR withdrawal_state <> 'signing' THEN
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
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_signature_guard
BEFORE INSERT ON ledger_withdrawal_signatures
FOR EACH ROW EXECUTE FUNCTION ledger_validate_withdrawal_signature();

CREATE TRIGGER ledger_withdrawal_authorizations_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_authorizations
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_withdrawal_signatures_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_signatures
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

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
        OR (OLD.state = 'signed' AND NEW.state IN ('broadcast', 'broadcast_failed'))
        OR (OLD.state = 'broadcast' AND NEW.state IN ('confirmed', 'replaced', 'dropped', 'broadcast_failed'))
        OR (OLD.state = 'replaced' AND NEW.state IN ('confirmed', 'replaced', 'dropped'))
        OR (OLD.state = 'dropped' AND NEW.state IN ('broadcast', 'replaced'))
    ) THEN
        RAISE EXCEPTION 'invalid withdrawal transition from % to %', OLD.state, NEW.state;
    END IF;

    IF NEW.state NOT IN ('requested', 'rejected')
       AND NEW.reservation_transaction_id IS NULL THEN
        RAISE EXCEPTION 'withdrawal state requires a reservation transaction';
    END IF;
    IF NEW.state IN ('approved', 'signing', 'signed', 'broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed')
       AND NOT EXISTS (
           SELECT 1 FROM ledger_withdrawal_authorizations authz
           WHERE authz.withdrawal_id = NEW.id
       ) THEN
        RAISE EXCEPTION 'withdrawal state requires a durable authorization';
    END IF;
    IF NEW.state IN ('approved', 'signing', 'signed', 'broadcast')
       AND NOT EXISTS (
           SELECT 1 FROM ledger_withdrawal_authorizations authz
           WHERE authz.withdrawal_id = NEW.id
             AND authz.expires_at > transaction_timestamp()
       ) THEN
        RAISE EXCEPTION 'pre-confirmation withdrawal state requires an unexpired authorization';
    END IF;
    IF NEW.state IN ('signed', 'broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed')
       AND NOT EXISTS (
           SELECT 1 FROM ledger_withdrawal_signatures signature_record
           WHERE signature_record.withdrawal_id = NEW.id
       ) THEN
        RAISE EXCEPTION 'withdrawal state requires immutable custody signature evidence';
    END IF;
    IF NEW.state = 'confirmed' AND NEW.completion_transaction_id IS NULL THEN
        RAISE EXCEPTION 'confirmed withdrawal requires a completion transaction';
    END IF;
    IF NEW.state IN ('cancelled', 'signing_failed', 'reversed')
       AND NEW.release_transaction_id IS NULL THEN
        RAISE EXCEPTION 'released withdrawal state requires a release transaction';
    END IF;
    IF NEW.transaction_hash IS NOT NULL
       AND NEW.state NOT IN ('broadcast', 'confirmed', 'replaced', 'dropped', 'broadcast_failed') THEN
        RAISE EXCEPTION 'withdrawal transaction hash is only valid after broadcast';
    END IF;

    RETURN NEW;
END;
$$;

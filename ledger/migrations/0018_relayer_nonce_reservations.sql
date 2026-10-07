-- Immutable relayer allocation. A nonce belongs to one signed withdrawal for
-- the lifetime of the ledger, including after an uncertain submission.
CREATE TABLE ledger_relayer_nonce_reservations (
    id UUID PRIMARY KEY,
    withdrawal_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_broadcast_queue(withdrawal_id),
    chain_id BIGINT NOT NULL CHECK (chain_id = 84532),
    sender_address TEXT NOT NULL CHECK (sender_address ~ '^0x[0-9a-f]{40}$'),
    sender_nonce NUMERIC NOT NULL CHECK (sender_nonce >= 0 AND sender_nonce = trunc(sender_nonce)),
    gas_limit BIGINT NOT NULL CHECK (gas_limit > 0),
    max_fee_per_gas_wei NUMERIC NOT NULL CHECK (max_fee_per_gas_wei > 0),
    max_priority_fee_per_gas_wei NUMERIC NOT NULL CHECK (
        max_priority_fee_per_gas_wei > 0
        AND max_priority_fee_per_gas_wei <= max_fee_per_gas_wei
    ),
    maximum_cost_wei NUMERIC NOT NULL CHECK (
        maximum_cost_wei = gas_limit * max_fee_per_gas_wei
    ),
    primary_provider TEXT NOT NULL CHECK (length(primary_provider) BETWEEN 1 AND 255),
    secondary_provider TEXT NOT NULL CHECK (length(secondary_provider) BETWEEN 1 AND 255),
    allocated_by TEXT NOT NULL CHECK (length(allocated_by) BETWEEN 1 AND 255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (chain_id, sender_address, sender_nonce),
    CHECK (lower(primary_provider) <> lower(secondary_provider))
);

CREATE FUNCTION ledger_validate_relayer_nonce_reservation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ledger_withdrawal_broadcast_queue queue
        JOIN ledger_withdrawals withdrawal ON withdrawal.id = queue.withdrawal_id
        JOIN ledger_withdrawal_authorizations authz ON authz.id = queue.authorization_id
        WHERE queue.withdrawal_id = NEW.withdrawal_id
          AND withdrawal.state = 'signed'
          AND authz.chain_id = NEW.chain_id
          AND authz.expires_at > transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'relayer nonce reservation requires a current queued signed authorization';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_relayer_nonce_reservation_guard
BEFORE INSERT ON ledger_relayer_nonce_reservations
FOR EACH ROW EXECUTE FUNCTION ledger_validate_relayer_nonce_reservation();

CREATE TRIGGER ledger_relayer_nonce_reservation_immutable
BEFORE UPDATE OR DELETE ON ledger_relayer_nonce_reservations
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

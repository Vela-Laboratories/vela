CREATE UNIQUE INDEX ledger_withdrawals_customer_nonce
ON ledger_withdrawals (lower(customer_reference), nonce)
WHERE nonce IS NOT NULL;

CREATE UNIQUE INDEX ledger_accounts_casefold_owner_identity
ON ledger_accounts (asset_id, owner_type, lower(owner_reference), account_type)
WHERE owner_type IN ('customer', 'custody');

CREATE FUNCTION ledger_enforce_withdrawal_transition()
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

CREATE TRIGGER ledger_withdrawal_transition_guard
BEFORE UPDATE ON ledger_withdrawals
FOR EACH ROW EXECUTE FUNCTION ledger_enforce_withdrawal_transition();

CREATE FUNCTION ledger_require_withdrawal_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT EXISTS (
            SELECT 1 FROM ledger_withdrawal_state_history history
            WHERE history.withdrawal_id = NEW.id
              AND history.from_state IS NULL
              AND history.to_state = NEW.state
              AND history.created_at = transaction_timestamp()
        ) THEN
            RAISE EXCEPTION 'initial withdrawal state requires immutable history';
        END IF;
    ELSIF NEW.state <> OLD.state AND NOT EXISTS (
        SELECT 1 FROM ledger_withdrawal_state_history history
        WHERE history.withdrawal_id = NEW.id
          AND history.from_state = OLD.state
          AND history.to_state = NEW.state
          AND history.created_at = transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'withdrawal transition from % to % requires immutable history', OLD.state, NEW.state;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_withdrawal_history_required
AFTER INSERT OR UPDATE ON ledger_withdrawals
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_require_withdrawal_history();

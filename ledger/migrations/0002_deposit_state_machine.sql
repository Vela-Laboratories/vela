CREATE FUNCTION ledger_enforce_deposit_transition()
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

    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_deposit_transition_guard
BEFORE UPDATE ON ledger_deposits
FOR EACH ROW EXECUTE FUNCTION ledger_enforce_deposit_transition();

CREATE UNIQUE INDEX ledger_deposit_chain_events_canonical_identity
ON ledger_deposit_chain_events (chain_id, lower(transaction_hash), log_index);

CREATE FUNCTION ledger_require_deposit_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT EXISTS (
            SELECT 1 FROM ledger_deposit_state_history history
            WHERE history.deposit_id = NEW.id
              AND history.from_state IS NULL
              AND history.to_state = NEW.state
              AND history.created_at = transaction_timestamp()
        ) THEN
            RAISE EXCEPTION 'initial deposit state requires immutable history';
        END IF;
    ELSIF NEW.state <> OLD.state AND NOT EXISTS (
        SELECT 1 FROM ledger_deposit_state_history history
        WHERE history.deposit_id = NEW.id
          AND history.from_state = OLD.state
          AND history.to_state = NEW.state
          AND history.created_at = transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'deposit transition from % to % requires immutable history', OLD.state, NEW.state;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_deposit_history_required
AFTER INSERT OR UPDATE ON ledger_deposits
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_require_deposit_history();

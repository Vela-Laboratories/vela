-- Persist exact signed bytes before any network send. A response timeout is
-- ambiguous and must never erase the prepared transaction or nonce claim.
CREATE TABLE ledger_relayer_prepared_transactions (
    id UUID PRIMARY KEY,
    reservation_id UUID NOT NULL UNIQUE REFERENCES ledger_relayer_nonce_reservations(id),
    transaction_hash TEXT NOT NULL UNIQUE CHECK (transaction_hash ~ '^0x[0-9a-f]{64}$'),
    signed_transaction BYTEA NOT NULL CHECK (
        octet_length(signed_transaction) BETWEEN 1 AND 32768
    ),
    prepared_by TEXT NOT NULL CHECK (length(prepared_by) BETWEEN 1 AND 255),
    prepared_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_relayer_submission_results (
    prepared_id UUID PRIMARY KEY REFERENCES ledger_relayer_prepared_transactions(id),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'uncertain')),
    provider_name TEXT NOT NULL CHECK (length(provider_name) BETWEEN 1 AND 255),
    provider_reference TEXT CHECK (
        provider_reference IS NULL OR length(provider_reference) BETWEEN 1 AND 1000
    ),
    submitted_by TEXT NOT NULL CHECK (length(submitted_by) BETWEEN 1 AND 255),
    submitted_at TIMESTAMPTZ NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK (outcome <> 'accepted' OR provider_reference IS NOT NULL)
);

CREATE FUNCTION ledger_validate_prepared_relayer_transaction()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ledger_relayer_nonce_reservations reservation
        JOIN ledger_withdrawals withdrawal ON withdrawal.id = reservation.withdrawal_id
        JOIN ledger_withdrawal_authorizations authz
          ON authz.withdrawal_id = withdrawal.id
        WHERE reservation.id = NEW.reservation_id
          AND withdrawal.state = 'signed'
          AND authz.expires_at > transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'prepared relay requires a current signed withdrawal';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_relayer_prepared_transaction_guard
BEFORE INSERT ON ledger_relayer_prepared_transactions
FOR EACH ROW EXECUTE FUNCTION ledger_validate_prepared_relayer_transaction();

CREATE TRIGGER ledger_relayer_prepared_transactions_immutable
BEFORE UPDATE OR DELETE ON ledger_relayer_prepared_transactions
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_relayer_submission_results_immutable
BEFORE UPDATE OR DELETE ON ledger_relayer_submission_results
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

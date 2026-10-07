-- A signed V3 authorization enters a durable queue in the same transaction
-- that stores its recoverable custody signature. This table is an intent
-- boundary only: it does not authorize a relayer, nonce, fee, or broadcast.
CREATE TABLE ledger_withdrawal_broadcast_queue (
    withdrawal_id UUID PRIMARY KEY REFERENCES ledger_withdrawals(id),
    authorization_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_authorizations(id),
    signature_id UUID NOT NULL UNIQUE REFERENCES ledger_withdrawal_signatures(id),
    queued_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE FUNCTION ledger_validate_withdrawal_broadcast_queue()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ledger_withdrawals withdrawal
        JOIN ledger_withdrawal_authorizations authz
          ON authz.withdrawal_id = withdrawal.id
        JOIN ledger_withdrawal_signatures signature_record
          ON signature_record.withdrawal_id = withdrawal.id
         AND signature_record.authorization_id = authz.id
         AND signature_record.authorization_digest = authz.digest
        WHERE withdrawal.id = NEW.withdrawal_id
          AND withdrawal.state = 'signing'
          AND authz.id = NEW.authorization_id
          AND signature_record.id = NEW.signature_id
          AND authz.envelope_version = 'vela.withdrawal.authorization.eip712.v3'
          AND authz.chain_id = 84532
          AND authz.expires_at > transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'broadcast queue requires a current signed Base Sepolia V3 authorization';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_broadcast_queue_guard
BEFORE INSERT ON ledger_withdrawal_broadcast_queue
FOR EACH ROW EXECUTE FUNCTION ledger_validate_withdrawal_broadcast_queue();

CREATE TRIGGER ledger_withdrawal_broadcast_queue_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_broadcast_queue
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

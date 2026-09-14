CREATE TABLE ledger_withdrawal_worker_checkpoints (
    deployment_id UUID PRIMARY KEY REFERENCES ledger_withdrawal_contract_capabilities(deployment_id),
    lease_owner UUID,
    lease_expires_at TIMESTAMPTZ,
    scan_cycle BIGINT NOT NULL DEFAULT 0 CHECK (scan_cycle >= 0),
    last_withdrawal_id UUID,
    last_withdrawal_updated_at TIMESTAMPTZ,
    last_completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((last_withdrawal_id IS NULL) = (last_withdrawal_updated_at IS NULL))
);

CREATE FUNCTION ledger_guard_withdrawal_worker_checkpoint()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'withdrawal worker checkpoints cannot be deleted';
    END IF;
    IF NEW.deployment_id <> OLD.deployment_id OR NEW.scan_cycle < OLD.scan_cycle THEN
        RAISE EXCEPTION 'withdrawal worker checkpoint identity and cycle are monotonic';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER ledger_withdrawal_worker_checkpoint_guard
BEFORE UPDATE OR DELETE ON ledger_withdrawal_worker_checkpoints
FOR EACH ROW EXECUTE FUNCTION ledger_guard_withdrawal_worker_checkpoint();

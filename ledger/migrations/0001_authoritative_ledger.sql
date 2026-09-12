CREATE TABLE ledger_assets (
    id UUID PRIMARY KEY,
    code TEXT NOT NULL UNIQUE CHECK (code = upper(code) AND code ~ '^[A-Z0-9]{2,16}$'),
    decimals SMALLINT NOT NULL CHECK (decimals BETWEEN 0 AND 77),
    chain_id BIGINT,
    contract_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK ((contract_address IS NULL) OR (chain_id IS NOT NULL))
);

CREATE TABLE ledger_accounts (
    id UUID PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    owner_type TEXT NOT NULL CHECK (owner_type IN ('customer', 'custody', 'platform')),
    owner_reference TEXT NOT NULL CHECK (length(owner_reference) BETWEEN 1 AND 255),
    account_type TEXT NOT NULL CHECK (account_type IN (
        'customer_available',
        'customer_locked',
        'customer_pending',
        'customer_withdrawal_reserved',
        'custody_vault',
        'custody_withdrawn',
        'fee_revenue',
        'rebate_expense',
        'operational_adjustment'
    )),
    allow_negative BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (id, asset_id),
    UNIQUE (asset_id, owner_type, owner_reference, account_type),
    CHECK (owner_type <> 'customer' OR NOT allow_negative),
    CHECK (
        (owner_type = 'customer' AND account_type IN (
            'customer_available', 'customer_locked', 'customer_pending',
            'customer_withdrawal_reserved'
        ))
        OR (owner_type = 'custody' AND account_type IN ('custody_vault', 'custody_withdrawn'))
        OR (owner_type = 'platform' AND account_type IN (
            'fee_revenue', 'rebate_expense', 'operational_adjustment'
        ))
    )
);

CREATE TABLE ledger_account_balances (
    account_id UUID PRIMARY KEY REFERENCES ledger_accounts(id),
    balance NUMERIC NOT NULL DEFAULT 0
        CHECK (balance = trunc(balance) AND abs(balance) < power(10::numeric, 78)),
    version BIGINT NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_journal_transactions (
    id UUID PRIMARY KEY,
    operation_type TEXT NOT NULL CHECK (operation_type IN (
        'deposit_credit',
        'deposit_reversal',
        'order_reservation',
        'order_release',
        'trade',
        'fee',
        'rebate',
        'withdrawal_reservation',
        'withdrawal_completion',
        'withdrawal_release',
        'administrative_adjustment'
    )),
    idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 255),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    external_reference JSONB,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_journal_legs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    transaction_id UUID NOT NULL REFERENCES ledger_journal_transactions(id),
    account_id UUID NOT NULL,
    asset_id UUID NOT NULL,
    amount NUMERIC NOT NULL CHECK (
        amount <> 0
        AND amount = trunc(amount)
        AND abs(amount) < power(10::numeric, 78)
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    FOREIGN KEY (account_id, asset_id) REFERENCES ledger_accounts(id, asset_id),
    UNIQUE (transaction_id, account_id)
);

CREATE INDEX ledger_journal_legs_account_sequence
    ON ledger_journal_legs (account_id, id);

CREATE TABLE ledger_outbox (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    transaction_id UUID NOT NULL UNIQUE REFERENCES ledger_journal_transactions(id),
    event_type TEXT NOT NULL CHECK (length(event_type) BETWEEN 1 AND 100),
    payload JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ
);

CREATE TABLE ledger_deposits (
    id UUID PRIMARY KEY,
    customer_reference TEXT NOT NULL,
    destination_account_id UUID NOT NULL,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    amount NUMERIC NOT NULL CHECK (amount > 0 AND amount = trunc(amount)),
    state TEXT NOT NULL CHECK (state IN (
        'observed', 'confirming', 'finalized', 'credited',
        'rejected', 'orphaned', 'reversed', 'quarantined'
    )),
    credit_transaction_id UUID UNIQUE REFERENCES ledger_journal_transactions(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    FOREIGN KEY (destination_account_id, asset_id) REFERENCES ledger_accounts(id, asset_id)
);

CREATE TABLE ledger_deposit_chain_events (
    id UUID PRIMARY KEY,
    deposit_id UUID NOT NULL UNIQUE REFERENCES ledger_deposits(id),
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL CHECK (block_number >= 0),
    block_hash TEXT NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL CHECK (transaction_index >= 0),
    log_index INTEGER NOT NULL CHECK (log_index >= 0),
    vault_address TEXT NOT NULL,
    vault_deposit_id TEXT NOT NULL UNIQUE,
    sender_address TEXT NOT NULL,
    asset_address TEXT,
    actual_amount NUMERIC NOT NULL CHECK (actual_amount > 0 AND actual_amount = trunc(actual_amount)),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (chain_id, transaction_hash, log_index)
);

CREATE TABLE ledger_deposit_state_history (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    deposit_id UUID NOT NULL REFERENCES ledger_deposits(id),
    from_state TEXT CHECK (from_state IS NULL OR from_state IN (
        'observed', 'confirming', 'finalized', 'credited',
        'rejected', 'orphaned', 'reversed', 'quarantined'
    )),
    to_state TEXT NOT NULL CHECK (to_state IN (
        'observed', 'confirming', 'finalized', 'credited',
        'rejected', 'orphaned', 'reversed', 'quarantined'
    )),
    reason TEXT,
    evidence JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_withdrawals (
    id UUID PRIMARY KEY,
    customer_reference TEXT NOT NULL,
    source_account_id UUID NOT NULL,
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    destination_address TEXT NOT NULL,
    amount NUMERIC NOT NULL CHECK (amount > 0 AND amount = trunc(amount)),
    state TEXT NOT NULL CHECK (state IN (
        'requested', 'reserved', 'approved', 'signing', 'signed', 'broadcast', 'confirmed',
        'rejected', 'cancelled', 'signing_failed', 'broadcast_failed', 'replaced', 'dropped', 'reversed'
    )),
    reservation_transaction_id UUID UNIQUE REFERENCES ledger_journal_transactions(id),
    completion_transaction_id UUID UNIQUE REFERENCES ledger_journal_transactions(id),
    release_transaction_id UUID UNIQUE REFERENCES ledger_journal_transactions(id),
    chain_id BIGINT NOT NULL,
    vault_address TEXT NOT NULL,
    nonce NUMERIC CHECK (nonce IS NULL OR (nonce >= 0 AND nonce = trunc(nonce))),
    expires_at TIMESTAMPTZ,
    transaction_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    FOREIGN KEY (source_account_id, asset_id) REFERENCES ledger_accounts(id, asset_id)
);

CREATE TABLE ledger_withdrawal_state_history (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    withdrawal_id UUID NOT NULL REFERENCES ledger_withdrawals(id),
    from_state TEXT CHECK (from_state IS NULL OR from_state IN (
        'requested', 'reserved', 'approved', 'signing', 'signed', 'broadcast', 'confirmed',
        'rejected', 'cancelled', 'signing_failed', 'broadcast_failed', 'replaced', 'dropped', 'reversed'
    )),
    to_state TEXT NOT NULL CHECK (to_state IN (
        'requested', 'reserved', 'approved', 'signing', 'signed', 'broadcast', 'confirmed',
        'rejected', 'cancelled', 'signing_failed', 'broadcast_failed', 'replaced', 'dropped', 'reversed'
    )),
    reason TEXT,
    evidence JSONB,
    actor_reference TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()
);

CREATE TABLE ledger_custody_snapshots (
    id UUID PRIMARY KEY,
    chain_id BIGINT NOT NULL,
    block_number BIGINT NOT NULL CHECK (block_number >= 0),
    block_hash TEXT NOT NULL,
    vault_address TEXT NOT NULL,
    deployment_version TEXT NOT NULL,
    input_hash BYTEA NOT NULL CHECK (octet_length(input_hash) = 32),
    captured_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (chain_id, vault_address, block_hash)
);

CREATE TABLE ledger_custody_snapshot_balances (
    snapshot_id UUID NOT NULL REFERENCES ledger_custody_snapshots(id),
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    amount NUMERIC NOT NULL CHECK (amount >= 0 AND amount = trunc(amount)),
    PRIMARY KEY (snapshot_id, asset_id)
);

CREATE TABLE ledger_reconciliation_results (
    id UUID PRIMARY KEY,
    snapshot_id UUID NOT NULL REFERENCES ledger_custody_snapshots(id),
    asset_id UUID NOT NULL REFERENCES ledger_assets(id),
    custody_amount NUMERIC NOT NULL CHECK (custody_amount >= 0 AND custody_amount = trunc(custody_amount)),
    liability_amount NUMERIC NOT NULL CHECK (liability_amount >= 0 AND liability_amount = trunc(liability_amount)),
    difference NUMERIC NOT NULL CHECK (difference = trunc(difference)),
    status TEXT NOT NULL CHECK (status IN ('balanced', 'surplus', 'deficit', 'indeterminate')),
    evidence JSONB NOT NULL CHECK (jsonb_typeof(evidence) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    UNIQUE (snapshot_id, asset_id),
    CHECK (difference = custody_amount - liability_amount)
);

CREATE TABLE ledger_administrative_corrections (
    id UUID PRIMARY KEY,
    journal_transaction_id UUID NOT NULL UNIQUE REFERENCES ledger_journal_transactions(id),
    reason_type TEXT NOT NULL,
    reason TEXT NOT NULL,
    evidence_reference TEXT NOT NULL,
    incident_id TEXT NOT NULL,
    initiated_by TEXT NOT NULL,
    approved_by TEXT NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CHECK (initiated_by <> approved_by)
);

CREATE FUNCTION ledger_reject_journal_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'journal rows are immutable';
END;
$$;

CREATE TRIGGER ledger_journal_transactions_immutable
BEFORE UPDATE OR DELETE ON ledger_journal_transactions
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_journal_legs_immutable
BEFORE UPDATE OR DELETE ON ledger_journal_legs
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_deposit_chain_events_immutable
BEFORE UPDATE OR DELETE ON ledger_deposit_chain_events
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_deposit_state_history_immutable
BEFORE UPDATE OR DELETE ON ledger_deposit_state_history
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_withdrawal_state_history_immutable
BEFORE UPDATE OR DELETE ON ledger_withdrawal_state_history
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_custody_snapshots_immutable
BEFORE UPDATE OR DELETE ON ledger_custody_snapshots
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_custody_snapshot_balances_immutable
BEFORE UPDATE OR DELETE ON ledger_custody_snapshot_balances
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_reconciliation_results_immutable
BEFORE UPDATE OR DELETE ON ledger_reconciliation_results
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE TRIGGER ledger_administrative_corrections_immutable
BEFORE UPDATE OR DELETE ON ledger_administrative_corrections
FOR EACH ROW EXECUTE FUNCTION ledger_reject_journal_mutation();

CREATE FUNCTION ledger_assert_transaction_balanced()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    target_transaction_id UUID;
    leg_count BIGINT;
BEGIN
    target_transaction_id := NEW.id;

    SELECT count(*) INTO leg_count
    FROM ledger_journal_legs
    WHERE transaction_id = target_transaction_id;

    IF leg_count < 2 THEN
        RAISE EXCEPTION 'journal transaction % must contain at least two legs', target_transaction_id;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM ledger_journal_legs
        WHERE transaction_id = target_transaction_id
        GROUP BY asset_id
        HAVING sum(amount) <> 0
    ) THEN
        RAISE EXCEPTION 'journal transaction % is not balanced per asset', target_transaction_id;
    END IF;

    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_journal_transaction_balanced
AFTER INSERT ON ledger_journal_transactions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ledger_assert_transaction_balanced();

ALTER TABLE ledger_journal_transactions
    ADD COLUMN commit_sequence BIGINT GENERATED ALWAYS AS IDENTITY;

ALTER TABLE ledger_journal_transactions
    ADD CONSTRAINT ledger_journal_transactions_commit_sequence_unique
    UNIQUE (commit_sequence);

CREATE INDEX ledger_journal_transactions_recovery_sequence
    ON ledger_journal_transactions (commit_sequence DESC);

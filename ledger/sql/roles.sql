-- Run as the database owner after migrations. These are NOLOGIN group roles;
-- create separate secret-bearing login roles and grant each one exactly one
-- group role outside this file.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'vela_ledger_migrator') THEN
        CREATE ROLE vela_ledger_migrator NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'vela_ledger_writer') THEN
        CREATE ROLE vela_ledger_writer NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'vela_ledger_reader') THEN
        CREATE ROLE vela_ledger_reader NOLOGIN;
    END IF;
END
$$;

ALTER SCHEMA public OWNER TO vela_ledger_migrator;

DO $$
DECLARE
    object RECORD;
BEGIN
    FOR object IN
        SELECT schemaname, tablename
        FROM pg_tables
        WHERE schemaname = 'public'
    LOOP
        EXECUTE format(
            'ALTER TABLE %I.%I OWNER TO vela_ledger_migrator',
            object.schemaname,
            object.tablename
        );
    END LOOP;

    FOR object IN
        SELECT sequence_schema, sequence_name
        FROM information_schema.sequences
        WHERE sequence_schema = 'public'
    LOOP
        EXECUTE format(
            'ALTER SEQUENCE %I.%I OWNER TO vela_ledger_migrator',
            object.sequence_schema,
            object.sequence_name
        );
    END LOOP;

    FOR object IN
        SELECT n.nspname AS schema_name, p.proname, pg_get_function_identity_arguments(p.oid) AS arguments
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public'
    LOOP
        EXECUTE format(
            'ALTER FUNCTION %I.%I(%s) OWNER TO vela_ledger_migrator',
            object.schema_name,
            object.proname,
            object.arguments
        );
    END LOOP;
END
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC;

GRANT USAGE ON SCHEMA public TO vela_ledger_writer, vela_ledger_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO vela_ledger_reader;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA public TO vela_ledger_writer;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO vela_ledger_writer;
REVOKE INSERT, UPDATE, DELETE ON _sqlx_migrations FROM vela_ledger_writer;
REVOKE INSERT, UPDATE, DELETE ON ledger_chain_deployments FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_asset_halts FROM vela_ledger_writer;
REVOKE INSERT, UPDATE, DELETE ON ledger_withdrawal_policy_limits FROM vela_ledger_writer;
REVOKE INSERT, UPDATE, DELETE ON ledger_withdrawal_contract_capabilities FROM vela_ledger_writer;
REVOKE INSERT, UPDATE, DELETE ON ledger_withdrawal_confirmation_policies FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_policy_decisions FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_global_withdrawal_halts FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_broadcast_attempts FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_chain_observations FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_executions FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_failure_releases FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_replacement_evidence FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_withdrawal_attempt_reverts FROM vela_ledger_writer;
REVOKE DELETE ON ledger_withdrawal_worker_checkpoints FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_custody_snapshots FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_custody_snapshot_balances FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_reconciliation_results FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_reconciliation_halt_links FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_reconciliation_failures FROM vela_ledger_writer;
REVOKE UPDATE, DELETE ON ledger_reconciliation_failure_halts FROM vela_ledger_writer;
REVOKE DELETE ON ledger_reconciliation_worker_checkpoints FROM vela_ledger_writer;

GRANT UPDATE (balance, version, updated_at) ON ledger_account_balances TO vela_ledger_writer;
GRANT UPDATE (published_at) ON ledger_outbox TO vela_ledger_writer;
GRANT UPDATE (
    state,
    credit_transaction_id,
    reversal_transaction_id,
    updated_at
) ON ledger_deposits TO vela_ledger_writer;
GRANT UPDATE (
    next_block,
    last_finalized_block_number,
    last_finalized_block_hash,
    lease_owner,
    lease_expires_at,
    updated_at
) ON ledger_indexer_checkpoints TO vela_ledger_writer;
GRANT UPDATE (
    next_block,
    cycle,
    last_scanned_block_number,
    last_scanned_block_hash,
    updated_at
) ON ledger_indexer_audit_checkpoints TO vela_ledger_writer;
GRANT UPDATE (
    lease_owner,
    lease_expires_at,
    scan_cycle,
    last_withdrawal_id,
    last_withdrawal_updated_at,
    last_completed_at,
    updated_at
) ON ledger_withdrawal_worker_checkpoints TO vela_ledger_writer;
GRANT UPDATE (
    lease_owner,
    lease_expires_at,
    cycle,
    last_finalized_block_number,
    last_finalized_block_hash,
    last_snapshot_id,
    last_completed_at,
    updated_at
) ON ledger_reconciliation_worker_checkpoints TO vela_ledger_writer;
GRANT UPDATE (
    state,
    reservation_transaction_id,
    completion_transaction_id,
    release_transaction_id,
    nonce,
    expires_at,
    transaction_hash,
    updated_at
) ON ledger_withdrawals TO vela_ledger_writer;

GRANT UPDATE (
    resolution_evidence,
    resolution_initiated_by,
    resolved_by,
    resolved_at
) ON ledger_global_withdrawal_halts TO vela_ledger_writer;

-- New migrations must extend these grants deliberately. There are no broad
-- writer default privileges because a newly added financial table must fail
-- closed until its access pattern is reviewed.

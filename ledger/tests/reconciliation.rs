use bigdecimal::BigDecimal;
use chrono::Utc;
use ledger::{
    JournalLegInput, Ledger, ObservedCustodyBalance, PostTransaction, ReconciliationStatus,
    RecordReconciliationCycle, RecordReconciliationFailure,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

struct Fixture {
    ledger: Ledger,
    deployment_id: Uuid,
    asset_id: Uuid,
    owner: Uuid,
}

async fn fixture(pool: PgPool, customer: i64, custody: i64) -> Fixture {
    let ledger = Ledger::from_pool(pool.clone());
    let deployment_id = Uuid::new_v4();
    let asset_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO ledger_chain_deployments (
            id, chain_id, network, vault_address, deployment_version,
            runtime_code_hash, deposit_event_signature, deposit_event_topic,
            finality_policy, start_block
        ) VALUES ($1, 84532, 'base-sepolia', $2, 'test-v1', $3, 'Deposit()', $4,
                  'rpc_finalized_two_provider', 1)
        "#,
    )
    .bind(deployment_id)
    .bind("0x1111111111111111111111111111111111111111")
    .bind(format!("0x{}", "22".repeat(32)))
    .bind(format!("0x{}", "33".repeat(32)))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_assets (id, code, decimals, chain_id) VALUES ($1, 'ETH', 18, 84532)",
    )
    .bind(asset_id)
    .execute(&pool)
    .await
    .unwrap();
    for (owner_type, owner_reference, account_type, allow_negative, balance) in [
        (
            "customer",
            "customer-1",
            "customer_available",
            false,
            customer,
        ),
        (
            "custody",
            "0x1111111111111111111111111111111111111111",
            "custody_vault",
            true,
            custody,
        ),
    ] {
        let account_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ledger_accounts
                (id, asset_id, owner_type, owner_reference, account_type, allow_negative)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(account_id)
        .bind(asset_id)
        .bind(owner_type)
        .bind(owner_reference)
        .bind(account_type)
        .bind(allow_negative)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO ledger_account_balances (account_id, balance) VALUES ($1, $2)")
            .bind(account_id)
            .bind(BigDecimal::from(balance))
            .execute(&pool)
            .await
            .unwrap();
    }
    let owner = Uuid::new_v4();
    assert!(ledger
        .acquire_reconciliation_lease(deployment_id, owner, Duration::from_secs(60))
        .await
        .unwrap());
    Fixture {
        ledger,
        deployment_id,
        asset_id,
        owner,
    }
}

fn command(fixture: &Fixture, custody: i64, discriminator: u8) -> RecordReconciliationCycle {
    let evidence = json!({"test": discriminator});
    let input_hash: [u8; 32] = Sha256::digest(serde_json::to_vec(&evidence).unwrap()).into();
    RecordReconciliationCycle {
        deployment_id: fixture.deployment_id,
        lease_owner: fixture.owner,
        chain_id: 84532,
        block_number: i64::from(discriminator) + 100,
        block_hash: format!("0x{:02x}{}", discriminator, "00".repeat(31)),
        block_timestamp: Utc::now(),
        vault_address: "0x1111111111111111111111111111111111111111".to_owned(),
        deployment_version: "test-v1".to_owned(),
        primary_provider: "provider-a".to_owned(),
        secondary_provider: "provider-b".to_owned(),
        input_hash,
        evidence,
        balances: vec![ObservedCustodyBalance {
            asset_id: fixture.asset_id,
            amount: BigDecimal::from(custody),
        }],
    }
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn exact_equality_is_balanced_and_restart_replay_is_idempotent(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let command = command(&fixture, 100, 1);
    let first = fixture
        .ledger
        .record_reconciliation_cycle(&command)
        .await
        .unwrap();
    assert_eq!(first.results[0].status, ReconciliationStatus::Balanced);
    assert!(first.results[0].halt_id.is_none());
    let replay = fixture
        .ledger
        .record_reconciliation_cycle(&command)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.results, first.results);
    let cycle: i64 = sqlx::query_scalar(
        "SELECT cycle FROM ledger_reconciliation_worker_checkpoints WHERE deployment_id = $1",
    )
    .bind(fixture.deployment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cycle, 1);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn same_custody_block_is_recomputed_when_ledger_projection_changes(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let command = command(&fixture, 100, 11);
    let first = fixture
        .ledger
        .record_reconciliation_cycle(&command)
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE ledger_account_balances balance
        SET balance = CASE account.owner_type WHEN 'customer' THEN 101 ELSE -101 END,
            version = version + 1
        FROM ledger_accounts account
        WHERE account.id = balance.account_id AND account.asset_id = $1
        "#,
    )
    .bind(fixture.asset_id)
    .execute(&pool)
    .await
    .unwrap();
    let second = fixture
        .ledger
        .record_reconciliation_cycle(&command)
        .await
        .unwrap();
    assert!(!second.replayed);
    assert_ne!(second.snapshot_id, first.snapshot_id);
    assert_eq!(second.results[0].status, ReconciliationStatus::Deficit);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn supplied_custody_amount_is_part_of_the_idempotency_identity(pool: PgPool) {
    let fixture = fixture(pool, 100, -100).await;
    let first_command = command(&fixture, 100, 12);
    let first = fixture
        .ledger
        .record_reconciliation_cycle(&first_command)
        .await
        .unwrap();
    let mut changed = first_command;
    changed.balances[0].amount = BigDecimal::from(99);
    let second = fixture
        .ledger
        .record_reconciliation_cycle(&changed)
        .await
        .unwrap();
    assert!(!second.replayed);
    assert_ne!(second.snapshot_id, first.snapshot_id);
    assert_eq!(second.results[0].status, ReconciliationStatus::Deficit);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn deficit_atomically_persists_full_asset_halt(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let result = fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 99, 2))
        .await
        .unwrap();
    assert_eq!(result.results[0].status, ReconciliationStatus::Deficit);
    let row: (String, bool, bool, bool, i64) = sqlx::query_as(
        r#"
        SELECT halt.reason, halt.deposit_credits_halted, halt.trading_halted,
               halt.withdrawals_halted, count(link.result_id)
        FROM ledger_asset_halts halt
        JOIN ledger_reconciliation_halt_links link ON link.halt_id = halt.id
        WHERE halt.asset_id = $1
        GROUP BY halt.id
        "#,
    )
    .bind(fixture.asset_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        ("reconciliation_deficit".to_owned(), true, true, true, 1)
    );
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn deficit_rejects_new_deposit_trade_and_withdrawal_journals(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let result = fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 99, 12))
        .await
        .unwrap();
    assert_eq!(result.results[0].status, ReconciliationStatus::Deficit);

    let accounts: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, account_type FROM ledger_accounts WHERE asset_id = $1 ORDER BY account_type",
    )
    .bind(fixture.asset_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let customer_id = accounts
        .iter()
        .find(|(_, kind)| kind == "customer_available")
        .unwrap()
        .0;
    let custody_id = accounts
        .iter()
        .find(|(_, kind)| kind == "custody_vault")
        .unwrap()
        .0;

    for operation in [
        "deposit_credit",
        "order_reservation",
        "trade",
        "withdrawal_reservation",
    ] {
        let id = Uuid::new_v4();
        let error = fixture
            .ledger
            .post(&PostTransaction {
                id,
                operation_type: operation.to_owned(),
                idempotency_key: format!("halted-{operation}-{id}"),
                request_hash: [12; 32],
                external_reference: None,
                metadata: json!({"test_fixture": true}),
                legs: vec![
                    JournalLegInput {
                        account_id: custody_id,
                        amount: BigDecimal::from(-1),
                    },
                    JournalLegInput {
                        account_id: customer_id,
                        amount: BigDecimal::from(1),
                    },
                ],
            })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("asset operation is halted"),
            "{operation}: {error}"
        );
    }
    let journal_count: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_journal_transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(journal_count, 0);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn surplus_is_preserved_without_claiming_balance_or_auto_halt(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let result = fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 101, 3))
        .await
        .unwrap();
    assert_eq!(result.results[0].status, ReconciliationStatus::Surplus);
    assert!(result.results[0].halt_id.is_none());
    let halted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM ledger_asset_halts WHERE asset_id = $1 AND resolved_at IS NULL)",
    )
    .bind(fixture.asset_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!halted);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn inconsistent_ledger_projection_is_indeterminate_and_halted(pool: PgPool) {
    let fixture = fixture(pool, 100, -90).await;
    let result = fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 100, 4))
        .await
        .unwrap();
    assert_eq!(
        result.results[0].status,
        ReconciliationStatus::Indeterminate
    );
    assert!(result.results[0].halt_id.is_some());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn registered_asset_without_accounts_is_indeterminate_and_halted(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let empty_asset_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO ledger_assets (id, code, decimals, chain_id, contract_address) VALUES ($1, 'USDC', 6, 84532, $2)",
    )
    .bind(empty_asset_id)
    .bind("0x2222222222222222222222222222222222222222")
    .execute(&pool)
    .await
    .unwrap();
    let mut command = command(&fixture, 100, 5);
    command.balances.push(ObservedCustodyBalance {
        asset_id: empty_asset_id,
        amount: BigDecimal::from(0),
    });

    let result = fixture
        .ledger
        .record_reconciliation_cycle(&command)
        .await
        .unwrap();
    let empty_result = result
        .results
        .iter()
        .find(|row| row.asset_id == empty_asset_id)
        .unwrap();
    assert_eq!(empty_result.status, ReconciliationStatus::Indeterminate);
    assert!(empty_result.halt_id.is_some());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn lease_excludes_concurrent_owner_and_checkpoint_cannot_regress(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    assert!(!fixture
        .ledger
        .acquire_reconciliation_lease(
            fixture.deployment_id,
            Uuid::new_v4(),
            Duration::from_secs(60)
        )
        .await
        .unwrap());
    fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 100, 8))
        .await
        .unwrap();
    let error = fixture
        .ledger
        .record_reconciliation_cycle(&command(&fixture, 100, 7))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cannot regress"));
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn provider_failure_evidence_halts_every_asset_idempotently(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let command = RecordReconciliationFailure {
        failure_id: Uuid::new_v4(),
        deployment_id: fixture.deployment_id,
        reason: "provider_disagreement".to_owned(),
        evidence: json!({"providers":"disagree"}),
        evidence_hash: Sha256::digest(
            serde_json::to_vec(&json!({"providers":"disagree"})).unwrap(),
        )
        .into(),
    };
    fixture
        .ledger
        .record_reconciliation_failure(&command)
        .await
        .unwrap();
    fixture
        .ledger
        .record_reconciliation_failure(&command)
        .await
        .unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM ledger_reconciliation_failures), (SELECT count(*) FROM ledger_asset_halts)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1));
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn database_rejects_deficit_result_without_halt_link(pool: PgPool) {
    let fixture = fixture(pool.clone(), 100, -100).await;
    let first_command = command(&fixture, 100, 9);
    fixture
        .ledger
        .record_reconciliation_cycle(&first_command)
        .await
        .unwrap();
    let second = command(&fixture, 99, 10);
    let second_snapshot_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO ledger_custody_snapshots (
            id, chain_id, block_number, block_hash, vault_address, deployment_version,
            input_hash, captured_at, deployment_id, primary_provider, secondary_provider,
            block_timestamp, evidence
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, transaction_timestamp(), $8, $9, $10, $11, $12)
        "#,
    )
    .bind(second_snapshot_id)
    .bind(second.chain_id)
    .bind(second.block_number)
    .bind(&second.block_hash)
    .bind(&second.vault_address)
    .bind(&second.deployment_version)
    .bind(second.input_hash.as_slice())
    .bind(second.deployment_id)
    .bind(&second.primary_provider)
    .bind(&second.secondary_provider)
    .bind(second.block_timestamp)
    .bind(&second.evidence)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_custody_snapshot_balances (snapshot_id, asset_id, amount) VALUES ($1, $2, 99)",
    )
    .bind(second_snapshot_id)
    .bind(fixture.asset_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO ledger_reconciliation_results
            (id, snapshot_id, asset_id, custody_amount, liability_amount,
             difference, status, evidence, evidence_hash)
        VALUES ($1, $2, $3, 99, 100, -1, 'deficit', '{}'::jsonb, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(second_snapshot_id)
    .bind(fixture.asset_id)
    .bind([99_u8; 32].as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();
    assert!(tx.commit().await.is_err());
}

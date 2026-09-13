use bigdecimal::BigDecimal;
use ledger::{
    AccountProvisionRequest, JournalLegInput, Ledger, LedgerError, NewAccount, NewAsset,
    PostTransaction,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    ledger: Ledger,
    asset_id: Uuid,
    source_id: Uuid,
    destination_a_id: Uuid,
    destination_b_id: Uuid,
    operations_id: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let ledger = Ledger::from_pool(pool);
        let asset_id = Uuid::new_v4();
        ledger
            .create_asset(&NewAsset {
                id: asset_id,
                code: "USDC".to_owned(),
                decimals: 6,
                chain_id: Some(8453),
                contract_address: Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_owned()),
            })
            .await
            .unwrap();

        let source_id = Uuid::new_v4();
        let destination_a_id = Uuid::new_v4();
        let destination_b_id = Uuid::new_v4();
        let operations_id = Uuid::new_v4();

        for account in [
            NewAccount {
                id: source_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: "customer:source".to_owned(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: destination_a_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: "customer:destination-a".to_owned(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: destination_b_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: "customer:destination-b".to_owned(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: operations_id,
                asset_id,
                owner_type: "platform".to_owned(),
                owner_reference: "platform:operations".to_owned(),
                account_type: "operational_adjustment".to_owned(),
                allow_negative: true,
            },
        ] {
            ledger.create_account(&account).await.unwrap();
        }

        Self {
            ledger,
            asset_id,
            source_id,
            destination_a_id,
            destination_b_id,
            operations_id,
        }
    }

    async fn seed_source(&self, amount: i64) {
        self.ledger
            .post(&command(
                "seed-source",
                "administrative_adjustment",
                1,
                vec![(self.operations_id, -amount), (self.source_id, amount)],
            ))
            .await
            .unwrap();
    }
}

fn command(
    idempotency_key: &str,
    operation_type: &str,
    hash_byte: u8,
    legs: Vec<(Uuid, i64)>,
) -> PostTransaction {
    PostTransaction {
        id: Uuid::new_v4(),
        operation_type: operation_type.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        request_hash: [hash_byte; 32],
        external_reference: None,
        metadata: json!({}),
        legs: legs
            .into_iter()
            .map(|(account_id, amount)| JournalLegInput {
                account_id,
                amount: BigDecimal::from(amount),
            })
            .collect(),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn posts_balanced_transaction_and_materializes_balances(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.seed_source(100).await;

    let posted = fixture
        .ledger
        .post(&command(
            "transfer-1",
            "trade",
            2,
            vec![(fixture.source_id, -25), (fixture.destination_a_id, 25)],
        ))
        .await
        .unwrap();

    assert_eq!(
        fixture.ledger.balance(fixture.source_id).await.unwrap(),
        BigDecimal::from(75)
    );
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.destination_a_id)
            .await
            .unwrap(),
        BigDecimal::from(25)
    );
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());

    let outbox_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_outbox WHERE transaction_id = $1")
            .bind(posted.id)
            .fetch_one(fixture.ledger.pool())
            .await
            .unwrap();
    assert_eq!(outbox_count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn returns_original_result_for_idempotent_retry(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.seed_source(100).await;
    let first_command = command(
        "transfer-retry",
        "trade",
        3,
        vec![(fixture.source_id, -10), (fixture.destination_a_id, 10)],
    );

    let first = fixture.ledger.post(&first_command).await.unwrap();
    let retry = fixture.ledger.post(&first_command).await.unwrap();

    assert_eq!(first, retry);
    assert_eq!(
        fixture.ledger.balance(fixture.source_id).await.unwrap(),
        BigDecimal::from(90)
    );

    let journal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE idempotency_key = $1",
    )
    .bind("transfer-retry")
    .fetch_one(fixture.ledger.pool())
    .await
    .unwrap();
    assert_eq!(journal_count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn rejects_idempotency_key_reuse_with_different_request(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.seed_source(100).await;
    let first = command(
        "conflicting-request",
        "trade",
        4,
        vec![(fixture.source_id, -10), (fixture.destination_a_id, 10)],
    );
    fixture.ledger.post(&first).await.unwrap();

    let conflicting = command(
        "conflicting-request",
        "trade",
        5,
        vec![(fixture.source_id, -20), (fixture.destination_a_id, 20)],
    );
    let error = fixture.ledger.post(&conflicting).await.unwrap_err();

    assert!(matches!(error, LedgerError::IdempotencyConflict));
}

#[sqlx::test(migrations = "./migrations")]
async fn database_rejects_unbalanced_transaction(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let transaction_id = Uuid::new_v4();
    let mut tx = fixture.ledger.pool().begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO ledger_journal_transactions
            (id, operation_type, idempotency_key, request_hash)
        VALUES ($1, 'trade', 'unbalanced-direct-write', $2)
        "#,
    )
    .bind(transaction_id)
    .bind([6_u8; 32].as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();

    for (account_id, amount) in [(fixture.source_id, -10_i64), (fixture.destination_a_id, 9)] {
        sqlx::query(
            r#"
            INSERT INTO ledger_journal_legs
                (transaction_id, account_id, asset_id, amount)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(transaction_id)
        .bind(account_id)
        .bind(fixture.asset_id)
        .bind(BigDecimal::from(amount))
        .execute(&mut *tx)
        .await
        .unwrap();
    }

    assert!(tx.commit().await.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn database_rejects_journal_mutation(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.seed_source(100).await;

    let update_result = sqlx::query(
        "UPDATE ledger_journal_transactions SET metadata = '{\"changed\":true}' WHERE idempotency_key = 'seed-source'",
    )
    .execute(fixture.ledger.pool())
    .await;
    assert!(update_result.is_err());

    let delete_result =
        sqlx::query("DELETE FROM ledger_journal_legs WHERE transaction_id = (SELECT id FROM ledger_journal_transactions WHERE idempotency_key = 'seed-source')")
            .execute(fixture.ledger.pool())
            .await;
    assert!(delete_result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_debits_cannot_overspend(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.seed_source(100).await;

    let ledger_a = fixture.ledger.clone();
    let ledger_b = fixture.ledger.clone();
    let debit_a = command(
        "concurrent-a",
        "trade",
        7,
        vec![(fixture.source_id, -80), (fixture.destination_a_id, 80)],
    );
    let debit_b = command(
        "concurrent-b",
        "trade",
        8,
        vec![(fixture.source_id, -80), (fixture.destination_b_id, 80)],
    );

    let (result_a, result_b) = tokio::join!(ledger_a.post(&debit_a), ledger_b.post(&debit_b));
    assert_eq!(
        usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
        1
    );
    assert!(matches!(
        result_a.as_ref().err().or(result_b.as_ref().err()),
        Some(LedgerError::InsufficientFunds { .. }) | Some(LedgerError::Database(_))
    ));
    assert_eq!(
        fixture.ledger.balance(fixture.source_id).await.unwrap(),
        BigDecimal::from(20)
    );
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
async fn rejects_fractional_base_units_before_database_write(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let fractional = PostTransaction {
        id: Uuid::new_v4(),
        operation_type: "trade".to_owned(),
        idempotency_key: "fractional".to_owned(),
        request_hash: [9; 32],
        external_reference: None,
        metadata: json!({}),
        legs: vec![
            JournalLegInput {
                account_id: fixture.source_id,
                amount: "-1.5".parse().unwrap(),
            },
            JournalLegInput {
                account_id: fixture.destination_a_id,
                amount: "1.5".parse().unwrap(),
            },
        ],
    };

    assert!(matches!(
        fixture.ledger.post(&fractional).await,
        Err(LedgerError::InvalidAmount)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn database_rejects_fractional_base_units(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let transaction_id = Uuid::new_v4();
    let mut tx = fixture.ledger.pool().begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO ledger_journal_transactions
            (id, operation_type, idempotency_key, request_hash)
        VALUES ($1, 'trade', 'fractional-direct-write', $2)
        "#,
    )
    .bind(transaction_id)
    .bind([10_u8; 32].as_slice())
    .execute(&mut *tx)
    .await
    .unwrap();

    let result = sqlx::query(
        r#"
        INSERT INTO ledger_journal_legs
            (transaction_id, account_id, asset_id, amount)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(transaction_id)
    .bind(fixture.source_id)
    .bind(fixture.asset_id)
    .bind("1.5".parse::<BigDecimal>().unwrap())
    .execute(&mut *tx)
    .await;

    assert!(result.is_err());
}

#[sqlx::test]
async fn readiness_rejects_database_without_current_schema(pool: PgPool) {
    let ledger = Ledger::from_pool(pool);
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(ledger.pool())
        .await
        .unwrap();
    assert!(matches!(
        ledger.health_check().await,
        Err(LedgerError::SchemaVersion {
            expected: 1,
            found: 0
        })
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn account_provisioning_is_idempotent_and_policy_checked(pool: PgPool) {
    let ledger = Ledger::from_pool(pool);
    ledger
        .ensure_asset(&NewAsset {
            id: Uuid::new_v4(),
            code: "ETH".to_owned(),
            decimals: 18,
            chain_id: Some(8453),
            contract_address: None,
        })
        .await
        .unwrap();
    ledger
        .ensure_asset(&NewAsset {
            id: Uuid::new_v4(),
            code: "ETH".to_owned(),
            decimals: 18,
            chain_id: Some(8453),
            contract_address: None,
        })
        .await
        .unwrap();

    let request = AccountProvisionRequest {
        asset_code: "ETH".to_owned(),
        owner_type: "customer".to_owned(),
        owner_reference: "0x0000000000000000000000000000000000000001".to_owned(),
        account_type: "customer_available".to_owned(),
        allow_negative: false,
    };
    let first = ledger.ensure_accounts(&[request.clone()]).await.unwrap();
    let second = ledger.ensure_accounts(&[request.clone()]).await.unwrap();
    assert_eq!(
        first.values().next().unwrap(),
        second.values().next().unwrap()
    );

    let mut invalid = request;
    invalid.allow_negative = true;
    assert!(matches!(
        ledger.ensure_accounts(&[invalid]).await,
        Err(LedgerError::InvalidAccountProvision(_))
    ));

    let conflicting_asset = ledger
        .ensure_asset(&NewAsset {
            id: Uuid::new_v4(),
            code: "ETH".to_owned(),
            decimals: 8,
            chain_id: Some(8453),
            contract_address: None,
        })
        .await;
    assert!(matches!(
        conflicting_asset,
        Err(LedgerError::AssetConfigurationConflict(code)) if code == "ETH"
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn journal_replay_matches_projection_and_detects_divergence(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    fixture.seed_source(100).await;

    let replayed = fixture.ledger.replay_account_balances().await.unwrap();
    let source = replayed
        .iter()
        .find(|account| account.account_id == fixture.source_id)
        .unwrap();
    assert_eq!(source.balance, BigDecimal::from(100));

    sqlx::query("UPDATE ledger_account_balances SET balance = balance + 1 WHERE account_id = $1")
        .bind(fixture.source_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        fixture.ledger.replay_account_balances().await,
        Err(LedgerError::ReplayProjectionMismatch { account_id, .. })
            if account_id == fixture.source_id
    ));
}

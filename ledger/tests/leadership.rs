use bigdecimal::BigDecimal;
use ledger::{EngineLeaderToken, JournalLegInput, Ledger, NewAccount, NewAsset, PostTransaction};
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

#[sqlx::test(migrations = "../ledger/migrations")]
async fn exactly_one_of_two_concurrent_contenders_acquires(pool: PgPool) {
    let ledger = Ledger::from_pool(pool);
    let left = ledger.acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30));
    let right = ledger.acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30));
    let (left, right) = tokio::join!(left, right);
    assert_ne!(left.unwrap().is_some(), right.unwrap().is_some());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn takeover_increments_generation_and_supersedes_stale_owner(pool: PgPool) {
    let ledger = Ledger::from_pool(pool.clone());
    let old = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    expire(&pool).await;
    let new = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert!(new.token.generation > old.token.generation);
    assert!(!ledger.validate_engine_leadership(old.token).await.unwrap());
    assert!(ledger.validate_engine_leadership(new.token).await.unwrap());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn lost_owner_cannot_renew_or_release_new_generation(pool: PgPool) {
    let ledger = Ledger::from_pool(pool.clone());
    let old = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    expire(&pool).await;
    let new = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert!(ledger
        .renew_engine_leadership(old.token, Duration::from_secs(30))
        .await
        .unwrap()
        .is_none());
    assert!(!ledger.release_engine_leadership(old.token).await.unwrap());
    assert!(ledger.validate_engine_leadership(new.token).await.unwrap());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn stale_generation_cannot_commit_financial_journal(pool: PgPool) {
    let ledger = Ledger::from_pool(pool.clone());
    let (source, destination) = financial_fixture(&ledger).await;
    let old = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    expire(&pool).await;
    let current = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    let command = transfer(source, destination, "stale-engine-commit");
    let error = ledger
        .post_as_engine_leader(&command, old.token)
        .await
        .unwrap_err();
    assert!(matches!(error, ledger::LedgerError::EngineLeadershipLost));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE idempotency_key = $1",
    )
    .bind(&command.idempotency_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    ledger
        .post_as_engine_leader(
            &transfer(source, destination, "current-engine-commit"),
            current.token,
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn release_requires_exact_owner_and_generation(pool: PgPool) {
    let ledger = Ledger::from_pool(pool);
    let active = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    let wrong = EngineLeaderToken {
        owner_id: active.token.owner_id,
        generation: active.token.generation + 1,
    };
    assert!(!ledger.release_engine_leadership(wrong).await.unwrap());
    assert!(ledger
        .validate_engine_leadership(active.token)
        .await
        .unwrap());
    assert!(ledger
        .release_engine_leadership(active.token)
        .await
        .unwrap());
    assert!(!ledger
        .validate_engine_leadership(active.token)
        .await
        .unwrap());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn database_rejects_generation_regression(pool: PgPool) {
    let ledger = Ledger::from_pool(pool.clone());
    let active = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    let error = sqlx::query(
        "UPDATE ledger_engine_leadership SET generation = generation - 1 WHERE singleton",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("engine leadership generation cannot regress"));
    assert!(ledger
        .validate_engine_leadership(active.token)
        .await
        .unwrap());
}

async fn expire(pool: &PgPool) {
    sqlx::query(
        "UPDATE ledger_engine_leadership SET lease_expires_at = transaction_timestamp() - interval '1 second'",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn financial_fixture(ledger: &Ledger) -> (Uuid, Uuid) {
    let asset = Uuid::new_v4();
    ledger
        .create_asset(&NewAsset {
            id: asset,
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(8453),
            contract_address: Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_owned()),
        })
        .await
        .unwrap();
    let source = Uuid::new_v4();
    let destination = Uuid::new_v4();
    for account in [
        NewAccount {
            id: source,
            asset_id: asset,
            owner_type: "platform".to_owned(),
            owner_reference: "platform:source".to_owned(),
            account_type: "operational_adjustment".to_owned(),
            allow_negative: true,
        },
        NewAccount {
            id: destination,
            asset_id: asset,
            owner_type: "platform".to_owned(),
            owner_reference: "platform:destination".to_owned(),
            account_type: "operational_adjustment".to_owned(),
            allow_negative: true,
        },
    ] {
        ledger.create_account(&account).await.unwrap();
    }
    (source, destination)
}

fn transfer(source: Uuid, destination: Uuid, key: &str) -> PostTransaction {
    PostTransaction {
        id: Uuid::new_v4(),
        operation_type: "trade".to_owned(),
        idempotency_key: key.to_owned(),
        request_hash: [7; 32],
        external_reference: None,
        metadata: json!({"source":"test"}),
        legs: vec![
            JournalLegInput {
                account_id: source,
                amount: BigDecimal::from(-1),
            },
            JournalLegInput {
                account_id: destination,
                amount: BigDecimal::from(1),
            },
        ],
    }
}

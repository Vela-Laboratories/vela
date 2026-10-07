use api::algo_request::TwapRequest;
use api::algos::AlgoStatus;
use api::algos_store::AlgoStore;
use ledger::Ledger;
use std::time::Duration;
use types::{OrderSide, UserId};
use uuid::Uuid;

fn request(nonce: u64) -> TwapRequest {
    TwapRequest {
        address: format!("0x{}", "11".repeat(20)),
        market: "ETH-USDC".into(),
        side: OrderSide::Bid,
        quantity: 1200,
        duration_secs: 60,
        price_limit: Some(100),
        slices: Some(3),
        nonce,
        signature: "test-evidence".into(),
        signature_version: 2,
    }
}
async fn fixture(pool: sqlx::PgPool) -> (Ledger, AlgoStore, ledger::EngineLeaderToken) {
    let ledger = Ledger::from_pool(pool.clone());
    let token = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap()
        .token;
    (
        ledger,
        AlgoStore::new(pool, "testnet-demo:84532".into(), token),
        token,
    )
}
#[sqlx::test(migrations = "../ledger/migrations")]
async fn signed_parent_replay_and_overlapping_nonce_ranges_are_rejected(pool: sqlx::PgPool) {
    let (_, store, _) = fixture(pool).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = store.admit(&req, &signer, 10, 1000).await.unwrap();
    assert_eq!(parent.request.signing_message(), req.signing_message());
    assert!(store.admit(&req, &signer, 10, 1000).await.is_err());
    assert!(store.admit(&request(103), &signer, 10, 1000).await.is_err());
    assert!(store.admit(&request(104), &signer, 10, 1000).await.is_ok());
}
#[sqlx::test(migrations = "../ledger/migrations")]
async fn child_intent_is_single_dispatch_and_completion_is_idempotent(pool: sqlx::PgPool) {
    let (_, store, _) = fixture(pool).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = store.admit(&req, &signer, 10, 1000).await.unwrap();
    assert!(store.start_child(parent.id, 0, 399).await.is_err());
    assert!(store.start_child(parent.id, 0, 400).await.unwrap());
    assert!(store.start_child(parent.id, 0, 400).await.is_err());
    let first = store
        .complete_child(parent.id, 0, 100, &[1; 32], false)
        .await
        .unwrap();
    assert_eq!(first.next_slice, 1);
    assert_eq!(first.filled_quantity, 100);
    let replay = store
        .complete_child(parent.id, 0, 100, &[1; 32], false)
        .await
        .unwrap();
    assert_eq!(replay.filled_quantity, 100);
    assert!(store
        .complete_child(parent.id, 0, 101, &[1; 32], false)
        .await
        .is_err());
}
#[sqlx::test(migrations = "../ledger/migrations")]
async fn restarted_store_recovers_progress_and_pauses_uncertain_intents(pool: sqlx::PgPool) {
    let (_, store, token) = fixture(pool.clone()).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = store.admit(&req, &signer, 10, 1000).await.unwrap();
    store.start_child(parent.id, 0, 400).await.unwrap();
    store
        .complete_child(parent.id, 0, 100, &[1; 32], false)
        .await
        .unwrap();
    let restarted = AlgoStore::new(pool, "testnet-demo:84532".into(), token);
    let restored = restarted.recover().await.unwrap();
    assert_eq!(restored[0].next_slice, 1);
    assert_eq!(restored[0].status, AlgoStatus::Running);
    restarted.start_child(parent.id, 1, 400).await.unwrap();
    let restored = restarted.recover().await.unwrap();
    assert_eq!(restored[0].status, AlgoStatus::RecoveryRequired);
    assert!(!restarted.start_child(parent.id, 1, 400).await.unwrap());
}
#[sqlx::test(migrations = "../ledger/migrations")]
async fn superseded_leader_cannot_dispatch_or_cancel(pool: sqlx::PgPool) {
    let (ledger, old, token) = fixture(pool).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = old.admit(&req, &signer, 10, 1000).await.unwrap();
    ledger.release_engine_leadership(token).await.unwrap();
    ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert!(old.start_child(parent.id, 0, 400).await.is_err());
    assert!(old
        .cancel(parent.id, &req.address, serde_json::json!({}))
        .await
        .is_err());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn cancellation_and_terminal_evidence_survive_restart(pool: sqlx::PgPool) {
    let (_, store, token) = fixture(pool.clone()).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = store.admit(&req, &signer, 10, 1000).await.unwrap();
    assert!(store
        .cancel(
            parent.id,
            &req.address,
            serde_json::json!({"signature":"test-evidence"})
        )
        .await
        .unwrap());
    let other = AlgoStore::new(pool.clone(), "testnet-demo:84532".into(), token);
    assert!(
        other
            .get(parent.id)
            .await
            .unwrap()
            .unwrap()
            .cancel_requested
    );
    assert!(!other.start_child(parent.id, 0, 400).await.unwrap());
    assert!(
        sqlx::query("UPDATE api_algo_events SET action='changed' WHERE parent_id=$1")
            .bind(parent.id as i64)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE api_algo_parents SET request='{}'::jsonb WHERE id=$1")
            .bind(parent.id as i64)
            .execute(&pool)
            .await
            .is_err()
    );
}
#[sqlx::test(migrations = "../ledger/migrations")]
async fn a_known_rejection_is_terminal_and_not_recovered_as_running(pool: sqlx::PgPool) {
    let (_, store, _) = fixture(pool).await;
    let req = request(100);
    let signer = UserId::from_hex(&req.address).unwrap();
    let parent = store.admit(&req, &signer, 10, 1000).await.unwrap();
    store.start_child(parent.id, 0, 400).await.unwrap();
    let failed = store
        .complete_child(parent.id, 0, 0, &[2; 32], true)
        .await
        .unwrap();
    assert_eq!(failed.status, AlgoStatus::Failed);
    assert_eq!(store.recover().await.unwrap()[0].status, AlgoStatus::Failed);
    assert!(!store.start_child(parent.id, 1, 400).await.unwrap());
}

use api::agents::{AgentDelegation, CapabilityScope, OrderScopeCheck};
use api::agents_store::AgentStore;
use sqlx::PgPool;
use types::{MarketId, OrderSide, OrderType, UserId};

fn delegation(nonce: u64) -> AgentDelegation {
    AgentDelegation {
        master: UserId([1; 20]),
        agent: UserId([2; 20]),
        expires_at_ms: 10_000,
        max_notional_per_order: 100,
        revoked: false,
        nonce,
        scope: CapabilityScope {
            max_notional_per_hour: Some(100),
            ..CapabilityScope::default()
        },
    }
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn concurrent_registration_serializes_nonce_checks(pool: PgPool) {
    let left = AgentStore::new(pool.clone(), "testnet-demo:11155111".into());
    let right = AgentStore::new(pool, "testnet-demo:11155111".into());
    let d = delegation(1);
    let (a, b) = tokio::join!(
        left.register_signed(&d, "message", "signature"),
        right.register_signed(&d, "message", "signature")
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let mut takeover = delegation(2);
    takeover.master = UserId([3; 20]);
    assert!(left
        .register_signed(&takeover, "message", "signature")
        .await
        .is_err());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn shared_notional_caps_cannot_be_exceeded_concurrently(pool: PgPool) {
    let left = AgentStore::new(pool.clone(), "testnet-demo:11155111".into());
    let right = AgentStore::new(pool, "testnet-demo:11155111".into());
    let d = delegation(1);
    left.register_signed(&d, "message", "signature")
        .await
        .unwrap();
    let market = MarketId::new("BTC", "USDC");
    let order = OrderScopeCheck {
        market: &market,
        side: OrderSide::Bid,
        order_type: OrderType::GoodTillCanceled,
    };
    let (a, b) = tokio::join!(
        left.authorize(&d.master, &d.agent, 60, 1, Some(&order)),
        right.authorize(&d.master, &d.agent, 60, 1, Some(&order))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    left.authorize(&d.master, &d.agent, 40, 1, Some(&order))
        .await
        .unwrap();
    assert!(right
        .authorize(&d.master, &d.agent, 1, 1, Some(&order))
        .await
        .is_err());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn restart_recovers_registration_and_revocation_tombstone(pool: PgPool) {
    let original = AgentStore::new(pool.clone(), "testnet-demo:11155111".into());
    let d = delegation(10);
    original
        .register_signed(&d, "registered message", "registered signature")
        .await
        .unwrap();
    drop(original);
    let restarted = AgentStore::new(pool.clone(), "testnet-demo:11155111".into());
    let recovered = restarted.agents_for(&d.master).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].nonce, 10);
    restarted
        .authorize(&d.master, &d.agent, 10, 1, None)
        .await
        .unwrap();
    assert!(restarted
        .revoke_signed(
            &d.master,
            &d.agent,
            11,
            "revoked message",
            "revoked signature"
        )
        .await
        .unwrap());
    drop(restarted);
    let restarted = AgentStore::new(pool.clone(), "testnet-demo:11155111".into());
    let recovered = restarted.agents_for(&d.master).await.unwrap();
    assert!(recovered[0].revoked);
    assert_eq!(recovered[0].nonce, 11);
    assert!(restarted
        .register_signed(&d, "old message", "old signature")
        .await
        .is_err());
    assert!(restarted
        .authorize(&d.master, &d.agent, 1, 1, None)
        .await
        .is_err());
    let evidence: Vec<(String, String)> =
        sqlx::query_as("SELECT action, message FROM api_agent_events ORDER BY nonce")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        evidence,
        vec![
            ("register".into(), "registered message".into()),
            ("revoke".into(), "revoked message".into())
        ]
    );
}

use api::AppState;
use axum_test::TestServer;
use engine::MatchingEngine;
use serde_json::{json, Value};
use std::sync::Arc;
use types::{
    AssetId, DepositRequest, FeeConfig, Market, MarketId, OrderSide, OrderType, PostOrderRequest,
    Request, PRICE_SCALE, QUANTITY_SCALE,
};

fn make_wal() -> Arc<api::wal::Wal> {
    let dir = std::env::temp_dir().join(format!(
        "vela_test_wal_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    Arc::new(api::wal::Wal::new(&dir).unwrap())
}

fn user_addr() -> String {
    "0x0000000000000000000000000000000000000001".to_string()
}

fn engine_with_market() -> MatchingEngine {
    let mut e = MatchingEngine::new(FeeConfig::default(), 5.0);
    e.add_market(Market {
        id: MarketId::new("BTC", "USDC"),
        base: AssetId::from_str("BTC"),
        quote: AssetId::from_str("USDC"),
        max_orders: 10_000,
        min_order_size: 1,
        price_tick: 1,
        quantity_tick: 1,
        maker_fee_bps: -1,
        taker_fee_bps: 5,
    });
    e
}

fn funded_engine() -> MatchingEngine {
    let mut e = engine_with_market();
    let user = types::UserId::from_hex(&user_addr()).unwrap();
    e.process(
        Request::Deposit(DepositRequest {
            user: user.clone(),
            asset: AssetId::from_str("USDC"),
            amount: 100_000 * PRICE_SCALE,
            l1_tx_hash: [0u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        }),
        1,
    );
    e.process(
        Request::Deposit(DepositRequest {
            user: user.clone(),
            asset: AssetId::from_str("BTC"),
            amount: 2 * QUANTITY_SCALE,
            l1_tx_hash: [1u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        }),
        2,
    );
    e
}

fn test_server(engine: MatchingEngine) -> TestServer {
    test_server_with_mode(engine, types::RuntimeMode::Development)
}

fn test_server_with_mode(engine: MatchingEngine, runtime_mode: types::RuntimeMode) -> TestServer {
    // AppState::new() panics without ADMIN_TOKEN.
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state = AppState::new_with_runtime(engine, make_wal(), runtime_mode);
    let router = api::build_router(state);
    TestServer::new(router).unwrap()
}

#[tokio::test]
async fn test_health() {
    let server = test_server(engine_with_market());
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    resp.assert_text("ok");
}

#[tokio::test]
async fn test_development_readiness_allows_unconfigured_ledger() {
    let server = test_server(engine_with_market());
    let response = server.get("/ready").await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["ledger"], "not_configured");
}

#[tokio::test]
async fn test_production_readiness_fails_without_ledger() {
    let server = test_server_with_mode(engine_with_market(), types::RuntimeMode::Production);
    let response = server.get("/ready").await;
    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json();
    assert_eq!(body["ok"], false);
    assert_eq!(body["ledger"], "not_configured");
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn test_production_readiness_passes_with_current_ledger_schema(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool));
    let state = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Production,
        Some(ledger),
    )
    .await
    .unwrap();
    assert!(state.shards.user_state.read().await.balances.is_empty());
    let server = TestServer::new(api::build_router(state)).unwrap();

    let response = server.get("/ready").await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["ledger"], "ready");
    assert_eq!(body["ledger_schema_version"], 2);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn test_ledger_backed_api_ignores_cache_and_rejects_demo_credit(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool));
    let state = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::TestnetDemo,
        Some(ledger),
    )
    .await
    .unwrap();
    let user = types::UserId::from_hex(&user_addr()).unwrap();
    state
        .shards
        .user_state
        .write()
        .await
        .phase1_deposit(&DepositRequest {
            user,
            asset: AssetId::from_str("USDC"),
            amount: 50_000 * PRICE_SCALE,
            l1_tx_hash: [8_u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        });
    let server = TestServer::new(api::build_router(state)).unwrap();

    let balances = server
        .get(&format!("/account/{}/balances", user_addr()))
        .await;
    balances.assert_status_ok();
    let body: Value = balances.json();
    assert!(body["data"].as_array().unwrap().is_empty());

    let deposit = server
        .post("/deposit")
        .json(&json!({
            "user": user_addr(),
            "asset": "USDC",
            "amount": "100.00"
        }))
        .await;
    deposit.assert_status(axum::http::StatusCode::NOT_IMPLEMENTED);
    let body: Value = deposit.json();
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("PostgreSQL ledger is authoritative"));
}

#[tokio::test]
async fn test_list_markets() {
    let server = test_server(engine_with_market());
    let resp = server.get("/markets").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    let markets = body["data"].as_array().unwrap();
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0]["id"], "BTC-USDC");
}

#[tokio::test]
async fn test_get_book_empty() {
    let server = test_server(engine_with_market());
    let resp = server.get("/markets/BTC-USDC/book").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["market"], "BTC-USDC");
    assert!(body["data"]["bids"].as_array().unwrap().is_empty());
    assert!(body["data"]["asks"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_get_book_not_found() {
    let server = test_server(engine_with_market());
    let resp = server.get("/markets/ETH-USDC/book").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_balances_empty() {
    let server = test_server(engine_with_market());
    let resp = server
        .get(&format!("/account/{}/balances", user_addr()))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert!(body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_get_balances_after_deposit() {
    let mut e = engine_with_market();
    let user = types::UserId::from_hex(&user_addr()).unwrap();
    e.process(
        Request::Deposit(DepositRequest {
            user: user.clone(),
            asset: AssetId::from_str("USDC"),
            amount: 50_000 * PRICE_SCALE,
            l1_tx_hash: [0u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        }),
        1,
    );

    let server = test_server(e);
    let resp = server
        .get(&format!("/account/{}/balances", user_addr()))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let balances = body["data"].as_array().unwrap();
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0]["asset"], "USDC");
    assert_eq!(balances[0]["available"], "50000");
}

#[tokio::test]
async fn test_public_reserves_is_sanitized_and_requires_no_admin_token() {
    let server = test_server(funded_engine());
    let response = server.get("/reserves/public").await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["scope"], "aggregate_engine_liabilities_only");
    assert_eq!(body["data"]["custody_assets_included"], false);
    assert_eq!(body["data"]["reconciliation_status"], "not_verified");
    assert_eq!(body["data"]["runtime_mode"], "development");
    assert!(body["data"]["liabilities_by_asset"]["USDC"].is_number());
    assert!(body["data"].get("total_users").is_none());
    assert!(body["data"].get("user_balances").is_none());
}

#[tokio::test]
async fn test_direct_credit_is_disabled_outside_testnet_demo() {
    let server = test_server_with_mode(engine_with_market(), types::RuntimeMode::Production);
    let response = server
        .post("/deposit")
        .json(&json!({
            "user": user_addr(),
            "asset": "USDC",
            "amount": "100.00"
        }))
        .await;

    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json();
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("only in testnet-demo"));
}

#[tokio::test]
async fn test_direct_credit_is_explicitly_labeled_in_testnet_demo() {
    let server = test_server_with_mode(engine_with_market(), types::RuntimeMode::TestnetDemo);
    let response = server
        .post("/deposit")
        .json(&json!({
            "user": user_addr(),
            "asset": "USDC",
            "amount": "100.00"
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["credit_type"], "demo_unverified");
    assert_eq!(body["data"]["verified_onchain"], false);
    assert!(body["data"]["balances"].is_array());
}

#[tokio::test]
async fn test_withdrawal_signature_is_disabled_in_production() {
    let server = test_server_with_mode(engine_with_market(), types::RuntimeMode::Production);
    let response = server
        .post("/withdrawal-signature")
        .json(&json!({
            "user": user_addr(),
            "asset": "ETH",
            "amount": "0.01",
            "nonce": 1
        }))
        .await;

    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json();
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("durably reserved"));
}

#[tokio::test]
async fn test_invalid_address_returns_400() {
    let server = test_server(engine_with_market());
    let resp = server.get("/account/notanaddress/balances").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_post_order_invalid_sig_returns_401() {
    let server = test_server(engine_with_market());
    let body = json!({
        "market": "BTC-USDC",
        "side": "Bid",
        "order_type": "GoodTillCanceled",
        "price": 50_000u64 * PRICE_SCALE,
        "quantity": QUANTITY_SCALE,
        "nonce": 1,
        "address": user_addr(),
        "signature": format!("0x{}", "00".repeat(65)),
    });
    let resp = server.post("/orders").json(&body).await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_format_amount() {
    use api::types::format_amount;
    assert_eq!(format_amount(100_000_000, 8), "1");
    assert_eq!(format_amount(50_000 * 100_000_000, 8), "50000");
    assert_eq!(format_amount(150_000_000, 8), "1.5");
    assert_eq!(format_amount(0, 8), "0");
    assert_eq!(format_amount(1, 8), "0.00000001");
}

// ─── VEL-P2-03: GET /account/:address/orders/by-client-id/:client_id ─────────

#[tokio::test]
async fn test_get_order_by_client_id_found() {
    let user_id = types::UserId::from_hex(&user_addr()).unwrap();
    let mut e = engine_with_market();

    // Deposit funds and place a resting order with a client_order_id directly through the engine.
    e.process(
        Request::Deposit(DepositRequest {
            user: user_id.clone(),
            asset: AssetId::from_str("USDC"),
            amount: 100_000 * PRICE_SCALE,
            l1_tx_hash: [0u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        }),
        1,
    );

    let resp = e.process(
        Request::PostOrder(PostOrderRequest {
            user: user_id.clone(),
            market: MarketId::new("BTC", "USDC"),
            side: OrderSide::Bid,
            order_type: OrderType::GoodTillCanceled,
            price: 50_000 * PRICE_SCALE,
            quantity: 1 * QUANTITY_SCALE,
            nonce: 42,
            client_order_id: Some("test-coid-api".to_string()),
            signature: vec![0u8; 65],
            stp: Default::default(),
            min_quantity: None,
            display_quantity: None,
        }),
        2,
    );

    let order_id = resp
        .iter()
        .find_map(|r| {
            if let types::Response::OrderPosted(p) = r {
                Some(p.order_id)
            } else {
                None
            }
        })
        .expect("order must be posted");

    let server = test_server(e);
    let endpoint = format!("/account/{}/orders/by-client-id/test-coid-api", user_addr());
    let res = server.get(&endpoint).await;
    res.assert_status_ok();

    let body: Value = res.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["id"], order_id);
    assert_eq!(body["data"]["client_order_id"], "test-coid-api");
}

#[tokio::test]
async fn test_get_order_by_client_id_not_found() {
    let server = test_server(engine_with_market());
    let endpoint = format!("/account/{}/orders/by-client-id/nonexistent", user_addr());
    let res = server.get(&endpoint).await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ── WebSocket private-feed tests ─────────────────────────────────────────────

mod ws_tests {
    use super::*;
    use axum_test::TestServerConfig;
    use axum_test::TestWebSocket;
    use k256::ecdsa::{RecoveryId, Signature, SigningKey};
    use sha3::{Digest, Keccak256};
    use std::time::Duration;

    /// Create a server that shares its AppState with the caller so tests can
    /// inject events directly via `state.feeds`.  WS requires HTTP transport.
    fn ws_test_server(engine: MatchingEngine) -> (axum_test::TestServer, Arc<AppState>) {
        if std::env::var("ADMIN_TOKEN").is_err() {
            std::env::set_var("ADMIN_TOKEN", "test-admin-token");
        }
        let state = AppState::new(engine, super::make_wal());
        let state_clone = state.clone();
        let config = TestServerConfig::builder().http_transport().build();
        let server =
            axum_test::TestServer::new_with_config(api::build_router(state), config).unwrap();
        (server, state_clone)
    }

    /// Deterministic test key derived from an integer seed.
    fn test_key(seed: u64) -> SigningKey {
        use rand::{rngs::StdRng, SeedableRng};
        let mut rng = StdRng::seed_from_u64(seed);
        SigningKey::random(&mut rng)
    }

    fn address_of(key: &SigningKey) -> String {
        let vk = key.verifying_key();
        let pubkey = vk.to_encoded_point(false);
        let hash = Keccak256::digest(&pubkey.as_bytes()[1..]);
        let addr: [u8; 20] = hash[12..].try_into().unwrap();
        format!("0x{}", hex::encode(addr))
    }

    /// Sign `vela:auth:{nonce}` with Ethereum personal-sign prefix.
    fn sign_auth(key: &SigningKey, nonce: &str) -> String {
        let msg = api::auth::auth_signing_message(nonce);
        let hash = api::auth::eth_message_hash(&msg);
        let (sig, rid): (Signature, RecoveryId) = key.sign_prehash_recoverable(&hash).unwrap();
        let mut bytes = [0u8; 65];
        bytes[..64].copy_from_slice(&sig.to_bytes());
        bytes[64] = rid.to_byte();
        format!("0x{}", hex::encode(bytes))
    }

    /// Full RequestChallenge → Auth round-trip; asserts Authenticated is returned.
    async fn do_auth(ws: &mut TestWebSocket, key: &SigningKey) {
        ws.send_json(&json!({"type": "request_challenge"})).await;
        let challenge: Value = ws.receive_json().await;
        assert_eq!(challenge["type"], "challenge");
        let nonce = challenge["nonce"].as_str().unwrap().to_string();
        let addr = address_of(key);
        let sig = sign_auth(key, &nonce);
        ws.send_json(&json!({
            "type": "auth",
            "address": addr,
            "signature": sig,
            "nonce": nonce,
        }))
        .await;
        let resp: Value = ws.receive_json().await;
        assert_eq!(resp["type"], "authenticated", "auth failed: {resp}");
    }

    // ── Test 1: unauthenticated connection must not receive private events ──

    #[tokio::test]
    async fn test_ws_unauthenticated_no_private_events() {
        let (server, state) = ws_test_server(engine_with_market());
        let user = types::UserId::from_hex(&user_addr()).unwrap();

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        // Publish a private event before any auth.
        state.feeds.lock().await.publish_private(
            &user,
            api::types::WsServerMessage::OrderUpdate {
                order_id: 1,
                status: "open".to_string(),
                filled_quantity: "0".to_string(),
            },
        );

        // Nothing should arrive within 100 ms.
        let result = tokio::time::timeout(Duration::from_millis(100), ws.receive_text()).await;
        assert!(
            result.is_err(),
            "unauthenticated connection must not receive private events"
        );

        ws.close().await;
    }

    // ── Test 2: authenticated connection receives its own fills ──────────────

    #[tokio::test]
    async fn test_ws_authenticated_receives_own_fills() {
        let key = test_key(1);
        let addr = address_of(&key);
        let user = types::UserId::from_hex(&addr).unwrap();

        let (server, state) = ws_test_server(engine_with_market());

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;
        do_auth(&mut ws, &key).await;

        // Inject a Fill directly into the private feed.
        state.feeds.lock().await.publish_private(
            &user,
            api::types::WsServerMessage::Fill {
                maker_order_id: 10,
                taker_order_id: 11,
                price: "50000".to_string(),
                quantity: "1".to_string(),
                side: "bid".to_string(),
                maker_fee: "0".to_string(),
                taker_fee: "0".to_string(),
                timestamp: 9999,
            },
        );

        let fill: Value = tokio::time::timeout(Duration::from_millis(500), ws.receive_json())
            .await
            .expect("authenticated connection should receive its own fill within 500 ms");

        assert_eq!(fill["type"], "fill");
        assert_eq!(fill["price"], "50000");
        assert_eq!(fill["maker_order_id"], 10);

        ws.close().await;
    }

    // ── Test 3: user A cannot see user B's private events ───────────────────

    #[tokio::test]
    async fn test_ws_private_feed_isolation() {
        let key_a = test_key(2);
        let key_b = test_key(3);
        let addr_b = address_of(&key_b);
        let user_b = types::UserId::from_hex(&addr_b).unwrap();

        let (server, state) = ws_test_server(engine_with_market());

        // User A connects and authenticates.
        let mut ws_a = server.get_websocket("/ws").await.into_websocket().await;
        do_auth(&mut ws_a, &key_a).await;

        // Publish an event exclusively for user B.
        state.feeds.lock().await.publish_private(
            &user_b,
            api::types::WsServerMessage::OrderUpdate {
                order_id: 99,
                status: "open".to_string(),
                filled_quantity: "0".to_string(),
            },
        );

        // User A must not receive user B's event.
        let result = tokio::time::timeout(Duration::from_millis(100), ws_a.receive_text()).await;
        assert!(
            result.is_err(),
            "user A must not receive user B's private events"
        );

        ws_a.close().await;
    }

    // ── Test 4: auth without prior RequestChallenge is rejected ─────────────

    #[tokio::test]
    async fn test_ws_auth_requires_server_nonce() {
        let key = test_key(4);
        let addr = address_of(&key);
        let nonce = "client-chosen-nonce";
        let sig = sign_auth(&key, nonce);

        let (server, _state) = ws_test_server(engine_with_market());
        let mut ws = server.get_websocket("/ws").await.into_websocket().await;

        ws.send_json(&json!({
            "type": "auth",
            "address": addr,
            "signature": sig,
            "nonce": nonce,
        }))
        .await;

        let resp: Value = ws.receive_json().await;
        assert_eq!(resp["type"], "error");
        assert_eq!(resp["code"], "NO_CHALLENGE");

        ws.close().await;
    }
}

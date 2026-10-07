use api::AppState;
use axum_test::TestServer;
use bigdecimal::BigDecimal;
use engine::MatchingEngine;
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::sync::Arc;
use types::{
    AssetId, DepositRequest, FeeConfig, Market, MarketId, OrderSide, OrderType, PostOrderRequest,
    Request, PRICE_SCALE, QUANTITY_SCALE,
};
use uuid::Uuid;

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
    wallet(1).1
}

fn wallet(seed: u8) -> (SigningKey, String) {
    let key = SigningKey::from_slice(&[seed; 32]).unwrap();
    let public_key = key.verifying_key().to_encoded_point(false);
    let hash = Keccak256::digest(&public_key.as_bytes()[1..]);
    (key, format!("0x{}", hex::encode(&hash[12..])))
}

fn sign_personal(key: &SigningKey, message: &[u8]) -> String {
    let hash = api::auth::eth_message_hash(message);
    let (signature, recovery_id) = key.sign_prehash_recoverable(&hash).unwrap();
    let mut bytes = Vec::with_capacity(65);
    bytes.extend_from_slice(signature.to_bytes().as_ref());
    bytes.push(recovery_id.to_byte() + 27);
    format!("0x{}", hex::encode(bytes))
}

async fn auth_token_for(server: &TestServer, key: &SigningKey, address: &str) -> String {
    let challenge = server
        .post("/auth/challenge")
        .json(&json!({ "address": address }))
        .await;
    challenge.assert_status_ok();
    let challenge_body: Value = challenge.json();
    let nonce = challenge_body["data"]["nonce"].as_str().unwrap();
    let message = challenge_body["data"]["message"].as_str().unwrap();
    let session = server
        .post("/auth/session")
        .json(&json!({
            "address": address,
            "nonce": nonce,
            "signature": sign_personal(key, message.as_bytes()),
        }))
        .await;
    session.assert_status_ok();
    let session_body: Value = session.json();
    session_body["data"]["token"].as_str().unwrap().to_owned()
}

async fn user_auth_token(server: &TestServer) -> String {
    let (key, address) = wallet(1);
    auth_token_for(server, &key, &address).await
}

async fn get_as_user(server: &TestServer, path: &str) -> axum_test::TestResponse {
    let token = user_auth_token(server).await;
    server
        .get(path)
        .add_header("Authorization", format!("Bearer {token}"))
        .await
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
async fn concurrent_rfq_accepts_settle_only_once() {
    use api::rfq::{MakerProvenance, RfqQuote, RfqRequest, RfqStatus};

    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state = AppState::new_with_runtime(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Development,
    );
    let (requester_key, requester) = wallet(41);
    let (_, maker) = wallet(42);
    let requester_id = types::UserId::from_hex(&requester).unwrap();
    let maker_id = types::UserId::from_hex(&maker).unwrap();
    let rfq_id = api::rfq::next_rfq_id();
    let quote_id = api::rfq::next_quote_id();
    let expires_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 60_000;
    state.rfq.requests.insert(
        rfq_id,
        RfqRequest {
            rfq_id,
            requester: requester.clone(),
            market: "BTC-USDC".to_owned(),
            side: OrderSide::Bid,
            quantity: 1_000_000,
            expires_at_ms,
            created_at_ms: expires_at_ms - 60_000,
            status: RfqStatus::Open,
            filled_by_quote_id: None,
        },
    );
    state.rfq.quotes.insert(
        (rfq_id, quote_id),
        RfqQuote {
            quote_id,
            rfq_id,
            maker: maker.clone(),
            price: 2_000_000,
            quantity: 1_000_000,
            expires_at_ms,
            created_at_ms: expires_at_ms - 60_000,
            provenance: MakerProvenance::Allowlist,
        },
    );
    {
        let mut users = state.shards.user_state.write().await;
        for (user, asset, available) in [
            (requester_id.clone(), "USDC", 4_000_000),
            (maker_id.clone(), "BTC", 2_000_000),
        ] {
            let asset = AssetId::from_str(asset);
            users.balances.insert(
                (user.clone(), asset),
                types::Balance {
                    user,
                    asset,
                    available,
                    locked: 0,
                },
            );
        }
    }
    let server = TestServer::new(api::build_router(state.clone())).unwrap();
    let body = json!({
        "rfq_id": rfq_id,
        "quote_id": quote_id,
        "requester": requester,
        "nonce": 1,
        "signature": sign_personal(
            &requester_key,
            format!("vela:rfq:accept:{rfq_id}:{quote_id}:1").as_bytes(),
        ),
    });
    let (first, second) = tokio::join!(
        server.post("/rfq/accept").json(&body),
        server.post("/rfq/accept").json(&body),
    );
    let mut statuses = [first.status_code(), second.status_code()];
    statuses.sort();
    assert_eq!(
        statuses,
        [axum::http::StatusCode::OK, axum::http::StatusCode::CONFLICT]
    );
    let users = state.shards.user_state.read().await;
    assert_eq!(
        users.balances[&(requester_id.clone(), AssetId::from_str("USDC"))].available,
        2_000_000
    );
    assert_eq!(
        users.balances[&(requester_id.clone(), AssetId::from_str("BTC"))].available,
        1_000_000
    );
    assert_eq!(
        state.rfq.requests.get(&rfq_id).unwrap().status,
        RfqStatus::Filled
    );
    drop(users);

    // A credit overflow must leave every balance and the request untouched.
    let next_rfq_id = api::rfq::next_rfq_id();
    let next_quote_id = api::rfq::next_quote_id();
    state.rfq.requests.insert(
        next_rfq_id,
        RfqRequest {
            rfq_id: next_rfq_id,
            requester: requester.clone(),
            market: "BTC-USDC".to_owned(),
            side: OrderSide::Bid,
            quantity: 1_000_000,
            expires_at_ms,
            created_at_ms: expires_at_ms - 60_000,
            status: RfqStatus::Open,
            filled_by_quote_id: None,
        },
    );
    state.rfq.quotes.insert(
        (next_rfq_id, next_quote_id),
        RfqQuote {
            quote_id: next_quote_id,
            rfq_id: next_rfq_id,
            maker: maker.clone(),
            price: 2_000_000,
            quantity: 1_000_000,
            expires_at_ms,
            created_at_ms: expires_at_ms - 60_000,
            provenance: MakerProvenance::Allowlist,
        },
    );
    {
        let mut users = state.shards.user_state.write().await;
        users
            .balances
            .get_mut(&(maker_id.clone(), AssetId::from_str("USDC")))
            .unwrap()
            .available = u64::MAX;
    }
    let overflow_accept = server
        .post("/rfq/accept")
        .json(&json!({
            "rfq_id": next_rfq_id,
            "quote_id": next_quote_id,
            "requester": requester,
            "nonce": 2,
            "signature": sign_personal(
                &requester_key,
                format!("vela:rfq:accept:{next_rfq_id}:{next_quote_id}:2").as_bytes(),
            ),
        }))
        .await;
    overflow_accept.assert_status(axum::http::StatusCode::CONFLICT);
    let users = state.shards.user_state.read().await;
    assert_eq!(
        users.balances[&(maker_id, AssetId::from_str("USDC"))].available,
        u64::MAX
    );
    assert_eq!(
        users.balances[&(requester_id, AssetId::from_str("USDC"))].available,
        2_000_000
    );
    assert_eq!(
        state.rfq.requests.get(&next_rfq_id).unwrap().status,
        RfqStatus::Open
    );
}

#[tokio::test]
async fn rfq_request_rejects_unlisted_market() {
    let server = test_server(engine_with_market());
    let (key, requester) = wallet(43);
    let expires_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 60_000;
    let message = format!("vela:rfq:request:FAKE-USDC:Bid:1000000:{expires_at_ms}:1");
    server
        .post("/rfq/request")
        .json(&json!({
            "requester": requester,
            "market": "FAKE-USDC",
            "side": "Bid",
            "quantity": 1_000_000,
            "expires_at_ms": expires_at_ms,
            "nonce": 1,
            "signature": sign_personal(&key, message.as_bytes()),
        }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_health() {
    let server = test_server(engine_with_market());
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    resp.assert_text("ok");
}

#[tokio::test]
async fn status_reports_the_process_build_identity() {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state = AppState::new_with_runtime(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Development,
    );
    let expected = state.build_id.clone();
    let server = TestServer::new(api::build_router(state)).unwrap();
    let status: Value = server.get("/status").await.json();
    assert_eq!(status["data"]["build_id"], json!(expected));
}

#[tokio::test]
async fn versioned_v1_routes_match_legacy_contracts() {
    let server = test_server(engine_with_market());
    let legacy: Value = server.get("/markets").await.json();
    let versioned = server.get("/v1/markets").await;
    versioned.assert_status_ok();
    assert_eq!(legacy, versioned.json::<Value>());

    let master = user_addr();
    let agents = server.get(&format!("/v1/agents/{master}")).await;
    agents.assert_status_ok();
    assert_eq!(agents.json::<Value>()["data"]["agents"], json!([]));

    let spec = api::openapi::openapi_spec();
    for path in [
        "/v1/markets",
        "/v1/orders",
        "/v1/portfolio/{address}",
        "/v1/agents/register",
    ] {
        assert!(spec["paths"].get(path).is_some(), "missing {path}");
    }
}

#[tokio::test]
async fn test_development_readiness_allows_unconfigured_ledger() {
    let server = test_server(engine_with_market());
    let response = server.get("/ready").await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["ledger"], "not_configured");
    assert!(body["recovery_replication"].is_null());
}

#[tokio::test]
async fn storage_fence_keeps_liveness_but_disables_readiness_and_posts() {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state = AppState::new_with_runtime(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Development,
    );
    state.storage_health.fence(
        "wal_commit",
        "/private/sensitive/path: injected write failure",
    );
    let server = TestServer::new(api::build_router(state)).unwrap();

    server.get("/health").await.assert_status_ok();
    let readiness = server.get("/ready").await;
    readiness.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = readiness.json();
    assert_eq!(body["matcher_storage"]["healthy"], false);
    assert_eq!(body["matcher_storage"]["failure_code"], "wal_commit");
    assert!(!body.to_string().contains("sensitive"));

    let post = server.post("/mcp").json(&json!({})).await;
    post.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
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
async fn test_production_readiness_requires_operational_dependencies_with_current_ledger_schema(
    pool: sqlx::PgPool,
) {
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
    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json();
    assert_eq!(body["ok"], false);
    assert_eq!(body["ledger"], "configured");
    assert!(body.get("ledger_schema_version").is_none());
    assert_eq!(body["engine_leadership"]["configured"], true);
    assert_eq!(body["engine_leadership"]["state"], "active");
    assert!(body["engine_leadership"].get("owner_id").is_none());
    assert_eq!(body["operational_dependencies"]["healthy"], false);
    for dependency in ["indexer", "signer", "custody_rpc", "reconciler"] {
        assert_eq!(
            body["operational_dependencies"]["dependencies"][dependency]["failure_code"],
            "not_configured"
        );
    }
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn second_ledger_backed_engine_refuses_to_initialize(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool));
    let first = ledger
        .acquire_engine_leadership(Uuid::new_v4(), std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    let result = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Production,
        Some(Arc::clone(&ledger)),
    )
    .await;
    let error = match result {
        Ok(_) => panic!("second engine unexpectedly initialized"),
        Err(error) => error,
    };
    assert!(error.contains("another engine holds the authoritative leadership lease"));
    assert!(ledger.release_engine_leadership(first.token).await.unwrap());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn superseded_engine_latches_mutation_fence(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool.clone()));
    let state = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Production,
        Some(Arc::clone(&ledger)),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE ledger_engine_leadership SET lease_expires_at = transaction_timestamp() - interval '1 second'",
    )
    .execute(&pool)
    .await
    .unwrap();
    ledger
        .acquire_engine_leadership(Uuid::new_v4(), std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();

    let error = state
        .engine_leadership
        .as_ref()
        .unwrap()
        .validate_at_commit()
        .await
        .unwrap_err();
    assert!(error.contains("engine leadership"));
    assert!(!state.storage_health.is_healthy());
    assert_eq!(
        state.storage_health.snapshot().failure_code.as_deref(),
        Some("engine_leadership_lost")
    );
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn replay_failure_releases_startup_leadership(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool.clone()));
    let asset_id = Uuid::new_v4();
    ledger
        .create_asset(&ledger::NewAsset {
            id: asset_id,
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(8453),
            contract_address: None,
        })
        .await
        .unwrap();
    let account_id = Uuid::new_v4();
    ledger
        .create_account(&ledger::NewAccount {
            id: account_id,
            asset_id,
            owner_type: "customer".to_owned(),
            owner_reference: user_addr(),
            account_type: "customer_available".to_owned(),
            allow_negative: false,
        })
        .await
        .unwrap();
    sqlx::query("UPDATE ledger_account_balances SET balance = 1 WHERE account_id = $1")
        .bind(account_id)
        .execute(&pool)
        .await
        .unwrap();

    let result = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::Production,
        Some(Arc::clone(&ledger)),
    )
    .await;
    assert!(
        matches!(result, Err(error) if error.starts_with("authoritative ledger replay failed"))
    );
    assert!(ledger
        .acquire_engine_leadership(Uuid::new_v4(), std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .is_some());
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

    let balances = get_as_user(&server, &format!("/account/{}/balances", user_addr())).await;
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
    for (path, payload) in [
        (
            "/subaccounts/create",
            json!({"master": user_addr(), "subaccount_id": 1, "name": "test", "nonce": 1, "signature": "0x00"}),
        ),
        (
            "/vaults/create",
            json!({"operator": user_addr(), "name": "test", "nonce": 1, "signature": "0x00"}),
        ),
    ] {
        server
            .post(path)
            .json(&payload)
            .await
            .assert_status(axum::http::StatusCode::NOT_IMPLEMENTED);
    }
}

#[tokio::test]
async fn test_deposit_status_fails_closed_without_authoritative_ledger() {
    let server = test_server(engine_with_market());
    let response = get_as_user(&server, &format!("/account/{}/deposits", user_addr())).await;
    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json();
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("authoritative deposit status"));
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn test_deposit_status_is_customer_scoped_and_filterable(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let ledger = Arc::new(ledger::Ledger::from_pool(pool));
    let asset_id = Uuid::new_v4();
    ledger
        .create_asset(&ledger::NewAsset {
            id: asset_id,
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(84532),
            contract_address: Some("0x1111111111111111111111111111111111111111".to_owned()),
        })
        .await
        .unwrap();
    let account_id = Uuid::new_v4();
    ledger
        .create_account(&ledger::NewAccount {
            id: account_id,
            asset_id,
            owner_type: "customer".to_owned(),
            owner_reference: user_addr(),
            account_type: "customer_available".to_owned(),
            allow_negative: false,
        })
        .await
        .unwrap();
    let transaction_hash = format!("0x{}", "22".repeat(32));
    let deposit_id = Uuid::new_v4();
    ledger
        .observe_deposit(&ledger::ObserveDeposit {
            deposit_id,
            customer_reference: user_addr(),
            destination_account_id: account_id,
            asset_id,
            amount: BigDecimal::from(1_500_000_i64),
            event: ledger::DepositChainEvent {
                chain_id: 84532,
                block_number: 1234,
                block_hash: format!("0x{}", "11".repeat(32)),
                transaction_hash: transaction_hash.clone(),
                transaction_index: 2,
                log_index: 3,
                vault_address: "0x3333333333333333333333333333333333333333".to_owned(),
                vault_deposit_id: format!("0x{}", "44".repeat(32)),
                sender_address: user_addr(),
                asset_address: Some("0x1111111111111111111111111111111111111111".to_owned()),
                actual_amount: BigDecimal::from(1_500_000_i64),
            },
        })
        .await
        .unwrap();

    let state = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::TestnetDemo,
        Some(ledger),
    )
    .await
    .unwrap();
    let server = TestServer::new(api::build_router(state)).unwrap();

    let response = get_as_user(&server, &format!("/account/{}/deposits", user_addr())).await;
    response.assert_status_ok();
    let body: Value = response.json();
    let deposits = body["data"].as_array().unwrap();
    assert_eq!(deposits.len(), 1);
    assert_eq!(deposits[0]["id"], deposit_id.to_string());
    assert_eq!(deposits[0]["asset"], "USDC");
    assert_eq!(deposits[0]["amount"], "1.5");
    assert_eq!(deposits[0]["amount_atomic"], "1500000");
    assert_eq!(deposits[0]["state"], "observed");
    assert_eq!(deposits[0]["transaction_hash"], transaction_hash);

    let absent_hash = format!("0x{}", "99".repeat(32));
    let response = get_as_user(
        &server,
        &format!(
            "/account/{}/deposits?transaction_hash={absent_hash}",
            user_addr()
        ),
    )
    .await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert!(body["data"].as_array().unwrap().is_empty());

    let invalid = get_as_user(
        &server,
        &format!(
            "/account/{}/deposits?transaction_hash=not-a-hash",
            user_addr()
        ),
    )
    .await;
    invalid.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_withdrawal_status_fails_closed_without_authoritative_ledger() {
    let server = test_server(engine_with_market());
    let response = get_as_user(&server, &format!("/account/{}/withdrawals", user_addr())).await;
    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn test_withdrawal_reservation_and_cancellation_are_ledger_backed(pool: sqlx::PgPool) {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let (key, address) = wallet(9);
    let ledger = Arc::new(ledger::Ledger::from_pool(pool));
    let asset_id = Uuid::new_v4();
    ledger
        .create_asset(&ledger::NewAsset {
            id: asset_id,
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(84532),
            contract_address: Some("0x3333333333333333333333333333333333333333".to_owned()),
        })
        .await
        .unwrap();
    let available_id = Uuid::new_v4();
    let operations_id = Uuid::new_v4();
    for account in [
        ledger::NewAccount {
            id: available_id,
            asset_id,
            owner_type: "customer".to_owned(),
            owner_reference: address.clone(),
            account_type: "customer_available".to_owned(),
            allow_negative: false,
        },
        ledger::NewAccount {
            id: Uuid::new_v4(),
            asset_id,
            owner_type: "custody".to_owned(),
            owner_reference: "0x2222222222222222222222222222222222222222".to_owned(),
            account_type: "custody_vault".to_owned(),
            allow_negative: true,
        },
        ledger::NewAccount {
            id: operations_id,
            asset_id,
            owner_type: "platform".to_owned(),
            owner_reference: "platform:test".to_owned(),
            account_type: "operational_adjustment".to_owned(),
            allow_negative: true,
        },
    ] {
        ledger.create_account(&account).await.unwrap();
    }
    sqlx::query(
        r#"
        INSERT INTO ledger_chain_deployments (
            id, chain_id, network, vault_address, deployment_version,
            runtime_code_hash, deposit_event_signature, deposit_event_topic,
            finality_policy, start_block
        ) VALUES ($1, 84532, 'base-sepolia', $2, 'test-v3',
                  $3, 'Deposited(bytes32,address,bytes32,address,uint256)', $4,
                  'rpc_finalized_two_provider', 0)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind("0x2222222222222222222222222222222222222222")
    .bind(format!("0x{}", "11".repeat(32)))
    .bind(format!("0x{}", "22".repeat(32)))
    .execute(ledger.pool())
    .await
    .unwrap();
    ledger
        .post(&ledger::PostTransaction {
            id: Uuid::new_v4(),
            operation_type: "deposit_credit".to_owned(),
            idempotency_key: "api-withdrawal-seed".to_owned(),
            request_hash: [3; 32],
            external_reference: None,
            metadata: json!({ "test": true }),
            legs: vec![
                ledger::JournalLegInput {
                    account_id: operations_id,
                    amount: BigDecimal::from(-5_000_000),
                },
                ledger::JournalLegInput {
                    account_id: available_id,
                    amount: BigDecimal::from(5_000_000),
                },
            ],
        })
        .await
        .unwrap();
    let state = AppState::new_with_runtime_and_ledger(
        engine_with_market(),
        make_wal(),
        types::RuntimeMode::TestnetDemo,
        Some(ledger.clone()),
    )
    .await
    .unwrap();
    let server = TestServer::new(api::build_router(state)).unwrap();

    let withdrawal_id = Uuid::new_v4().to_string();
    let amount = "100000000";
    let nonce = 41;
    let message = api::auth::withdrawal_reservation_signing_message(
        &withdrawal_id,
        &address,
        &address,
        "USDC",
        amount,
        nonce,
    );
    let request = json!({
        "withdrawal_id": withdrawal_id,
        "asset": "USDC",
        "amount": amount,
        "nonce": nonce,
        "address": address,
        "destination_address": address,
        "signature": sign_personal(&key, &message),
    });
    let first = server.post("/withdrawals").json(&request).await;
    first.assert_status_ok();
    let body: Value = first.json();
    assert_eq!(body["data"]["state"], "reserved");
    assert_eq!(body["data"]["amount"], "1");
    assert_eq!(body["data"]["amount_atomic"], "1000000");

    let replay = server.post("/withdrawals").json(&request).await;
    replay.assert_status_ok();
    assert_eq!(
        ledger.balance(available_id).await.unwrap(),
        BigDecimal::from(4_000_000)
    );
    let token = auth_token_for(&server, &key, &address).await;
    let listed = server
        .get(&format!("/account/{address}/withdrawals"))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    listed.assert_status_ok();
    let body: Value = listed.json();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    let cancel_message = api::auth::withdrawal_cancel_signing_message(&withdrawal_id, nonce);
    let cancel = server
        .post(&format!("/withdrawals/{withdrawal_id}/cancel"))
        .json(&json!({
            "nonce": nonce,
            "address": address,
            "signature": sign_personal(&key, &cancel_message),
        }))
        .await;
    cancel.assert_status_ok();
    let body: Value = cancel.json();
    assert_eq!(body["data"]["state"], "cancelled");
    assert_eq!(
        ledger.balance(available_id).await.unwrap(),
        BigDecimal::from(5_000_000)
    );

    let signing = server
        .post("/withdrawal-signature")
        .json(&json!({
            "user": address,
            "asset": "ETH",
            "amount": "0.01",
            "nonce": 1
        }))
        .await;
    signing.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
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
    let resp = get_as_user(&server, &format!("/account/{}/balances", user_addr())).await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert!(body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_portfolio_response_matches_documented_empty_shape() {
    let server = test_server(engine_with_market());
    let (key, address) = wallet(1);
    let token = auth_token_for(&server, &key, &address).await;

    for (method, expected) in [("fifo", "FIFO"), ("hifo", "HIFO")] {
        let response = server
            .get(&format!("/portfolio/{address}?method={method}"))
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        response.assert_status_ok();
        let body: Value = response.json();
        let data = &body["data"];
        assert_eq!(data["address"], address.to_ascii_lowercase());
        assert_eq!(data["cost_basis_method"], expected);
        assert_eq!(data["realized_pnl_usdc"], "0.000000");
        assert_eq!(data["unrealized_pnl_usdc"], "0.000000");
        assert_eq!(data["total_pnl_usdc"], "0.000000");
        assert!(data["markets"].as_array().unwrap().is_empty());
        assert!(data["per_market"].as_array().unwrap().is_empty());
        assert!(data["tax_lots"].as_array().unwrap().is_empty());
    }

    let csv = server
        .get(&format!("/portfolio/{address}/csv"))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    csv.assert_status_ok();
    assert_eq!(
        csv.text(),
        "timestamp_ms,market,side,role,price_usdc,quantity_base,notional_usdc,fee_usdc,counterparty,fill_id\n"
    );
}

#[tokio::test]
async fn test_every_customer_data_route_requires_wallet_session() {
    let server = test_server(engine_with_market());
    let address = user_addr();
    let paths = [
        format!("/account/{address}/balances"),
        format!("/account/{address}/orders"),
        format!("/account/{address}/orders/by-client-id/missing"),
        format!("/account/{address}/fills"),
        format!("/account/{address}/deposits"),
        format!("/account/{address}/withdrawals"),
        format!("/portfolio/{address}"),
        format!("/portfolio/{address}/csv"),
    ];

    for path in paths {
        let missing = server.get(&path).await;
        missing.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn test_http_auth_challenge_is_single_use_and_session_is_revocable() {
    let server = test_server(engine_with_market());
    let (key, address) = wallet(1);
    let challenge = server
        .post("/auth/challenge")
        .json(&json!({ "address": address }))
        .await;
    challenge.assert_status_ok();
    let body: Value = challenge.json();
    let nonce = body["data"]["nonce"].as_str().unwrap();
    let message = body["data"]["message"].as_str().unwrap();
    let proof = json!({
        "address": address,
        "nonce": nonce,
        "signature": sign_personal(&key, message.as_bytes()),
    });

    let session = server.post("/auth/session").json(&proof).await;
    session.assert_status_ok();
    let session_body: Value = session.json();
    let token = session_body["data"]["token"].as_str().unwrap();

    let replay = server.post("/auth/session").json(&proof).await;
    replay.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let revoke = server
        .post("/auth/session/revoke")
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    revoke.assert_status_ok();

    let after_revoke = server
        .get(&format!("/account/{address}/balances"))
        .add_header("Authorization", format!("Bearer {token}"))
        .await;
    after_revoke.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_http_auth_challenge_issuance_is_rate_limited() {
    let server = test_server(engine_with_market());
    let address = user_addr();
    for _ in 0..10 {
        server
            .post("/auth/challenge")
            .json(&json!({ "address": address }))
            .await
            .assert_status_ok();
    }
    server
        .post("/auth/challenge")
        .json(&json!({ "address": address }))
        .await
        .assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_http_auth_session_cannot_read_another_accounts_private_routes() {
    let server = test_server(engine_with_market());
    let token = user_auth_token(&server).await;
    let (_, other_address) = wallet(2);
    let paths = [
        format!("/account/{other_address}/balances"),
        format!("/account/{other_address}/orders"),
        format!("/account/{other_address}/orders/by-client-id/missing"),
        format!("/account/{other_address}/fills"),
        format!("/account/{other_address}/deposits"),
        format!("/account/{other_address}/withdrawals"),
        format!("/portfolio/{other_address}"),
        format!("/portfolio/{other_address}/csv"),
    ];
    for path in paths {
        let response = server
            .get(&path)
            .add_header("Authorization", format!("Bearer {token}"))
            .await;
        response.assert_status(axum::http::StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn test_private_fill_history_is_account_scoped_and_redacted() {
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state = AppState::new(engine_with_market(), make_wal());
    let (_, other_address) = wallet(2);
    state.fills.lock().await.extend([
        api::types::StoredFill {
            id: "own-fill".to_owned(),
            market_id: "BTC-USDC".to_owned(),
            price: 50_000 * PRICE_SCALE,
            quantity: QUANTITY_SCALE,
            maker_order_id: 10,
            taker_order_id: 11,
            maker_address: user_addr(),
            taker_address: other_address.clone(),
            timestamp: 20,
            side: "bid".to_owned(),
            maker_fee_micro: Some(0),
            taker_fee_micro: Some(0),
            synthetic: false,
            toxicity_score: 0.0,
        },
        api::types::StoredFill {
            id: "other-fill".to_owned(),
            market_id: "BTC-USDC".to_owned(),
            price: 49_000 * PRICE_SCALE,
            quantity: QUANTITY_SCALE,
            maker_order_id: 12,
            taker_order_id: 13,
            maker_address: other_address.clone(),
            taker_address: wallet(3).1,
            timestamp: 10,
            side: "ask".to_owned(),
            maker_fee_micro: Some(0),
            taker_fee_micro: Some(0),
            synthetic: false,
            toxicity_score: 0.0,
        },
    ]);
    let server = TestServer::new(api::build_router(state)).unwrap();

    let response = get_as_user(&server, &format!("/account/{}/fills", user_addr())).await;
    response.assert_status_ok();
    let body: Value = response.json();
    let fills = body["data"].as_array().unwrap();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0]["id"], "own-fill");
    assert_eq!(fills[0]["role"], "maker");
    assert_eq!(fills[0]["side"], "sell");
    assert_eq!(fills[0]["price"], "50000");
    assert!(fills[0].get("maker_address").is_none());
    assert!(fills[0].get("taker_address").is_none());
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
    let resp = get_as_user(&server, &format!("/account/{}/balances", user_addr())).await;
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
            quantity: QUANTITY_SCALE,
            nonce: 42,
            client_order_id: Some("test-coid-api".to_string()),
            signature: vec![0u8; 65],
            stp: types::SelfTradePreventionMode::CancelTaker,
            min_quantity: None,
            display_quantity: Some(QUANTITY_SCALE / 2),
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
    let res = get_as_user(&server, &endpoint).await;
    res.assert_status_ok();

    let body: Value = res.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["id"], order_id);
    assert_eq!(body["data"]["client_order_id"], "test-coid-api");
    assert_eq!(body["data"]["display_quantity"], "0.5");
    assert_eq!(body["data"]["stp"], "CancelTaker");
    let open = get_as_user(&server, &format!("/account/{}/orders", user_addr())).await;
    open.assert_status_ok();
    let open_body: Value = open.json();
    assert_eq!(open_body["data"][0]["display_quantity"], "0.5");
    assert_eq!(open_body["data"][0]["stp"], "CancelTaker");
}

#[tokio::test]
async fn test_get_order_by_client_id_not_found() {
    let server = test_server(engine_with_market());
    let endpoint = format!("/account/{}/orders/by-client-id/nonexistent", user_addr());
    let res = get_as_user(&server, &endpoint).await;
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

    #[tokio::test]
    async fn test_ws_accepts_http_session_without_second_wallet_signature() {
        let (key, address) = wallet(7);
        let user = types::UserId::from_hex(&address).unwrap();
        let (server, state) = ws_test_server(engine_with_market());
        let token = auth_token_for(&server, &key, &address).await;

        let mut ws = server.get_websocket("/ws").await.into_websocket().await;
        ws.send_json(&json!({
            "type": "session_auth",
            "address": address,
            "token": token,
        }))
        .await;
        let response: Value = ws.receive_json().await;
        assert_eq!(response["type"], "authenticated", "auth failed: {response}");

        state.feeds.lock().await.publish_private(
            &user,
            api::types::WsServerMessage::OrderUpdate {
                order_id: 77,
                status: "open".to_string(),
                filled_quantity: "0".to_string(),
            },
        );
        let update: Value = tokio::time::timeout(Duration::from_millis(500), ws.receive_json())
            .await
            .expect("session-authenticated connection should receive private events");
        assert_eq!(update["type"], "order_update");
        assert_eq!(update["order_id"], 77);
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

fn twap_request() -> api::algo_request::TwapRequest {
    let (key, address) = wallet(1);
    let mut request = api::algo_request::TwapRequest {
        address,
        market: "BTC-USDC".into(),
        side: OrderSide::Bid,
        quantity: QUANTITY_SCALE,
        duration_secs: 60,
        price_limit: Some(60_000 * PRICE_SCALE),
        slices: Some(2),
        nonce: 100,
        signature: String::new(),
        signature_version: 2,
    };
    request.signature = sign_personal(&key, &request.signing_message());
    request
}

#[tokio::test]
async fn twap_rejects_changed_execution_fields_and_legacy_signatures() {
    let server = test_server(funded_engine());
    for field in ["side", "price_limit", "slices", "signature_version"] {
        let mut body = serde_json::to_value(twap_request()).unwrap();
        body[field] = match field {
            "side" => json!("Ask"),
            "price_limit" => json!(60_001 * PRICE_SCALE),
            "slices" => json!(3),
            _ => json!(1),
        };
        let response = server.post("/v1/orders/algo/twap").json(&body).await;
        response.assert_status(if field == "signature_version" {
            axum::http::StatusCode::BAD_REQUEST
        } else {
            axum::http::StatusCode::UNAUTHORIZED
        });
    }
}

#[tokio::test]
async fn twap_replay_private_progress_and_prompt_cancel() {
    let server = test_server(funded_engine());
    let request = twap_request();
    let created = server.post("/v1/orders/algo/twap").json(&request).await;
    created.assert_status_ok();
    let parent_id = created.json::<Value>()["data"]["parent_id"]
        .as_u64()
        .unwrap();
    server
        .post("/v1/orders/algo/twap")
        .json(&request)
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);
    let path = format!("/v1/orders/algo/{parent_id}");
    server
        .get(&path)
        .await
        .assert_status(axum::http::StatusCode::UNAUTHORIZED);
    let (other_key, other) = wallet(2);
    let token = auth_token_for(&server, &other_key, &other).await;
    server
        .get(&path)
        .add_header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .await
        .assert_status(axum::http::StatusCode::FORBIDDEN);
    let owner_token = user_auth_token(&server).await;
    let listing = server
        .get(&format!("/v1/account/{}/algos", user_addr()))
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {owner_token}"),
        )
        .await;
    listing.assert_status_ok();
    assert_eq!(listing.json::<Value>()["data"][0]["parent_id"], parent_id);
    let (key, address) = wallet(1);
    let nonce = 1000;
    let signature = sign_personal(
        &key,
        format!("vela:algo:cancel:{parent_id}:{nonce}").as_bytes(),
    );
    server
        .post("/v1/orders/algo/cancel")
        .json(&json!({"parent_id":parent_id,"address":address,"nonce":nonce,"signature":signature}))
        .await
        .assert_status_ok();
    for _ in 0..200 {
        let status = server
            .get(&path)
            .add_header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {owner_token}"),
            )
            .await;
        if status.json::<Value>()["data"]["status"] == "Canceled" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("cancellation did not wake the parent before its 30-second next slice");
}

#[tokio::test]
async fn twap_empty_book_finishes_incomplete_with_exact_remaining_quantity() {
    let server = test_server(funded_engine());
    let mut request = twap_request();
    request.slices = Some(1);
    request.duration_secs = 1;
    request.signature = sign_personal(&wallet(1).0, &request.signing_message());
    let created = server.post("/v1/orders/algo/twap").json(&request).await;
    created.assert_status_ok();
    let parent_id = created.json::<Value>()["data"]["parent_id"]
        .as_u64()
        .unwrap();
    let token = user_auth_token(&server).await;
    for _ in 0..300 {
        let status = server
            .get(&format!("/v1/orders/algo/{parent_id}"))
            .add_header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .await;
        let body = status.json::<Value>();
        if body["data"]["status"] != "Running" {
            assert_eq!(body["data"]["status"], "Incomplete");
            assert_eq!(body["data"]["filled_quantity"], 0);
            assert_eq!(body["data"]["remaining_quantity"], QUANTITY_SCALE);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("single-slice parent never reached a terminal state");
}

#[tokio::test]
async fn twap_continuation_fails_after_agent_expiry_revocation_and_scope_change() {
    use api::agents::{AgentDelegation, AgentRegistry, CapabilityScope};
    let registry = AgentRegistry::new();
    let master = types::UserId::from_hex(&wallet(1).1).unwrap();
    let agent = types::UserId::from_hex(&wallet(2).1).unwrap();
    let market = MarketId::new("BTC", "USDC");
    let delegation = AgentDelegation {
        master: master.clone(),
        agent: agent.clone(),
        expires_at_ms: 100,
        max_notional_per_order: 1_000_000,
        revoked: false,
        nonce: 1,
        scope: CapabilityScope::default(),
    };
    registry.register_signed(delegation.clone()).unwrap();
    assert!(registry
        .authorize_continuation(&master, &agent, 99, &market, OrderSide::Bid)
        .await
        .is_ok());
    assert!(registry
        .authorize_continuation(&master, &agent, 100, &market, OrderSide::Bid)
        .await
        .is_err());
    assert!(registry.revoke(&master, &agent, 2));
    assert!(registry
        .authorize_continuation(&master, &agent, 99, &market, OrderSide::Bid)
        .await
        .is_err());
    let mut changed = delegation;
    changed.nonce = 3;
    changed.scope.allowed_sides = Some(vec![OrderSide::Ask]);
    registry.register_signed(changed).unwrap();
    assert!(registry
        .authorize_continuation(&master, &agent, 99, &market, OrderSide::Bid)
        .await
        .is_err());
    assert!(registry
        .authorize_continuation(&master, &master, 100, &market, OrderSide::Bid)
        .await
        .is_ok());
}

#[tokio::test]
async fn scoped_agent_cannot_cancel_parent_in_another_market() {
    use api::agents::{AgentDelegation, CapabilityScope};
    if std::env::var("ADMIN_TOKEN").is_err() {
        std::env::set_var("ADMIN_TOKEN", "test-admin-token");
    }
    let state =
        AppState::new_with_runtime(funded_engine(), make_wal(), types::RuntimeMode::Development);
    let (agent_key, agent_address) = wallet(2);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    state
        .agents
        .register_signed(AgentDelegation {
            master: types::UserId::from_hex(&user_addr()).unwrap(),
            agent: types::UserId::from_hex(&agent_address).unwrap(),
            expires_at_ms: now_ms + 60_000,
            max_notional_per_order: u64::MAX,
            revoked: false,
            nonce: 1,
            scope: CapabilityScope {
                allowed_markets: Some(vec![MarketId::new("ETH", "USDC")]),
                ..Default::default()
            },
        })
        .unwrap();
    let server = TestServer::new(api::build_router(state)).unwrap();
    let created = server
        .post("/v1/orders/algo/twap")
        .json(&twap_request())
        .await;
    created.assert_status_ok();
    let parent_id = created.json::<Value>()["data"]["parent_id"]
        .as_u64()
        .unwrap();
    let nonce = 1000;
    let message = format!("vela:algo:cancel:{parent_id}:{nonce}");
    server.post("/v1/orders/algo/cancel").json(&json!({"address":user_addr(),"parent_id":parent_id,"nonce":nonce,"signature":sign_personal(&agent_key,message.as_bytes())})).await.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    server.post("/v1/orders/algo/cancel").json(&json!({"address":user_addr(),"parent_id":parent_id,"nonce":nonce,"signature":sign_personal(&wallet(1).0,message.as_bytes())})).await.assert_status_ok();
}

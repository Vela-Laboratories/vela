//! Opt-in local chain integration proof. The two loopback URLs deliberately
//! share one Anvil process; this tests chain-to-ledger wiring, not independent
//! provider operation.

use bigdecimal::BigDecimal;
use engine::{
    BatchMetrics, BatchedRequest, FinancialCommitter, MarketShards, MatchingEngine, UserState,
};
use indexer::{Config, DepositIndexer, HttpRpc, IndexerStore, NewDeployment, Rpc};
use ledger::{AccountLookup, AccountProvisionRequest, Ledger, NewAsset};
use reqwest::Url;
use sha3::{Digest, Keccak256};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use types::{
    AssetId, FeeConfig, Market, MarketId, OrderSide, OrderType, PostOrderRequest, Request,
    Response, UserId, PRICE_SCALE, QUANTITY_SCALE,
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "run with scripts/with_local_custody_deps.sh scripts/test_local_v3_deposits.sh"]
async fn local_eth_and_usdc_deposits_reach_the_authoritative_ledger() {
    let vault = env::var("VELA_LOCAL_VAULT").expect("local vault address");
    let rpc_url = env::var("ANVIL_RPC_URL").expect("local Anvil RPC URL");
    let usdc = env::var("VELA_LOCAL_USDC").expect("local USDC address");
    let eth_depositor = env::var("VELA_LOCAL_ETH_DEPOSITOR").expect("ETH depositor");
    let usdc_depositor = env::var("VELA_LOCAL_USDC_DEPOSITOR").expect("USDC depositor");
    let start_block: i64 = env::var("VELA_LOCAL_DEPLOYMENT_BLOCK")
        .expect("local deployment block")
        .parse()
        .expect("deployment block integer");
    let database_url = env::var("DATABASE_URL").expect("local PostgreSQL URL");
    let ledger = Ledger::connect(&database_url, 4).await.unwrap();
    ledger.ensure_schema_current().await.unwrap();

    ledger
        .create_asset(&NewAsset {
            id: Uuid::new_v4(),
            code: "ETH".to_owned(),
            decimals: 18,
            chain_id: Some(84532),
            contract_address: None,
        })
        .await
        .unwrap();
    ledger
        .create_asset(&NewAsset {
            id: Uuid::new_v4(),
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(84532),
            contract_address: Some(usdc.to_ascii_lowercase()),
        })
        .await
        .unwrap();
    let lookup = AccountLookup {
        asset_code: "ETH".to_owned(),
        owner_type: "custody".to_owned(),
        owner_reference: vault.to_ascii_lowercase(),
        account_type: "custody_vault".to_owned(),
    };
    let custody_id = ledger
        .ensure_accounts(&[AccountProvisionRequest {
            asset_code: "ETH".to_owned(),
            owner_type: "custody".to_owned(),
            owner_reference: vault.to_ascii_lowercase(),
            account_type: "custody_vault".to_owned(),
            allow_negative: true,
        }])
        .await
        .unwrap()[&lookup]
        .id;
    let usdc_lookup = AccountLookup {
        asset_code: "USDC".to_owned(),
        ..lookup.clone()
    };
    let usdc_custody_id = ledger
        .ensure_accounts(&[AccountProvisionRequest {
            asset_code: "USDC".to_owned(),
            owner_type: "custody".to_owned(),
            owner_reference: vault.to_ascii_lowercase(),
            account_type: "custody_vault".to_owned(),
            allow_negative: true,
        }])
        .await
        .unwrap()[&usdc_lookup]
        .id;

    let primary_url = Url::parse(&rpc_url).unwrap();
    let mut secondary_url = primary_url.clone();
    secondary_url.set_host(Some("localhost")).unwrap();
    let primary = HttpRpc::new(
        "local-anvil-a".to_owned(),
        primary_url.clone(),
        Duration::from_secs(5),
    )
    .unwrap();
    let secondary = HttpRpc::new(
        "local-anvil-b".to_owned(),
        secondary_url.clone(),
        Duration::from_secs(5),
    )
    .unwrap();
    let finalized = primary.block("finalized").await.unwrap();
    let code = primary.code(&vault, finalized.number).await.unwrap();
    let code_hash = format!(
        "0x{}",
        hex::encode(Keccak256::digest(hex::decode(&code[2..]).unwrap()))
    );
    let store = IndexerStore::from_pool(ledger.pool().clone());
    let deployment = store
        .register_deployment(&NewDeployment {
            id: Uuid::new_v4(),
            chain_id: 84532,
            network: "base-sepolia".to_owned(),
            vault_address: vault.clone(),
            deployment_version: "v3".to_owned(),
            runtime_code_hash: code_hash,
            start_block,
        })
        .await
        .unwrap();
    let mut indexer = DepositIndexer::new(
        Config {
            database_url,
            deployment_id: deployment.id,
            primary_name: "local-anvil-a".to_owned(),
            primary_url,
            secondary_name: "local-anvil-b".to_owned(),
            secondary_url,
            request_timeout: Duration::from_secs(5),
            poll_interval: Duration::from_secs(2),
            retry_max: Duration::from_secs(30),
            max_head_age: Duration::from_secs(120),
            max_provider_block_lag: 3,
            batch_size: 500,
            audit_batch_size: 500,
            lease_duration: Duration::from_secs(30),
        },
        store,
        primary,
        secondary,
    )
    .await
    .unwrap();

    assert_eq!(indexer.tick().await.unwrap(), 2);
    assert_eq!(indexer.tick().await.unwrap(), 0);
    let customer_balance: BigDecimal = sqlx::query_scalar(
        "SELECT balance.balance FROM ledger_account_balances balance \
         JOIN ledger_accounts account ON account.id = balance.account_id \
         WHERE account.owner_reference = $1 AND account.account_type = 'customer_available' \
           AND account.asset_id = (SELECT id FROM ledger_assets WHERE code = 'ETH')",
    )
    .bind(eth_depositor.to_ascii_lowercase())
    .fetch_one(ledger.pool())
    .await
    .unwrap();
    let one_eth = BigDecimal::from(10_u64.pow(18));
    assert_eq!(customer_balance, one_eth);
    assert_eq!(ledger.balance(custody_id).await.unwrap(), -one_eth);
    let usdc_balance: BigDecimal = sqlx::query_scalar(
        "SELECT balance.balance FROM ledger_account_balances balance \
         JOIN ledger_accounts account ON account.id = balance.account_id \
         WHERE account.owner_reference = $1 AND account.account_type = 'customer_available' \
           AND account.asset_id = (SELECT id FROM ledger_assets WHERE code = 'USDC')",
    )
    .bind(usdc_depositor.to_ascii_lowercase())
    .fetch_one(ledger.pool())
    .await
    .unwrap();
    let received_usdc = BigDecimal::from(1_500_000_u64);
    assert_eq!(usdc_balance, received_usdc);
    assert_eq!(
        ledger.balance(usdc_custody_id).await.unwrap(),
        -received_usdc
    );
    let credited_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_deposits WHERE state = 'credited'")
            .fetch_one(ledger.pool())
            .await
            .unwrap();
    assert_eq!(credited_count, 2);
    let misplaced_credits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_deposits WHERE length(customer_reference) != 42",
    )
    .fetch_one(ledger.pool())
    .await
    .unwrap();
    assert_eq!(misplaced_credits, 0);

    // Replay the real chain credits into the matcher. The ETH depositor sells
    // half an ETH to the USDC depositor, and the ledger commits the fill before
    // either order is acknowledged.
    let maker = UserId::from_hex(&eth_depositor).unwrap();
    let taker = UserId::from_hex(&usdc_depositor).unwrap();
    let market = Market {
        id: MarketId("ETH-USDC".to_owned()),
        base: AssetId::from_str("ETH"),
        quote: AssetId::from_str("USDC"),
        max_orders: 1_000,
        min_order_size: 1,
        price_tick: 1,
        quantity_tick: 1,
        maker_fee_bps: -1,
        taker_fee_bps: 5,
    };
    let mut matching_engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    matching_engine.add_market(market);
    let replay =
        api::ledger_coordinator::restore_engine_financial_state(&mut matching_engine, &ledger)
            .await
            .unwrap();
    assert_eq!(replay.customer_balances, 2);
    let user_state = Arc::new(RwLock::new(UserState::new(5.0)));
    user_state.write().await.sync_from_engine(&matching_engine);
    let mut builder = MarketShards::new(user_state);
    builder.add_shard(MarketId("ETH-USDC".to_owned()), matching_engine);
    let shards = Arc::new(builder);
    let ledger = Arc::new(ledger);
    let leadership = ledger
        .acquire_engine_leadership(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    let committer: Arc<dyn FinancialCommitter> =
        Arc::new(api::ledger_coordinator::LedgerFinancialCommitter::new(
            Arc::clone(&ledger),
            leadership.token,
        ));
    for (owner, side, nonce) in [
        (maker.clone(), OrderSide::Ask, 1),
        (taker.clone(), OrderSide::Bid, 1),
    ] {
        let (responder, receiver) = tokio::sync::oneshot::channel();
        MarketShards::dispatch_sharded_batch_with_committer(
            Arc::clone(&shards),
            vec![BatchedRequest {
                request: Request::PostOrder(PostOrderRequest {
                    user: owner,
                    market: MarketId("ETH-USDC".to_owned()),
                    side,
                    order_type: OrderType::GoodTillCanceled,
                    price: PRICE_SCALE,
                    quantity: QUANTITY_SCALE / 2,
                    nonce,
                    client_order_id: None,
                    signature: vec![],
                    stp: Default::default(),
                    min_quantity: None,
                    display_quantity: None,
                }),
                ts: nonce,
                responder,
                decryption_proof: None,
            }],
            std::time::Instant::now(),
            BatchMetrics::new(),
            Some(Arc::clone(&committer)),
        )
        .await;
        let responses = receiver.await.unwrap();
        assert!(
            responses
                .iter()
                .all(|response| !matches!(response, Response::Error(_))),
            "{responses:?}"
        );
    }
    let account_balance =
        |asset: &str, owner_type: &str, owner_reference: &str, account_type: &str| {
            let ledger = Arc::clone(&ledger);
            let lookup = AccountLookup {
                asset_code: asset.to_owned(),
                owner_type: owner_type.to_owned(),
                owner_reference: owner_reference.to_owned(),
                account_type: account_type.to_owned(),
            };
            async move {
                let accounts = ledger
                    .resolve_accounts(std::slice::from_ref(&lookup))
                    .await
                    .unwrap();
                ledger.balance(accounts[&lookup].id).await.unwrap()
            }
        };
    let maker_ref = maker.to_hex().to_ascii_lowercase();
    let taker_ref = taker.to_hex().to_ascii_lowercase();
    assert_eq!(
        account_balance("ETH", "customer", &maker_ref, "customer_available").await,
        BigDecimal::from(500_000_000_000_000_000_u64)
    );
    assert_eq!(
        account_balance("ETH", "customer", &taker_ref, "customer_available").await,
        BigDecimal::from(500_000_000_000_000_000_u64)
    );
    assert_eq!(
        account_balance("USDC", "customer", &maker_ref, "customer_available").await,
        BigDecimal::from(500_050_u64)
    );
    assert_eq!(
        account_balance("USDC", "customer", &taker_ref, "customer_available").await,
        BigDecimal::from(999_750_u64)
    );
    assert_eq!(
        account_balance("USDC", "platform", "platform:vela", "fee_revenue").await,
        BigDecimal::from(250_u64)
    );
    assert_eq!(
        account_balance("USDC", "platform", "platform:vela", "rebate_expense").await,
        BigDecimal::from(-50)
    );
    let trade_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'trade'",
    )
    .fetch_one(ledger.pool())
    .await
    .unwrap();
    assert_eq!(trade_count, 1);
    assert!(ledger.verify_balance_projection().await.unwrap());
}

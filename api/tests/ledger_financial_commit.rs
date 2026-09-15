use api::ledger_coordinator::LedgerFinancialCommitter;
use bigdecimal::BigDecimal;
use engine::{
    BatchMetrics, BatchedRequest, FinancialCommitter, MarketShards, MatchingEngine, UserState,
};
use ledger::{
    AccountLookup, AccountProvisionRequest, JournalLegInput, Ledger, NewAccount, NewAsset,
    PostTransaction,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use types::{
    AssetId, DepositRequest, FeeConfig, Market, MarketId, OrderSide, OrderType, PostOrderRequest,
    Request, Response, UserId, PRICE_SCALE, QUANTITY_SCALE,
};
use uuid::Uuid;

const MARKET: &str = "ETH-USDC";

fn user(last_byte: u8) -> UserId {
    let mut bytes = [0_u8; 20];
    bytes[19] = last_byte;
    UserId(bytes)
}

fn market() -> Market {
    Market {
        id: MarketId(MARKET.to_owned()),
        base: AssetId::from_str("ETH"),
        quote: AssetId::from_str("USDC"),
        max_orders: 1_000,
        min_order_size: 1,
        price_tick: 1,
        quantity_tick: 1,
        maker_fee_bps: -1,
        taker_fee_bps: 5,
    }
}

async fn deposit(state: &Arc<RwLock<UserState>>, owner: UserId, asset: &str, amount: u64) {
    state.write().await.phase1_deposit(&DepositRequest {
        user: owner,
        asset: AssetId::from_str(asset),
        amount,
        l1_tx_hash: [0_u8; 32],
        source_chain: Default::default(),
        bridge_id: None,
    });
}

async fn submit(
    shards: Arc<MarketShards>,
    committer: Arc<dyn FinancialCommitter>,
    request: PostOrderRequest,
    timestamp: u64,
) -> Vec<Response> {
    let (responder, receiver) = tokio::sync::oneshot::channel();
    MarketShards::dispatch_sharded_batch_with_committer(
        shards,
        vec![BatchedRequest {
            request: Request::PostOrder(request),
            ts: timestamp,
            responder,
            decryption_proof: None,
        }],
        std::time::Instant::now(),
        BatchMetrics::new(),
        Some(committer),
    )
    .await;
    receiver.await.expect("dispatcher should respond")
}

async fn leader_committer(ledger: Arc<Ledger>) -> Arc<dyn FinancialCommitter> {
    let leadership = ledger
        .acquire_engine_leadership(Uuid::new_v4(), std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    Arc::new(LedgerFinancialCommitter::new(ledger, leadership.token))
}

struct LedgerFixture {
    ledger: Arc<Ledger>,
    accounts: HashMap<String, Uuid>,
}

impl LedgerFixture {
    async fn new(pool: sqlx::PgPool) -> Self {
        let ledger = Arc::new(Ledger::from_pool(pool));
        let mut assets = HashMap::new();
        for (code, decimals) in [("ETH", 18_i16), ("USDC", 6_i16)] {
            let id = Uuid::new_v4();
            ledger
                .create_asset(&NewAsset {
                    id,
                    code: code.to_owned(),
                    decimals,
                    chain_id: None,
                    contract_address: None,
                })
                .await
                .unwrap();
            assets.insert(code, id);
        }

        let mut accounts = HashMap::new();
        for (code, asset_id) in &assets {
            Self::create_account(
                &ledger,
                &mut accounts,
                code,
                *asset_id,
                "platform",
                "platform:operations",
                "operational_adjustment",
                true,
            )
            .await;
        }
        Self { ledger, accounts }
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_account(
        ledger: &Ledger,
        accounts: &mut HashMap<String, Uuid>,
        asset: &str,
        asset_id: Uuid,
        owner_type: &str,
        owner_reference: &str,
        account_type: &str,
        allow_negative: bool,
    ) {
        let id = Uuid::new_v4();
        ledger
            .create_account(&NewAccount {
                id,
                asset_id,
                owner_type: owner_type.to_owned(),
                owner_reference: owner_reference.to_owned(),
                account_type: account_type.to_owned(),
                allow_negative,
            })
            .await
            .unwrap();
        accounts.insert(Self::key(asset, owner_reference, account_type), id);
    }

    fn key(asset: &str, owner_reference: &str, account_type: &str) -> String {
        format!("{asset}:{owner_reference}:{account_type}")
    }

    fn account(&self, asset: &str, owner_reference: &str, account_type: &str) -> Uuid {
        self.accounts[&Self::key(asset, owner_reference, account_type)]
    }

    async fn seed(&self, asset: &str, customer: &UserId, amount: i128) {
        let reference = customer.to_hex().to_ascii_lowercase();
        let lookup = AccountLookup {
            asset_code: asset.to_owned(),
            owner_type: "customer".to_owned(),
            owner_reference: reference.clone(),
            account_type: "customer_available".to_owned(),
        };
        let provisioned = self
            .ledger
            .ensure_accounts(&[AccountProvisionRequest {
                asset_code: asset.to_owned(),
                owner_type: "customer".to_owned(),
                owner_reference: reference.clone(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            }])
            .await
            .unwrap();
        self.ledger
            .post(&PostTransaction {
                id: Uuid::new_v4(),
                operation_type: "deposit_credit".to_owned(),
                idempotency_key: format!("seed:{asset}:{reference}"),
                request_hash: [asset.as_bytes()[0]; 32],
                external_reference: None,
                metadata: json!({"test_fixture": true}),
                legs: vec![
                    JournalLegInput {
                        account_id: self.account(
                            asset,
                            "platform:operations",
                            "operational_adjustment",
                        ),
                        amount: BigDecimal::from(-amount),
                    },
                    JournalLegInput {
                        account_id: provisioned[&lookup].id,
                        amount: BigDecimal::from(amount),
                    },
                ],
            })
            .await
            .unwrap();
    }

    async fn balance(&self, asset: &str, owner_reference: &str, account_type: &str) -> BigDecimal {
        let lookup = AccountLookup {
            asset_code: asset.to_owned(),
            owner_type: if owner_reference.starts_with("platform:") {
                "platform".to_owned()
            } else {
                "customer".to_owned()
            },
            owner_reference: owner_reference.to_owned(),
            account_type: account_type.to_owned(),
        };
        let resolved = self
            .ledger
            .resolve_accounts(std::slice::from_ref(&lookup))
            .await
            .unwrap();
        self.ledger.balance(resolved[&lookup].id).await.unwrap()
    }
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn matcher_commits_trade_fees_and_rebate_before_ack(pool: sqlx::PgPool) {
    let maker = user(1);
    let taker = user(2);
    let fixture = LedgerFixture::new(pool.clone()).await;
    fixture.seed("ETH", &maker, 2_000_000_000_000_000_000).await;
    fixture.seed("USDC", &taker, 10_000_000_000).await;

    let user_state = Arc::new(RwLock::new(UserState::new(5.0)));
    user_state.write().await.add_market(market());
    deposit(&user_state, maker.clone(), "ETH", 2 * QUANTITY_SCALE).await;
    deposit(&user_state, taker.clone(), "USDC", 10_000 * PRICE_SCALE).await;

    let mut shard_builder = MarketShards::new(Arc::clone(&user_state));
    let mut matching_engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    matching_engine.add_market(market());
    shard_builder.add_shard(MarketId(MARKET.to_owned()), matching_engine);
    let shards = Arc::new(shard_builder);
    let committer = leader_committer(Arc::clone(&fixture.ledger)).await;

    let maker_responses = submit(
        Arc::clone(&shards),
        Arc::clone(&committer),
        PostOrderRequest {
            user: maker.clone(),
            market: MarketId(MARKET.to_owned()),
            side: OrderSide::Ask,
            order_type: OrderType::GoodTillCanceled,
            price: 2_000 * PRICE_SCALE,
            quantity: QUANTITY_SCALE,
            nonce: 1,
            client_order_id: Some("maker-1".to_owned()),
            signature: vec![],
            stp: Default::default(),
            min_quantity: None,
            display_quantity: None,
        },
        1,
    )
    .await;
    assert!(maker_responses
        .iter()
        .any(|response| matches!(response, Response::OrderPosted(_))));

    let taker_responses = submit(
        Arc::clone(&shards),
        Arc::clone(&committer),
        PostOrderRequest {
            user: taker.clone(),
            market: MarketId(MARKET.to_owned()),
            side: OrderSide::Bid,
            order_type: OrderType::GoodTillCanceled,
            price: 2_000 * PRICE_SCALE,
            quantity: QUANTITY_SCALE,
            nonce: 1,
            client_order_id: Some("taker-1".to_owned()),
            signature: vec![],
            stp: Default::default(),
            min_quantity: None,
            display_quantity: None,
        },
        2,
    )
    .await;
    assert!(taker_responses
        .iter()
        .any(|response| matches!(response, Response::OrderFilled(_))));

    let maker_reference = maker.to_hex().to_ascii_lowercase();
    let taker_reference = taker.to_hex().to_ascii_lowercase();
    assert_eq!(
        fixture
            .balance("ETH", &maker_reference, "customer_available")
            .await,
        BigDecimal::from(1_000_000_000_000_000_000_i128)
    );
    assert_eq!(
        fixture
            .balance("ETH", &maker_reference, "customer_locked")
            .await,
        BigDecimal::from(0)
    );
    assert_eq!(
        fixture
            .balance("USDC", &maker_reference, "customer_available")
            .await,
        BigDecimal::from(2_000_200_000_i64)
    );
    assert_eq!(
        fixture
            .balance("USDC", &taker_reference, "customer_available")
            .await,
        BigDecimal::from(7_999_000_000_i64)
    );
    assert_eq!(
        fixture
            .balance("ETH", &taker_reference, "customer_available")
            .await,
        BigDecimal::from(1_000_000_000_000_000_000_i128)
    );
    assert_eq!(
        fixture
            .balance("USDC", "platform:vela", "fee_revenue")
            .await,
        BigDecimal::from(1_000_000_i64)
    );
    assert_eq!(
        fixture
            .balance("USDC", "platform:vela", "rebate_expense")
            .await,
        BigDecimal::from(-200_000_i64)
    );

    let trade_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'trade'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(trade_count, 1);
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());

    let provisioned_account_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_accounts WHERE owner_reference <> 'platform:operations'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(provisioned_account_count, 7);

    let mut restarted_engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    restarted_engine.add_market(market());
    restarted_engine.process(
        Request::Deposit(DepositRequest {
            user: maker.clone(),
            asset: AssetId::from_str("ETH"),
            amount: 99,
            l1_tx_hash: [9_u8; 32],
            source_chain: Default::default(),
            bridge_id: None,
        }),
        3,
    );
    let stats = api::ledger_coordinator::restore_engine_financial_state(
        &mut restarted_engine,
        &fixture.ledger,
    )
    .await
    .unwrap();
    assert_eq!(stats.customer_balances, 4);
    assert_eq!(
        restarted_engine
            .balances
            .get(&(maker.clone(), AssetId::from_str("ETH")))
            .unwrap()
            .available,
        QUANTITY_SCALE
    );
    assert_eq!(
        restarted_engine
            .balances
            .get(&(taker.clone(), AssetId::from_str("USDC")))
            .unwrap()
            .available,
        7_999 * PRICE_SCALE
    );
    assert_eq!(restarted_engine.fee_balances.get("USDC"), Some(&80_000_000));
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn missing_financial_account_rejects_order_without_state_change(pool: sqlx::PgPool) {
    let maker = user(3);
    let ledger = Arc::new(Ledger::from_pool(pool));
    let user_state = Arc::new(RwLock::new(UserState::new(5.0)));
    user_state.write().await.add_market(market());
    deposit(&user_state, maker.clone(), "ETH", 2 * QUANTITY_SCALE).await;
    let before = user_state.read().await.balances.clone();

    let mut shard_builder = MarketShards::new(Arc::clone(&user_state));
    let mut matching_engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    matching_engine.add_market(market());
    shard_builder.add_shard(MarketId(MARKET.to_owned()), matching_engine);
    let shards = Arc::new(shard_builder);
    let committer = leader_committer(ledger).await;

    let responses = submit(
        Arc::clone(&shards),
        committer,
        PostOrderRequest {
            user: maker,
            market: MarketId(MARKET.to_owned()),
            side: OrderSide::Ask,
            order_type: OrderType::GoodTillCanceled,
            price: 2_000 * PRICE_SCALE,
            quantity: QUANTITY_SCALE,
            nonce: 1,
            client_order_id: None,
            signature: vec![],
            stp: Default::default(),
            min_quantity: None,
            display_quantity: None,
        },
        1,
    )
    .await;

    assert!(responses.iter().any(|response| matches!(
        response,
        Response::Error(error) if error.code == types::ErrorCode::InternalError
    )));
    assert_eq!(user_state.read().await.balances, before);
    let shard = shards.shards.get(&MarketId(MARKET.to_owned())).unwrap();
    assert!(shard
        .lock()
        .await
        .engine
        .order_books
        .get(&MarketId(MARKET.to_owned()))
        .unwrap()
        .all_orders()
        .is_empty());
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn customer_reads_use_ledger_projection_instead_of_engine_cache(pool: sqlx::PgPool) {
    let customer = user(4);
    let fixture = LedgerFixture::new(pool).await;
    fixture
        .seed("ETH", &customer, 1_000_000_000_000_000_000)
        .await;
    fixture.seed("USDC", &customer, 2_500_000).await;

    let balances = api::ledger_coordinator::load_customer_balances(&fixture.ledger, &customer)
        .await
        .unwrap();

    assert_eq!(balances.len(), 2);
    assert_eq!(balances[0].asset, AssetId::from_str("ETH"));
    assert_eq!(balances[0].available, QUANTITY_SCALE);
    assert_eq!(balances[0].locked, 0);
    assert_eq!(balances[1].asset, AssetId::from_str("USDC"));
    assert_eq!(balances[1].available, 250_000_000);
    assert_eq!(balances[1].locked, 0);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn external_credit_cannot_mutate_a_ledger_backed_engine_projection(pool: sqlx::PgPool) {
    let customer = user(5);
    let fixture = LedgerFixture::new(pool.clone()).await;
    let user_state = Arc::new(RwLock::new(UserState::new(5.0)));
    let before = user_state.read().await.balances.clone();
    let shards = Arc::new(MarketShards::new(Arc::clone(&user_state)));
    let committer = leader_committer(Arc::clone(&fixture.ledger)).await;
    let (responder, receiver) = tokio::sync::oneshot::channel();

    MarketShards::dispatch_sharded_batch_with_committer(
        shards,
        vec![BatchedRequest {
            request: Request::Deposit(DepositRequest {
                user: customer,
                asset: AssetId::from_str("ETH"),
                amount: QUANTITY_SCALE,
                l1_tx_hash: [7_u8; 32],
                source_chain: Default::default(),
                bridge_id: None,
            }),
            ts: 1,
            responder,
            decryption_proof: None,
        }],
        std::time::Instant::now(),
        BatchMetrics::new(),
        Some(committer),
    )
    .await;

    let responses = receiver.await.unwrap();
    assert!(responses.iter().any(|response| matches!(
        response,
        Response::Error(error) if error.code == types::ErrorCode::InternalError
    )));
    assert_eq!(user_state.read().await.balances, before);
    let journal_count: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_journal_transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(journal_count, 0);
}

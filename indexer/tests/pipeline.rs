use async_trait::async_trait;
use bigdecimal::BigDecimal;
use chrono::Utc;
use indexer::{
    Config, Deployment, DepositIndexer, HaltReason, IndexerError, IndexerStore, NewDeployment, Rpc,
    RpcBlock, RpcLog, DEPOSIT_EVENT_TOPIC,
};
use ledger::{AccountLookup, AccountProvisionRequest, Ledger, NewAsset};
use reqwest::Url;
use sha3::{Digest, Keccak256};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const VAULT: &str = "0x1111111111111111111111111111111111111111";
const CUSTOMER: &str = "0x2222222222222222222222222222222222222222";
const ACCOUNT_ID: &str = "0x3333333333333333333333333333333333333333333333333333333333333333";
const USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";

#[derive(Clone)]
struct FakeRpc {
    name: String,
    chain_id: u64,
    latest: RpcBlock,
    finalized: RpcBlock,
    logs: Vec<RpcLog>,
    code: String,
    deposit_block_hash: String,
}

#[async_trait]
impl Rpc for FakeRpc {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chain_id(&self) -> Result<u64, IndexerError> {
        Ok(self.chain_id)
    }

    async fn block(&self, tag: &str) -> Result<RpcBlock, IndexerError> {
        if tag == "latest" {
            Ok(self.latest.clone())
        } else if tag == "0x6e" {
            Ok(RpcBlock {
                number: 110,
                hash: self.deposit_block_hash.clone(),
                timestamp: self.finalized.timestamp,
            })
        } else {
            Ok(self.finalized.clone())
        }
    }

    async fn logs(
        &self,
        _address: &str,
        _topic: &str,
        _from: u64,
        _to: u64,
    ) -> Result<Vec<RpcLog>, IndexerError> {
        Ok(self.logs.clone())
    }

    async fn code(&self, _address: &str, _block: u64) -> Result<String, IndexerError> {
        Ok(self.code.clone())
    }
}

struct Fixture {
    store: IndexerStore,
    deployment: Deployment,
    custody_account_id: Uuid,
    code: String,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let ledger = Ledger::from_pool(pool.clone());
        let asset_id = Uuid::new_v4();
        ledger
            .create_asset(&NewAsset {
                id: asset_id,
                code: "USDC".to_owned(),
                decimals: 6,
                chain_id: Some(8453),
                contract_address: Some(USDC.to_owned()),
            })
            .await
            .unwrap();
        let custody_lookup = AccountLookup {
            asset_code: "USDC".to_owned(),
            owner_type: "custody".to_owned(),
            owner_reference: VAULT.to_owned(),
            account_type: "custody_vault".to_owned(),
        };
        let custody_account_id = ledger
            .ensure_accounts(&[AccountProvisionRequest {
                asset_code: "USDC".to_owned(),
                owner_type: "custody".to_owned(),
                owner_reference: VAULT.to_owned(),
                account_type: "custody_vault".to_owned(),
                allow_negative: true,
            }])
            .await
            .unwrap()[&custody_lookup]
            .id;

        let code = "0x60006000".to_owned();
        let runtime_code_hash = format!(
            "0x{}",
            hex::encode(Keccak256::digest(hex::decode(&code[2..]).unwrap()))
        );
        let store = IndexerStore::from_pool(pool.clone());
        let deployment = store
            .register_deployment(&NewDeployment {
                id: Uuid::new_v4(),
                chain_id: 8453,
                network: "base".to_owned(),
                vault_address: VAULT.to_owned(),
                deployment_version: "v3".to_owned(),
                runtime_code_hash,
                start_block: 100,
            })
            .await
            .unwrap();
        Self {
            store,
            deployment,
            custody_account_id,
            code,
        }
    }

    fn config(&self) -> Config {
        Config {
            database_url: "postgres://unused".to_owned(),
            deployment_id: self.deployment.id,
            primary_name: "provider-a".to_owned(),
            primary_url: Url::parse("https://provider-a.example/rpc").unwrap(),
            secondary_name: "provider-b".to_owned(),
            secondary_url: Url::parse("https://provider-b.example/rpc").unwrap(),
            request_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_millis(10),
            retry_max: Duration::from_millis(20),
            max_head_age: Duration::from_secs(120),
            max_provider_block_lag: 3,
            batch_size: 500,
            audit_batch_size: 500,
            lease_duration: Duration::from_secs(30),
        }
    }

    fn rpc(&self, name: &str) -> FakeRpc {
        let now = u64::try_from(Utc::now().timestamp()).unwrap();
        FakeRpc {
            name: name.to_owned(),
            chain_id: 8453,
            latest: RpcBlock {
                number: 125,
                hash: hash(0xaa),
                timestamp: now,
            },
            finalized: RpcBlock {
                number: 120,
                hash: hash(0xbb),
                timestamp: now.saturating_sub(5),
            },
            logs: vec![deposit_log()],
            code: self.code.clone(),
            deposit_block_hash: hash(0x44),
        }
    }
}

fn hash(byte: u8) -> String {
    format!("0x{}", format!("{byte:02x}").repeat(32))
}

fn padded_address(address: &str) -> String {
    format!("0x{}{}", "00".repeat(12), &address[2..])
}

fn deposit_log() -> RpcLog {
    RpcLog {
        address: VAULT.to_owned(),
        topics: vec![
            DEPOSIT_EVENT_TOPIC.to_owned(),
            hash(0x33),
            padded_address(CUSTOMER),
            ACCOUNT_ID.to_owned(),
        ],
        data: format!("0x{}{:064x}", &padded_address(USDC)[2..], 1_500_000_u64),
        block_number: "0x6e".to_owned(),
        block_hash: hash(0x44),
        transaction_hash: hash(0x55),
        transaction_index: "0x2".to_owned(),
        log_index: "0x3".to_owned(),
        removed: false,
    }
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn agreed_finalized_event_is_credited_once_and_checkpointed(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let mut indexer = DepositIndexer::new(
        fixture.config(),
        fixture.store.clone(),
        fixture.rpc("provider-a"),
        fixture.rpc("provider-b"),
    )
    .await
    .unwrap();

    assert_eq!(indexer.tick().await.unwrap(), 1);
    assert_eq!(indexer.tick().await.unwrap(), 0);

    let state: String = sqlx::query_scalar("SELECT state FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "credited");
    let customer_balance: BigDecimal = sqlx::query_scalar(
        r#"
        SELECT balance.balance
        FROM ledger_account_balances balance
        JOIN ledger_accounts account ON account.id = balance.account_id
        WHERE account.owner_reference = $1 AND account.account_type = 'customer_available'
        "#,
    )
    .bind(ACCOUNT_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(customer_balance, BigDecimal::from(1_500_000_u64));
    assert_eq!(
        Ledger::from_pool(pool.clone())
            .balance(fixture.custody_account_id)
            .await
            .unwrap(),
        BigDecimal::from(-1_500_000_i64)
    );
    let next_block: i64 = sqlx::query_scalar("SELECT next_block FROM ledger_indexer_checkpoints")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(next_block, 121);
    let journal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'deposit_credit'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journal_count, 1);
    let audit_next: i64 =
        sqlx::query_scalar("SELECT next_block FROM ledger_indexer_audit_checkpoints")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audit_next, 121);
    assert!(
        sqlx::query("UPDATE ledger_indexer_audit_checkpoints SET next_block = 100")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn provider_log_disagreement_halts_before_observation_or_checkpoint(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let first = fixture.rpc("provider-a");
    let mut second = fixture.rpc("provider-b");
    second.logs.clear();
    let mut indexer = DepositIndexer::new(fixture.config(), fixture.store, first, second)
        .await
        .unwrap();

    let error = indexer.tick().await.unwrap_err();
    assert!(matches!(
        error,
        IndexerError::Safety {
            reason: HaltReason::ProviderDisagreement,
            ..
        }
    ));
    let deposits: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deposits, 0);
    let next_block: i64 = sqlx::query_scalar("SELECT next_block FROM ledger_indexer_checkpoints")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(next_block, 100);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn stale_head_halts_before_credit(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let first = fixture.rpc("provider-a");
    let mut stale = fixture.rpc("provider-b");
    stale.latest.timestamp = stale.latest.timestamp.saturating_sub(1_000);
    let mut indexer = DepositIndexer::new(fixture.config(), fixture.store.clone(), first, stale)
        .await
        .unwrap();
    assert!(matches!(
        indexer.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::ProviderStale,
            ..
        })
    ));

    let deposits: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deposits, 0);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn wrong_bytecode_halts_before_credit(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let first = fixture.rpc("provider-a");
    let mut wrong_code = fixture.rpc("provider-b");
    wrong_code.code = "0x6001".to_owned();
    let mut indexer = DepositIndexer::new(fixture.config(), fixture.store, first, wrong_code)
        .await
        .unwrap();
    assert!(matches!(
        indexer.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::VaultCodeMismatch,
            ..
        })
    ));
    let deposits: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deposits, 0);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn second_worker_cannot_take_live_lease_or_rewind_checkpoint(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let mut first_rpc = fixture.rpc("provider-a");
    first_rpc.logs.clear();
    let mut second_rpc = fixture.rpc("provider-b");
    second_rpc.logs.clear();
    let mut first = DepositIndexer::new(
        fixture.config(),
        fixture.store.clone(),
        first_rpc.clone(),
        second_rpc.clone(),
    )
    .await
    .unwrap();
    assert_eq!(first.tick().await.unwrap(), 0);

    let mut second = DepositIndexer::new(fixture.config(), fixture.store, first_rpc, second_rpc)
        .await
        .unwrap();
    assert!(matches!(
        second.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::LeaseLost,
            ..
        })
    ));
    assert!(
        sqlx::query("UPDATE ledger_indexer_checkpoints SET next_block = 100")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE ledger_chain_deployments SET runtime_code_hash = $1")
            .bind(hash(0xee))
            .execute(&pool)
            .await
            .is_err()
    );
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn finalized_history_contradiction_halts_and_reverses_credit_to_pending(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let mut first = DepositIndexer::new(
        fixture.config(),
        fixture.store.clone(),
        fixture.rpc("provider-a"),
        fixture.rpc("provider-b"),
    )
    .await
    .unwrap();
    assert_eq!(first.tick().await.unwrap(), 1);

    sqlx::query(
        "UPDATE ledger_indexer_checkpoints SET lease_expires_at = transaction_timestamp() - interval '1 second'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut primary = fixture.rpc("provider-a");
    primary.logs.clear();
    primary.deposit_block_hash = hash(0x66);
    let mut secondary = fixture.rpc("provider-b");
    secondary.logs.clear();
    secondary.deposit_block_hash = hash(0x66);
    let mut recovery =
        DepositIndexer::new(fixture.config(), fixture.store.clone(), primary, secondary)
            .await
            .unwrap();

    assert!(matches!(
        recovery.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::CreditedDepositContradiction,
            ..
        })
    ));
    let state: String = sqlx::query_scalar("SELECT state FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "reversed");
    let customer_available: BigDecimal = sqlx::query_scalar(
        r#"
        SELECT balance.balance
        FROM ledger_account_balances balance
        JOIN ledger_accounts account ON account.id = balance.account_id
        WHERE account.owner_reference = $1 AND account.account_type = 'customer_available'
        "#,
    )
    .bind(ACCOUNT_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    let customer_pending: BigDecimal = sqlx::query_scalar(
        r#"
        SELECT balance.balance
        FROM ledger_account_balances balance
        JOIN ledger_accounts account ON account.id = balance.account_id
        WHERE account.owner_reference = $1 AND account.account_type = 'customer_pending'
        "#,
    )
    .bind(ACCOUNT_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(customer_available, BigDecimal::from(0));
    assert_eq!(customer_pending, BigDecimal::from(1_500_000_i64));
    let active_halts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_asset_halts WHERE resolved_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active_halts, 1);
    let audit_next: i64 =
        sqlx::query_scalar("SELECT next_block FROM ledger_indexer_audit_checkpoints")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audit_next, 100);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn audit_provider_disagreement_does_not_reverse_credited_deposit(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let mut first = DepositIndexer::new(
        fixture.config(),
        fixture.store.clone(),
        fixture.rpc("provider-a"),
        fixture.rpc("provider-b"),
    )
    .await
    .unwrap();
    assert_eq!(first.tick().await.unwrap(), 1);
    sqlx::query(
        "UPDATE ledger_indexer_checkpoints SET lease_expires_at = transaction_timestamp() - interval '1 second'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut primary = fixture.rpc("provider-a");
    primary.logs.clear();
    let secondary = fixture.rpc("provider-b");
    let mut recovery = DepositIndexer::new(fixture.config(), fixture.store, primary, secondary)
        .await
        .unwrap();
    assert!(matches!(
        recovery.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::ProviderDisagreement,
            ..
        })
    ));
    let state: String = sqlx::query_scalar("SELECT state FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "credited");
    let active_halts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_asset_halts WHERE resolved_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active_halts, 0);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn newly_appeared_event_behind_forward_checkpoint_halts_asset(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let mut primary = fixture.rpc("provider-a");
    primary.logs.clear();
    let mut secondary = fixture.rpc("provider-b");
    secondary.logs.clear();
    let mut first =
        DepositIndexer::new(fixture.config(), fixture.store.clone(), primary, secondary)
            .await
            .unwrap();
    assert_eq!(first.tick().await.unwrap(), 0);
    sqlx::query(
        "UPDATE ledger_indexer_checkpoints SET lease_expires_at = transaction_timestamp() - interval '1 second'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut audit = DepositIndexer::new(
        fixture.config(),
        fixture.store.clone(),
        fixture.rpc("provider-a"),
        fixture.rpc("provider-b"),
    )
    .await
    .unwrap();
    assert!(matches!(
        audit.tick().await,
        Err(IndexerError::Safety {
            reason: HaltReason::HistoricalDepositAppeared,
            ..
        })
    ));
    let deposits: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_deposits")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deposits, 0);
    let active_halts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_asset_halts WHERE resolved_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active_halts, 1);
    let next_block: i64 = sqlx::query_scalar("SELECT next_block FROM ledger_indexer_checkpoints")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(next_block, 121);
}

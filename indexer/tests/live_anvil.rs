//! Opt-in local chain integration proof. The two loopback URLs deliberately
//! share one Anvil process; this tests chain-to-ledger wiring, not independent
//! provider operation.

use bigdecimal::BigDecimal;
use indexer::{Config, DepositIndexer, HttpRpc, IndexerStore, NewDeployment, Rpc};
use ledger::{AccountLookup, AccountProvisionRequest, Ledger, NewAsset};
use reqwest::Url;
use sha3::{Digest, Keccak256};
use std::{env, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn local_eth_deposit_reaches_the_authoritative_ledger() {
    let Ok(rpc_url) = env::var("ANVIL_RPC_URL") else {
        return;
    };
    let vault = env::var("VELA_LOCAL_VAULT").expect("local vault address");
    let account_id = env::var("VELA_LOCAL_ACCOUNT_ID").expect("local account ID");
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

    assert_eq!(indexer.tick().await.unwrap(), 1);
    assert_eq!(indexer.tick().await.unwrap(), 0);
    let customer_balance: BigDecimal = sqlx::query_scalar(
        "SELECT balance.balance FROM ledger_account_balances balance \
         JOIN ledger_accounts account ON account.id = balance.account_id \
         WHERE account.owner_reference = $1 AND account.account_type = 'customer_available'",
    )
    .bind(account_id.to_ascii_lowercase())
    .fetch_one(ledger.pool())
    .await
    .unwrap();
    let one_eth = BigDecimal::from(10_u64.pow(18));
    assert_eq!(customer_balance, one_eth);
    assert_eq!(ledger.balance(custody_id).await.unwrap(), -one_eth);
    let credited_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_deposits WHERE state = 'credited'")
            .fetch_one(ledger.pool())
            .await
            .unwrap();
    assert_eq!(credited_count, 1);
}

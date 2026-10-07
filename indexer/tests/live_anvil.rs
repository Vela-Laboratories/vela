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
}

use indexer::{IndexerStore, NewDeployment};
use ledger::Ledger;
use std::{env, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = required("DATABASE_MIGRATOR_URL")?;
    let deployment = NewDeployment {
        id: required("VELA_INDEXER_DEPLOYMENT_ID")?
            .parse()
            .map_err(|_| "VELA_INDEXER_DEPLOYMENT_ID must be a UUID")?,
        chain_id: required("VELA_CHAIN_ID")?
            .parse()
            .map_err(|_| "VELA_CHAIN_ID must be an integer")?,
        network: required("VELA_INDEXER_NETWORK")?,
        vault_address: required("VELA_CUSTODY_VAULT_ADDRESS")?,
        deployment_version: required("VELA_CUSTODY_VAULT_DEPLOYMENT_VERSION")?,
        runtime_code_hash: required("VELA_CUSTODY_VAULT_CODE_HASH")?,
        start_block: required("VELA_CUSTODY_VAULT_START_BLOCK")?
            .parse()
            .map_err(|_| "VELA_CUSTODY_VAULT_START_BLOCK must be a nonnegative integer")?,
    };

    let ledger = Ledger::connect_with_options(
        &database_url,
        1,
        Duration::from_secs(10),
        Duration::from_secs(60),
        0,
    )
    .await?;
    ledger.ensure_schema_current().await?;
    let store = IndexerStore::connect(&database_url).await?;
    let registered = store.register_deployment(&deployment).await?;
    println!(
        "registered immutable {} deployment {} on chain {}",
        registered.network, registered.id, registered.chain_id
    );
    Ok(())
}

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} must be set").into())
}

use indexer::{DeploymentManifest, IndexerStore};
use ledger::Ledger;
use std::{env, fs, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = required("DATABASE_MIGRATOR_URL")?;
    let manifest_path = required("VELA_CUSTODY_DEPLOYMENT_MANIFEST_PATH")?;
    let manifest = DeploymentManifest::parse(&fs::read(manifest_path)?)?;
    let deployment = manifest.registration();

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

use indexer::{verify_deployment_rpc, DeploymentManifest, HttpRpc, IndexerStore};
use ledger::Ledger;
use reqwest::Url;
use std::{env, fs, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = required("DATABASE_MIGRATOR_URL")?;
    let manifest_path = required("VELA_CUSTODY_DEPLOYMENT_MANIFEST_PATH")?;
    let manifest = DeploymentManifest::parse(&fs::read(manifest_path)?)?;
    let deployment = manifest.registration();

    let primary_url = rpc_url("VELA_BASE_RPC_PRIMARY_URL")?;
    let secondary_url = rpc_url("VELA_BASE_RPC_SECONDARY_URL")?;
    if primary_url.origin() == secondary_url.origin() {
        return Err("registration RPC providers must have distinct origins".into());
    }
    let primary = HttpRpc::new(
        required("VELA_BASE_RPC_PRIMARY_NAME")?,
        primary_url,
        Duration::from_secs(5),
    )?;
    let secondary = HttpRpc::new(
        required("VELA_BASE_RPC_SECONDARY_NAME")?,
        secondary_url,
        Duration::from_secs(5),
    )?;
    verify_deployment_rpc(&manifest, &primary, &secondary).await?;

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

fn rpc_url(name: &str) -> Result<Url, Box<dyn std::error::Error>> {
    let value = required(name)?;
    let url = Url::parse(&value).map_err(|_| format!("{name} must be a valid URL"))?;
    let host = url.host_str().unwrap_or_default();
    let loopback = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1";
    if host.is_empty()
        || (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(format!("{name} must be HTTPS or loopback HTTP without URL userinfo").into());
    }
    Ok(url)
}

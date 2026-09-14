use indexer::{Config, DepositIndexer, HttpRpc, IndexerStore};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let config = Config::from_env()?;
    let store = IndexerStore::connect(&config.database_url).await?;
    let primary = HttpRpc::new(
        config.primary_name.clone(),
        config.primary_url.clone(),
        config.request_timeout,
    )?;
    let secondary = HttpRpc::new(
        config.secondary_name.clone(),
        config.secondary_url.clone(),
        config.request_timeout,
    )?;
    info!(deployment_id = %config.deployment_id, "starting fail-closed Base deposit indexer");
    DepositIndexer::new(config, store, primary, secondary)
        .await?
        .run()
        .await?;
    Ok(())
}

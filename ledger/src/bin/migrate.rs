use std::time::Duration;

use ledger::Ledger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_MIGRATOR_URL")
        .map_err(|_| "DATABASE_MIGRATOR_URL must be set for controlled ledger migrations")?;
    let ledger = Ledger::connect_with_options(
        &database_url,
        1,
        Duration::from_secs(10),
        Duration::from_secs(60),
        0,
    )
    .await?;

    ledger.migrate().await?;
    ledger.ensure_schema_current().await?;
    println!(
        "ledger migration complete at schema version {}",
        Ledger::EXPECTED_SCHEMA_VERSION
    );
    Ok(())
}

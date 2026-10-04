use chrono::{Days, NaiveDate, Utc};
use ledger::Ledger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let date = match std::env::args().nth(1) {
        Some(date) => NaiveDate::parse_from_str(&date, "%Y-%m-%d")?,
        None => Utc::now()
            .date_naive()
            .checked_sub_days(Days::new(1))
            .ok_or("invalid previous date")?,
    };
    let database_url = std::env::var("DATABASE_READER_URL")
        .map_err(|_| "DATABASE_READER_URL is required for the read-only daily report")?;
    let ledger = Ledger::connect(&database_url, 1).await?;
    ledger.ensure_schema_current().await?;
    let report = ledger.daily_reconciliation_report(date).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

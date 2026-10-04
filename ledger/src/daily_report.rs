//! Read-only UTC reports reconstructed from immutable journal and custody evidence.
use crate::{Ledger, LedgerError};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow)]
pub struct DailyOperationCount {
    pub operation_type: String,
    pub transaction_count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyAssetReport {
    pub asset: String,
    pub decimals: i16,
    pub closing_customer_liability_atomic: String,
    pub closing_platform_liability_atomic: String,
    pub closing_custody_journal_atomic: String,
    pub journal_net_atomic: String,
    pub journal_balanced: bool,
    pub daily_fee_revenue_atomic: String,
    pub daily_rebate_expense_atomic: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyCustodyEvidence {
    pub result_id: Uuid,
    pub snapshot_id: Uuid,
    pub deployment_id: Uuid,
    pub asset: String,
    pub status: String,
    pub custody_amount_atomic: String,
    pub liability_amount_atomic: String,
    pub difference_atomic: String,
    pub evidence_hash: String,
    pub block_number: i64,
    pub block_timestamp: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DailyReconciliationFailure {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub reason: String,
    pub evidence_hash: String,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DailyReconciliationReport {
    pub date_utc: NaiveDate,
    pub generated_at: DateTime<Utc>,
    pub cutoff_exclusive: DateTime<Utc>,
    pub complete_day: bool,
    pub source: &'static str,
    pub operations: Vec<DailyOperationCount>,
    pub assets: Vec<DailyAssetReport>,
    pub latest_custody_evidence: Vec<DailyCustodyEvidence>,
    pub failures_recorded_today: Vec<DailyReconciliationFailure>,
}

impl Ledger {
    pub async fn daily_reconciliation_report(
        &self,
        date: NaiveDate,
    ) -> Result<DailyReconciliationReport, LedgerError> {
        let generated_at = Utc::now();
        if date > generated_at.date_naive() {
            return Err(LedgerError::InvalidReportDate);
        }
        let start = date
            .and_hms_opt(0, 0, 0)
            .expect("midnight is valid")
            .and_utc();
        let end = date
            .succ_opt()
            .ok_or(LedgerError::InvalidReportDate)?
            .and_hms_opt(0, 0, 0)
            .expect("midnight is valid")
            .and_utc();
        let cutoff = end.min(generated_at);
        let mut tx = self.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let operations = sqlx::query_as::<_, DailyOperationCount>("SELECT operation_type, count(*) AS transaction_count FROM ledger_journal_transactions WHERE created_at >= $1 AND created_at < $2 GROUP BY operation_type ORDER BY operation_type")
            .bind(start).bind(cutoff).fetch_all(&mut *tx).await?;
        let assets = sqlx::query_as::<_, DailyAssetReport>(r#"
            WITH history AS (
                SELECT leg.*, account.owner_type, account.account_type
                FROM ledger_journal_legs leg
                JOIN ledger_journal_transactions journal ON journal.id=leg.transaction_id
                JOIN ledger_accounts account ON account.id=leg.account_id
                WHERE journal.created_at < $2
            )
            SELECT asset.code AS asset, asset.decimals,
                COALESCE(sum(h.amount) FILTER (WHERE h.owner_type='customer'),0)::text AS closing_customer_liability_atomic,
                COALESCE(sum(h.amount) FILTER (WHERE h.owner_type='platform'),0)::text AS closing_platform_liability_atomic,
                COALESCE(sum(h.amount) FILTER (WHERE h.owner_type='custody'),0)::text AS closing_custody_journal_atomic,
                COALESCE(sum(h.amount),0)::text AS journal_net_atomic,
                COALESCE(sum(h.amount),0)=0 AS journal_balanced,
                COALESCE(sum(h.amount) FILTER (WHERE h.account_type='fee_revenue' AND h.created_at >= $1),0)::text AS daily_fee_revenue_atomic,
                COALESCE(sum(h.amount) FILTER (WHERE h.account_type='rebate_expense' AND h.created_at >= $1),0)::text AS daily_rebate_expense_atomic
            FROM ledger_assets asset LEFT JOIN history h ON h.asset_id=asset.id
            GROUP BY asset.id, asset.code, asset.decimals ORDER BY asset.code
        "#).bind(start).bind(cutoff).fetch_all(&mut *tx).await?;
        let latest_custody_evidence = sqlx::query_as::<_, DailyCustodyEvidence>(r#"
            SELECT DISTINCT ON (snapshot.deployment_id, result.asset_id)
                result.id AS result_id, result.snapshot_id, snapshot.deployment_id, asset.code AS asset,
                result.status, result.custody_amount::text AS custody_amount_atomic,
                result.liability_amount::text AS liability_amount_atomic, result.difference::text AS difference_atomic,
                encode(result.evidence_hash,'hex') AS evidence_hash,
                snapshot.block_number, snapshot.block_timestamp, result.created_at AS recorded_at
            FROM ledger_reconciliation_results result
            JOIN ledger_custody_snapshots snapshot ON snapshot.id=result.snapshot_id
            JOIN ledger_assets asset ON asset.id=result.asset_id
            WHERE result.created_at < $1
            ORDER BY snapshot.deployment_id, result.asset_id, result.created_at DESC, result.id DESC
        "#).bind(cutoff).fetch_all(&mut *tx).await?;
        let failures_recorded_today = sqlx::query_as::<_, DailyReconciliationFailure>("SELECT id,deployment_id,reason,encode(evidence_hash,'hex') AS evidence_hash,detected_at FROM ledger_reconciliation_failures WHERE detected_at >= $1 AND detected_at < $2 ORDER BY detected_at,id")
            .bind(start).bind(cutoff).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(DailyReconciliationReport {
            date_utc: date,
            generated_at,
            cutoff_exclusive: cutoff,
            complete_day: end <= generated_at,
            source: "immutable_journal_and_custody_evidence",
            operations,
            assets,
            latest_custody_evidence,
            failures_recorded_today,
        })
    }
}

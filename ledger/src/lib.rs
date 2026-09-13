use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLegInput {
    pub account_id: Uuid,
    pub amount: BigDecimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostTransaction {
    pub id: Uuid,
    pub operation_type: String,
    pub idempotency_key: String,
    pub request_hash: [u8; 32],
    pub external_reference: Option<Value>,
    pub metadata: Value,
    pub legs: Vec<JournalLegInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdministrativeCorrectionReason {
    CustomerBalanceCorrection,
    DepositRecovery,
    WithdrawalRecovery,
    FeeCorrection,
    ReconciliationCorrection,
    IncidentRemediation,
}

impl AdministrativeCorrectionReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::CustomerBalanceCorrection => "customer_balance_correction",
            Self::DepositRecovery => "deposit_recovery",
            Self::WithdrawalRecovery => "withdrawal_recovery",
            Self::FeeCorrection => "fee_correction",
            Self::ReconciliationCorrection => "reconciliation_correction",
            Self::IncidentRemediation => "incident_remediation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdministrativeCorrection {
    pub correction_id: Uuid,
    pub transaction_id: Uuid,
    pub idempotency_key: String,
    pub reason_type: AdministrativeCorrectionReason,
    pub reason: String,
    pub evidence_reference: String,
    pub incident_id: String,
    pub initiated_by: String,
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
    pub legs: Vec<JournalLegInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostedAdministrativeCorrection {
    pub correction_id: Uuid,
    pub transaction: PostedTransaction,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct PostedTransaction {
    pub id: Uuid,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAsset {
    pub id: Uuid,
    pub code: String,
    pub decimals: i16,
    pub chain_id: Option<i64>,
    pub contract_address: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAccount {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub owner_type: String,
    pub owner_reference: String,
    pub account_type: String,
    pub allow_negative: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountLookup {
    pub asset_code: String,
    pub owner_type: String,
    pub owner_reference: String,
    pub account_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAccount {
    pub id: Uuid,
    pub asset_decimals: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountProvisionRequest {
    pub asset_code: String,
    pub owner_type: String,
    pub owner_reference: String,
    pub account_type: String,
    pub allow_negative: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayedAccountBalance {
    pub account_id: Uuid,
    pub asset_code: String,
    pub asset_decimals: i16,
    pub owner_type: String,
    pub owner_reference: String,
    pub account_type: String,
    pub balance: BigDecimal,
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("journal transaction must contain at least two legs")]
    TooFewLegs,
    #[error("journal legs must have nonzero integer amounts")]
    InvalidAmount,
    #[error("journal transaction contains a duplicate account leg: {0}")]
    DuplicateAccount(Uuid),
    #[error("unknown ledger account: {0}")]
    UnknownAccount(Uuid),
    #[error("journal transaction is not balanced for asset {0}")]
    Unbalanced(Uuid),
    #[error("insufficient funds in account {account_id}: balance {balance}, delta {delta}")]
    InsufficientFunds {
        account_id: Uuid,
        balance: BigDecimal,
        delta: BigDecimal,
    },
    #[error("idempotency key was already used with a different request")]
    IdempotencyConflict,
    #[error("administrative adjustments must use the dual-approval correction workflow")]
    AdministrativeCorrectionRequired,
    #[error("invalid administrative correction: {0}")]
    InvalidAdministrativeCorrection(String),
    #[error("administrative correction audit record is missing for transaction {0}")]
    AdministrativeCorrectionAuditMissing(Uuid),
    #[error("ledger schema version mismatch: expected {expected}, found {found}")]
    SchemaVersion { expected: i64, found: i64 },
    #[error(
        "ledger account is not provisioned for asset {asset_code}, owner {owner_reference}, type {account_type}"
    )]
    MissingAccount {
        asset_code: String,
        owner_reference: String,
        account_type: String,
    },
    #[error("ledger asset is not provisioned: {0}")]
    MissingAsset(String),
    #[error("existing ledger asset configuration does not match requested asset: {0}")]
    AssetConfigurationConflict(String),
    #[error("invalid ledger account provisioning request: {0}")]
    InvalidAccountProvision(String),
    #[error("journal replay referenced unknown account: {0}")]
    UnknownReplayAccount(Uuid),
    #[error(
        "journal replay differs from materialized projection for account {account_id}: replayed {replayed}, projected {projected}"
    )]
    ReplayProjectionMismatch {
        account_id: Uuid,
        replayed: BigDecimal,
        projected: BigDecimal,
    },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Debug, FromRow)]
struct LockedAccount {
    id: Uuid,
    asset_id: Uuid,
    allow_negative: bool,
    balance: BigDecimal,
}

#[derive(Debug, FromRow)]
struct ExistingTransaction {
    id: Uuid,
    idempotency_key: String,
    request_hash: Vec<u8>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ReplayAccountProjection {
    account_id: Uuid,
    asset_code: String,
    asset_decimals: i16,
    owner_type: String,
    owner_reference: String,
    account_type: String,
    projected_balance: BigDecimal,
}

#[derive(Debug, FromRow)]
struct ReplayLeg {
    account_id: Uuid,
    amount: BigDecimal,
}

#[derive(Clone)]
pub struct Ledger {
    pool: PgPool,
    max_serializable_retries: u32,
}

impl Ledger {
    pub const EXPECTED_SCHEMA_VERSION: i64 = 1;

    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            max_serializable_retries: 3,
        }
    }

    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, LedgerError> {
        Self::connect_with_options(
            database_url,
            max_connections,
            Duration::from_secs(5),
            Duration::from_secs(5),
            3,
        )
        .await
    }

    pub async fn connect_with_options(
        database_url: &str,
        max_connections: u32,
        acquire_timeout: Duration,
        statement_timeout: Duration,
        max_serializable_retries: u32,
    ) -> Result<Self, LedgerError> {
        let statement_timeout_ms = statement_timeout.as_millis().to_string();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(acquire_timeout)
            .idle_timeout(Some(Duration::from_secs(300)))
            .max_lifetime(Some(Duration::from_secs(1_800)))
            .after_connect(move |connection, _metadata| {
                let statement_timeout_ms = statement_timeout_ms.clone();
                Box::pin(async move {
                    sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                        .bind(statement_timeout_ms)
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;
        Ok(Self {
            pool,
            max_serializable_retries,
        })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), LedgerError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn health_check(&self) -> Result<(), LedgerError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        self.ensure_schema_current().await
    }

    pub async fn ensure_schema_current(&self) -> Result<(), LedgerError> {
        let migration_table_exists = sqlx::query_scalar::<_, bool>(
            "SELECT to_regclass('public._sqlx_migrations') IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;

        let found = if migration_table_exists {
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(max(version), 0) FROM _sqlx_migrations WHERE success",
            )
            .fetch_one(&self.pool)
            .await?
        } else {
            0
        };

        if found != Self::EXPECTED_SCHEMA_VERSION {
            return Err(LedgerError::SchemaVersion {
                expected: Self::EXPECTED_SCHEMA_VERSION,
                found,
            });
        }
        Ok(())
    }

    pub async fn create_asset(&self, asset: &NewAsset) -> Result<(), LedgerError> {
        sqlx::query(
            r#"
            INSERT INTO ledger_assets (id, code, decimals, chain_id, contract_address)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(asset.id)
        .bind(&asset.code)
        .bind(asset.decimals)
        .bind(asset.chain_id)
        .bind(&asset.contract_address)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn ensure_asset(&self, asset: &NewAsset) -> Result<(), LedgerError> {
        sqlx::query(
            r#"
            INSERT INTO ledger_assets (id, code, decimals, chain_id, contract_address)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (code) DO NOTHING
            "#,
        )
        .bind(asset.id)
        .bind(&asset.code)
        .bind(asset.decimals)
        .bind(asset.chain_id)
        .bind(&asset.contract_address)
        .execute(&self.pool)
        .await?;

        let existing = sqlx::query_as::<_, (i16, Option<i64>, Option<String>)>(
            "SELECT decimals, chain_id, contract_address FROM ledger_assets WHERE code = $1",
        )
        .bind(&asset.code)
        .fetch_one(&self.pool)
        .await?;
        let address_matches = match (&existing.2, &asset.contract_address) {
            (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
            (None, None) => true,
            _ => false,
        };
        if existing.0 != asset.decimals || existing.1 != asset.chain_id || !address_matches {
            return Err(LedgerError::AssetConfigurationConflict(asset.code.clone()));
        }
        Ok(())
    }

    pub async fn create_account(&self, account: &NewAccount) -> Result<(), LedgerError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO ledger_accounts
                (id, asset_id, owner_type, owner_reference, account_type, allow_negative)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(account.id)
        .bind(account.asset_id)
        .bind(&account.owner_type)
        .bind(&account.owner_reference)
        .bind(&account.account_type)
        .bind(account.allow_negative)
        .execute(&mut *tx)
        .await?;

        sqlx::query("INSERT INTO ledger_account_balances (account_id) VALUES ($1)")
            .bind(account.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn resolve_accounts(
        &self,
        lookups: &[AccountLookup],
    ) -> Result<HashMap<AccountLookup, ResolvedAccount>, LedgerError> {
        let mut resolved = HashMap::with_capacity(lookups.len());
        for lookup in lookups {
            if resolved.contains_key(lookup) {
                continue;
            }
            let account = sqlx::query_as::<_, (Uuid, i16)>(
                r#"
                SELECT account.id, asset.decimals
                FROM ledger_accounts account
                JOIN ledger_assets asset ON asset.id = account.asset_id
                WHERE asset.code = $1
                  AND account.owner_type = $2
                  AND account.owner_reference = $3
                  AND account.account_type = $4
                "#,
            )
            .bind(&lookup.asset_code)
            .bind(&lookup.owner_type)
            .bind(&lookup.owner_reference)
            .bind(&lookup.account_type)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| LedgerError::MissingAccount {
                asset_code: lookup.asset_code.clone(),
                owner_reference: lookup.owner_reference.clone(),
                account_type: lookup.account_type.clone(),
            })?;
            resolved.insert(
                lookup.clone(),
                ResolvedAccount {
                    id: account.0,
                    asset_decimals: account.1,
                },
            );
        }
        Ok(resolved)
    }

    pub async fn ensure_accounts(
        &self,
        requests: &[AccountProvisionRequest],
    ) -> Result<HashMap<AccountLookup, ResolvedAccount>, LedgerError> {
        let mut tx = self.pool.begin().await?;
        let mut resolved = HashMap::with_capacity(requests.len());

        for request in requests {
            validate_account_provision(request)?;
            let lookup = AccountLookup {
                asset_code: request.asset_code.clone(),
                owner_type: request.owner_type.clone(),
                owner_reference: request.owner_reference.clone(),
                account_type: request.account_type.clone(),
            };
            if resolved.contains_key(&lookup) {
                continue;
            }

            let asset = sqlx::query_as::<_, (Uuid, i16)>(
                "SELECT id, decimals FROM ledger_assets WHERE code = $1",
            )
            .bind(&request.asset_code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| LedgerError::MissingAsset(request.asset_code.clone()))?;

            let account_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO ledger_accounts
                    (id, asset_id, owner_type, owner_reference, account_type, allow_negative)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (asset_id, owner_type, owner_reference, account_type) DO NOTHING
                "#,
            )
            .bind(account_id)
            .bind(asset.0)
            .bind(&request.owner_type)
            .bind(&request.owner_reference)
            .bind(&request.account_type)
            .bind(request.allow_negative)
            .execute(&mut *tx)
            .await?;

            let existing = sqlx::query_as::<_, (Uuid, bool)>(
                r#"
                SELECT id, allow_negative
                FROM ledger_accounts
                WHERE asset_id = $1
                  AND owner_type = $2
                  AND owner_reference = $3
                  AND account_type = $4
                "#,
            )
            .bind(asset.0)
            .bind(&request.owner_type)
            .bind(&request.owner_reference)
            .bind(&request.account_type)
            .fetch_one(&mut *tx)
            .await?;
            if existing.1 != request.allow_negative {
                return Err(LedgerError::InvalidAccountProvision(format!(
                    "allow_negative policy mismatch for {} {}",
                    request.asset_code, request.account_type
                )));
            }

            sqlx::query(
                "INSERT INTO ledger_account_balances (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
            )
            .bind(existing.0)
            .execute(&mut *tx)
            .await?;
            resolved.insert(
                lookup,
                ResolvedAccount {
                    id: existing.0,
                    asset_decimals: asset.1,
                },
            );
        }

        tx.commit().await?;
        Ok(resolved)
    }

    pub async fn replay_account_balances(
        &self,
    ) -> Result<Vec<ReplayedAccountBalance>, LedgerError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;

        let accounts = sqlx::query_as::<_, ReplayAccountProjection>(
            r#"
            SELECT
                account.id AS account_id,
                asset.code AS asset_code,
                asset.decimals AS asset_decimals,
                account.owner_type,
                account.owner_reference,
                account.account_type,
                balance.balance AS projected_balance
            FROM ledger_accounts account
            JOIN ledger_assets asset ON asset.id = account.asset_id
            JOIN ledger_account_balances balance ON balance.account_id = account.id
            ORDER BY asset.code, account.owner_type, account.owner_reference, account.account_type
            "#,
        )
        .fetch_all(&mut *tx)
        .await?;
        let legs = sqlx::query_as::<_, ReplayLeg>(
            "SELECT account_id, amount FROM ledger_journal_legs ORDER BY id",
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut replayed: HashMap<Uuid, BigDecimal> = accounts
            .iter()
            .map(|account| (account.account_id, BigDecimal::zero()))
            .collect();
        for leg in legs {
            let balance = replayed
                .get_mut(&leg.account_id)
                .ok_or(LedgerError::UnknownReplayAccount(leg.account_id))?;
            *balance += leg.amount;
        }

        let mut result = Vec::with_capacity(accounts.len());
        for account in accounts {
            let replayed_balance = replayed
                .remove(&account.account_id)
                .ok_or(LedgerError::UnknownReplayAccount(account.account_id))?;
            if replayed_balance != account.projected_balance {
                return Err(LedgerError::ReplayProjectionMismatch {
                    account_id: account.account_id,
                    replayed: replayed_balance,
                    projected: account.projected_balance,
                });
            }
            result.push(ReplayedAccountBalance {
                account_id: account.account_id,
                asset_code: account.asset_code,
                asset_decimals: account.asset_decimals,
                owner_type: account.owner_type,
                owner_reference: account.owner_reference,
                account_type: account.account_type,
                balance: replayed_balance,
            });
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn post(&self, command: &PostTransaction) -> Result<PostedTransaction, LedgerError> {
        if command.operation_type == "administrative_adjustment" {
            return Err(LedgerError::AdministrativeCorrectionRequired);
        }
        self.post_authorized(command, None).await
    }

    pub async fn post_administrative_correction(
        &self,
        correction: &AdministrativeCorrection,
    ) -> Result<PostedAdministrativeCorrection, LedgerError> {
        validate_administrative_correction(correction)?;
        let command = administrative_correction_transaction(correction);
        let transaction = self.post_authorized(&command, Some(correction)).await?;
        Ok(PostedAdministrativeCorrection {
            correction_id: correction.correction_id,
            transaction,
        })
    }

    async fn post_authorized(
        &self,
        command: &PostTransaction,
        correction: Option<&AdministrativeCorrection>,
    ) -> Result<PostedTransaction, LedgerError> {
        validate_command(command)?;

        let mut attempt = 0_u32;
        loop {
            match self.post_once(command, correction).await {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn post_once(
        &self,
        command: &PostTransaction,
        correction: Option<&AdministrativeCorrection>,
    ) -> Result<PostedTransaction, LedgerError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;

        let inserted = sqlx::query_as::<_, PostedTransaction>(
            r#"
            INSERT INTO ledger_journal_transactions
                (id, operation_type, idempotency_key, request_hash, external_reference, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING id, idempotency_key, created_at
            "#,
        )
        .bind(command.id)
        .bind(&command.operation_type)
        .bind(&command.idempotency_key)
        .bind(command.request_hash.as_slice())
        .bind(&command.external_reference)
        .bind(&command.metadata)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(posted) = inserted else {
            let posted = resolve_idempotent_replay(&mut tx, command).await?;
            if let Some(correction) = correction {
                let audit_exists = sqlx::query_scalar::<_, bool>(
                    r#"
                    SELECT EXISTS (
                        SELECT 1
                        FROM ledger_administrative_corrections
                        WHERE id = $1 AND journal_transaction_id = $2
                    )
                    "#,
                )
                .bind(correction.correction_id)
                .bind(posted.id)
                .fetch_one(&mut *tx)
                .await?;
                if !audit_exists {
                    return Err(LedgerError::AdministrativeCorrectionAuditMissing(posted.id));
                }
            }
            tx.commit().await?;
            return Ok(posted);
        };

        let mut account_ids: Vec<Uuid> = command.legs.iter().map(|leg| leg.account_id).collect();
        account_ids.sort_unstable();

        let locked_accounts = sqlx::query_as::<_, LockedAccount>(
            r#"
            SELECT a.id, a.asset_id, a.allow_negative, b.balance
            FROM ledger_accounts a
            JOIN ledger_account_balances b ON b.account_id = a.id
            WHERE a.id = ANY($1)
            ORDER BY a.id
            FOR UPDATE OF b
            "#,
        )
        .bind(&account_ids)
        .fetch_all(&mut *tx)
        .await?;

        let account_map: HashMap<Uuid, LockedAccount> = locked_accounts
            .into_iter()
            .map(|account| (account.id, account))
            .collect();

        let mut asset_totals: HashMap<Uuid, BigDecimal> = HashMap::new();
        for leg in &command.legs {
            let account = account_map
                .get(&leg.account_id)
                .ok_or(LedgerError::UnknownAccount(leg.account_id))?;
            *asset_totals
                .entry(account.asset_id)
                .or_insert_with(BigDecimal::zero) += leg.amount.clone();
        }

        if let Some(asset_id) = asset_totals
            .iter()
            .find_map(|(asset_id, total)| (!total.is_zero()).then_some(*asset_id))
        {
            return Err(LedgerError::Unbalanced(asset_id));
        }

        for leg in &command.legs {
            let account = &account_map[&leg.account_id];
            let next_balance = &account.balance + &leg.amount;
            if !account.allow_negative && next_balance < BigDecimal::zero() {
                return Err(LedgerError::InsufficientFunds {
                    account_id: account.id,
                    balance: account.balance.clone(),
                    delta: leg.amount.clone(),
                });
            }

            sqlx::query(
                r#"
                INSERT INTO ledger_journal_legs
                    (transaction_id, account_id, asset_id, amount)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(command.id)
            .bind(leg.account_id)
            .bind(account.asset_id)
            .bind(&leg.amount)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                UPDATE ledger_account_balances
                SET balance = $2, version = version + 1, updated_at = transaction_timestamp()
                WHERE account_id = $1
                "#,
            )
            .bind(leg.account_id)
            .bind(&next_balance)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO ledger_outbox (transaction_id, event_type, payload)
            VALUES ($1, 'ledger.transaction_posted', $2)
            "#,
        )
        .bind(command.id)
        .bind(json!({ "transaction_id": command.id }))
        .execute(&mut *tx)
        .await?;

        if let Some(correction) = correction {
            sqlx::query(
                r#"
                INSERT INTO ledger_administrative_corrections (
                    id,
                    journal_transaction_id,
                    reason_type,
                    reason,
                    evidence_reference,
                    incident_id,
                    initiated_by,
                    approved_by,
                    approved_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(correction.correction_id)
            .bind(command.id)
            .bind(correction.reason_type.as_str())
            .bind(&correction.reason)
            .bind(&correction.evidence_reference)
            .bind(&correction.incident_id)
            .bind(&correction.initiated_by)
            .bind(&correction.approved_by)
            .bind(correction.approved_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(posted)
    }

    pub async fn balance(&self, account_id: Uuid) -> Result<BigDecimal, LedgerError> {
        let balance = sqlx::query_scalar::<_, BigDecimal>(
            "SELECT balance FROM ledger_account_balances WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(balance)
    }

    pub async fn verify_balance_projection(&self) -> Result<bool, LedgerError> {
        let mismatch_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM ledger_account_balances b
            WHERE b.balance <> COALESCE((
                SELECT sum(l.amount)
                FROM ledger_journal_legs l
                WHERE l.account_id = b.account_id
            ), 0)
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(mismatch_count == 0)
    }
}

fn is_retryable_transaction_error(error: &LedgerError) -> bool {
    matches!(
        error,
        LedgerError::Database(sqlx::Error::Database(database_error))
            if matches!(database_error.code().as_deref(), Some("40001" | "40P01"))
    )
}

fn validate_account_provision(request: &AccountProvisionRequest) -> Result<(), LedgerError> {
    if request.owner_reference.is_empty() || request.owner_reference.len() > 255 {
        return Err(LedgerError::InvalidAccountProvision(
            "owner reference length is invalid".to_owned(),
        ));
    }
    let valid_pair = match request.owner_type.as_str() {
        "customer" => {
            !request.allow_negative
                && matches!(
                    request.account_type.as_str(),
                    "customer_available"
                        | "customer_locked"
                        | "customer_pending"
                        | "customer_withdrawal_reserved"
                )
        }
        "custody" => matches!(
            request.account_type.as_str(),
            "custody_vault" | "custody_withdrawn"
        ),
        "platform" => matches!(
            request.account_type.as_str(),
            "fee_revenue" | "rebate_expense" | "operational_adjustment"
        ),
        _ => false,
    };
    if !valid_pair {
        return Err(LedgerError::InvalidAccountProvision(format!(
            "owner type {} cannot use account type {} with allow_negative={}",
            request.owner_type, request.account_type, request.allow_negative
        )));
    }
    Ok(())
}

fn validate_command(command: &PostTransaction) -> Result<(), LedgerError> {
    if command.legs.len() < 2 {
        return Err(LedgerError::TooFewLegs);
    }

    let mut accounts = HashSet::with_capacity(command.legs.len());
    for leg in &command.legs {
        if leg.amount.is_zero() || !leg.amount.is_integer() {
            return Err(LedgerError::InvalidAmount);
        }
        if !accounts.insert(leg.account_id) {
            return Err(LedgerError::DuplicateAccount(leg.account_id));
        }
    }
    Ok(())
}

fn validate_administrative_correction(
    correction: &AdministrativeCorrection,
) -> Result<(), LedgerError> {
    validate_correction_field("idempotency key", &correction.idempotency_key, 255)?;
    validate_correction_field("reason", &correction.reason, 4_000)?;
    validate_correction_field("evidence reference", &correction.evidence_reference, 2_000)?;
    validate_correction_field("incident ID", &correction.incident_id, 255)?;
    validate_correction_field("initiator", &correction.initiated_by, 255)?;
    validate_correction_field("approver", &correction.approved_by, 255)?;

    if correction
        .initiated_by
        .trim()
        .eq_ignore_ascii_case(correction.approved_by.trim())
    {
        return Err(LedgerError::InvalidAdministrativeCorrection(
            "initiator and approver must be different people".to_owned(),
        ));
    }
    validate_command(&administrative_correction_transaction(correction))
}

fn validate_correction_field(
    field_name: &str,
    value: &str,
    maximum_length: usize,
) -> Result<(), LedgerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > maximum_length {
        return Err(LedgerError::InvalidAdministrativeCorrection(format!(
            "{field_name} must contain between 1 and {maximum_length} characters"
        )));
    }
    Ok(())
}

fn administrative_correction_transaction(correction: &AdministrativeCorrection) -> PostTransaction {
    PostTransaction {
        id: correction.transaction_id,
        operation_type: "administrative_adjustment".to_owned(),
        idempotency_key: correction.idempotency_key.clone(),
        request_hash: administrative_correction_request_hash(correction),
        external_reference: Some(json!({
            "correction_id": correction.correction_id,
            "incident_id": correction.incident_id,
        })),
        metadata: json!({
            "reason_type": correction.reason_type,
            "reason": correction.reason,
            "evidence_reference": correction.evidence_reference,
            "initiated_by": correction.initiated_by,
            "approved_by": correction.approved_by,
            "approved_at": correction.approved_at,
        }),
        legs: correction.legs.clone(),
    }
}

fn administrative_correction_request_hash(correction: &AdministrativeCorrection) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_correction_field(&mut hasher, correction.correction_id.as_bytes());
    hash_correction_field(&mut hasher, correction.transaction_id.as_bytes());
    hash_correction_field(&mut hasher, correction.idempotency_key.as_bytes());
    hash_correction_field(&mut hasher, correction.reason_type.as_str().as_bytes());
    hash_correction_field(&mut hasher, correction.reason.as_bytes());
    hash_correction_field(&mut hasher, correction.evidence_reference.as_bytes());
    hash_correction_field(&mut hasher, correction.incident_id.as_bytes());
    hash_correction_field(&mut hasher, correction.initiated_by.as_bytes());
    hash_correction_field(&mut hasher, correction.approved_by.as_bytes());
    hash_correction_field(&mut hasher, correction.approved_at.to_rfc3339().as_bytes());

    let mut legs = correction.legs.clone();
    legs.sort_unstable_by_key(|leg| leg.account_id);
    for leg in legs {
        hash_correction_field(&mut hasher, leg.account_id.as_bytes());
        hash_correction_field(&mut hasher, leg.amount.normalized().to_string().as_bytes());
    }
    hasher.finalize().into()
}

fn hash_correction_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

async fn resolve_idempotent_replay(
    tx: &mut Transaction<'_, Postgres>,
    command: &PostTransaction,
) -> Result<PostedTransaction, LedgerError> {
    let existing = sqlx::query_as::<_, ExistingTransaction>(
        r#"
        SELECT id, idempotency_key, request_hash, created_at
        FROM ledger_journal_transactions
        WHERE idempotency_key = $1
        "#,
    )
    .bind(&command.idempotency_key)
    .fetch_one(&mut **tx)
    .await?;

    if existing.request_hash != command.request_hash {
        return Err(LedgerError::IdempotencyConflict);
    }

    Ok(PostedTransaction {
        id: existing.id,
        idempotency_key: existing.idempotency_key,
        created_at: existing.created_at,
    })
}

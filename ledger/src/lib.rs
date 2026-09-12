use std::collections::{HashMap, HashSet};

use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

#[derive(Clone)]
pub struct Ledger {
    pool: PgPool,
}

impl Ledger {
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, LedgerError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        Ok(Self::from_pool(pool))
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), LedgerError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
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

    pub async fn post(&self, command: &PostTransaction) -> Result<PostedTransaction, LedgerError> {
        validate_command(command)?;

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
            return resolve_idempotent_replay(tx, command).await;
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

async fn resolve_idempotent_replay(
    mut tx: Transaction<'_, Postgres>,
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
    .fetch_one(&mut *tx)
    .await?;

    if existing.request_hash != command.request_hash {
        return Err(LedgerError::IdempotencyConflict);
    }

    tx.commit().await?;
    Ok(PostedTransaction {
        id: existing.id,
        idempotency_key: existing.idempotency_key,
        created_at: existing.created_at,
    })
}

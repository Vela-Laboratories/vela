use super::{
    is_retryable_transaction_error, post_command_in_transaction, validate_command, JournalLegInput,
    Ledger, LedgerError, PostTransaction, PostedTransaction,
};
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepositState {
    Observed,
    Confirming,
    Finalized,
    Credited,
    Rejected,
    Orphaned,
    Reversed,
    Quarantined,
}

impl DepositState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Confirming => "confirming",
            Self::Finalized => "finalized",
            Self::Credited => "credited",
            Self::Rejected => "rejected",
            Self::Orphaned => "orphaned",
            Self::Reversed => "reversed",
            Self::Quarantined => "quarantined",
        }
    }
}

impl FromStr for DepositState {
    type Err = LedgerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "observed" => Ok(Self::Observed),
            "confirming" => Ok(Self::Confirming),
            "finalized" => Ok(Self::Finalized),
            "credited" => Ok(Self::Credited),
            "rejected" => Ok(Self::Rejected),
            "orphaned" => Ok(Self::Orphaned),
            "reversed" => Ok(Self::Reversed),
            "quarantined" => Ok(Self::Quarantined),
            _ => Err(LedgerError::InvalidDeposit(format!(
                "unknown stored deposit state {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositChainEvent {
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: i32,
    pub log_index: i32,
    pub vault_address: String,
    pub vault_deposit_id: String,
    pub sender_address: String,
    pub asset_address: Option<String>,
    pub actual_amount: BigDecimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserveDeposit {
    pub deposit_id: Uuid,
    pub customer_reference: String,
    pub destination_account_id: Uuid,
    pub asset_id: Uuid,
    pub amount: BigDecimal,
    pub event: DepositChainEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcFinalityObservation {
    pub provider: String,
    pub chain_id: i64,
    pub finalized_block_number: i64,
    pub finalized_block_hash: String,
    pub deposit_block_hash: String,
    pub transaction_hash: String,
    pub log_index: i32,
    pub vault_address: String,
    pub vault_deposit_id: String,
    pub sender_address: String,
    pub beneficiary_reference: String,
    pub asset_address: Option<String>,
    pub actual_amount: BigDecimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizeDeposit {
    pub deposit_id: Uuid,
    pub first: RpcFinalityObservation,
    pub second: RpcFinalityObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositRecord {
    pub id: Uuid,
    pub customer_reference: String,
    pub destination_account_id: Uuid,
    pub asset_id: Uuid,
    pub amount: BigDecimal,
    pub state: DepositState,
    pub credit_transaction_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditedDeposit {
    pub deposit: DepositRecord,
    pub transaction: PostedTransaction,
}

#[derive(Debug, FromRow)]
struct StoredDeposit {
    id: Uuid,
    customer_reference: String,
    destination_account_id: Uuid,
    asset_id: Uuid,
    amount: BigDecimal,
    state: String,
    credit_transaction_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredObservation {
    id: Uuid,
    customer_reference: String,
    destination_account_id: Uuid,
    asset_id: Uuid,
    amount: BigDecimal,
    state: String,
    credit_transaction_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    chain_id: i64,
    block_number: i64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: i32,
    log_index: i32,
    vault_address: String,
    vault_deposit_id: String,
    sender_address: String,
    asset_address: Option<String>,
    actual_amount: BigDecimal,
}

#[derive(Debug, FromRow)]
struct DestinationAccount {
    asset_id: Uuid,
    owner_type: String,
    owner_reference: String,
    account_type: String,
}

#[derive(Debug, FromRow)]
struct CreditableDeposit {
    id: Uuid,
    customer_reference: String,
    destination_account_id: Uuid,
    asset_id: Uuid,
    amount: BigDecimal,
    state: String,
    credit_transaction_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    chain_id: i64,
    transaction_hash: String,
    log_index: i32,
}

impl TryFrom<StoredDeposit> for DepositRecord {
    type Error = LedgerError;

    fn try_from(value: StoredDeposit) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            customer_reference: value.customer_reference,
            destination_account_id: value.destination_account_id,
            asset_id: value.asset_id,
            amount: value.amount,
            state: DepositState::from_str(&value.state)?,
            credit_transaction_id: value.credit_transaction_id,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl Ledger {
    pub async fn observe_deposit(
        &self,
        observation: &ObserveDeposit,
    ) -> Result<DepositRecord, LedgerError> {
        validate_observation(observation)?;
        let mut attempt = 0_u32;
        loop {
            match self.observe_deposit_once(observation).await {
                Err(error)
                    if (is_retryable_transaction_error(&error) || is_unique_violation(&error))
                        && attempt < self.max_serializable_retries =>
                {
                    retry_backoff(attempt).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn observe_deposit_once(
        &self,
        observation: &ObserveDeposit,
    ) -> Result<DepositRecord, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;

        let existing = find_existing_observations(&mut tx, observation).await?;
        if !existing.is_empty() {
            if existing.len() != 1 || !observation_matches(&existing[0], observation) {
                return Err(LedgerError::DepositObservationConflict);
            }
            let record = stored_observation_record(&existing[0])?;
            tx.commit().await?;
            return Ok(record);
        }

        validate_destination_account(&mut tx, observation).await?;
        sqlx::query(
            r#"
            INSERT INTO ledger_deposits
                (id, customer_reference, destination_account_id, asset_id, amount, state)
            VALUES ($1, $2, $3, $4, $5, 'observed')
            "#,
        )
        .bind(observation.deposit_id)
        .bind(&observation.customer_reference)
        .bind(observation.destination_account_id)
        .bind(observation.asset_id)
        .bind(&observation.amount)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO ledger_deposit_chain_events (
                id, deposit_id, chain_id, block_number, block_hash,
                transaction_hash, transaction_index, log_index, vault_address,
                vault_deposit_id, sender_address, asset_address, actual_amount
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(observation.deposit_id)
        .bind(observation.event.chain_id)
        .bind(observation.event.block_number)
        .bind(&observation.event.block_hash)
        .bind(&observation.event.transaction_hash)
        .bind(observation.event.transaction_index)
        .bind(observation.event.log_index)
        .bind(&observation.event.vault_address)
        .bind(&observation.event.vault_deposit_id)
        .bind(&observation.event.sender_address)
        .bind(&observation.event.asset_address)
        .bind(&observation.event.actual_amount)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            observation.deposit_id,
            None,
            DepositState::Observed,
            Some("canonical vault event observed"),
            json!({ "event": observation.event }),
        )
        .await?;

        let stored = load_deposit(&mut tx, observation.deposit_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn mark_deposit_confirming(
        &self,
        deposit_id: Uuid,
        confirmations: u64,
        head_block_number: i64,
        head_block_hash: &str,
    ) -> Result<DepositRecord, LedgerError> {
        if confirmations == 0 || head_block_number < 0 || !is_hex_bytes(head_block_hash, 32) {
            return Err(LedgerError::InvalidDeposit(
                "confirmation evidence is incomplete".to_owned(),
            ));
        }
        self.transition_deposit(
            deposit_id,
            DepositState::Confirming,
            "deposit is awaiting finalized agreement",
            json!({
                "confirmations": confirmations,
                "head_block_number": head_block_number,
                "head_block_hash": head_block_hash.to_ascii_lowercase(),
            }),
        )
        .await
    }

    pub async fn finalize_deposit(
        &self,
        finality: &FinalizeDeposit,
    ) -> Result<DepositRecord, LedgerError> {
        validate_finality_agreement(finality)?;
        let mut attempt = 0_u32;
        loop {
            match self.finalize_deposit_once(finality).await {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    retry_backoff(attempt).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn finalize_deposit_once(
        &self,
        finality: &FinalizeDeposit,
    ) -> Result<DepositRecord, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let stored = load_deposit_for_update(&mut tx, finality.deposit_id).await?;
        let state = DepositState::from_str(&stored.state)?;
        if state == DepositState::Finalized {
            tx.commit().await?;
            return stored.try_into();
        }
        if state != DepositState::Confirming {
            return Err(invalid_transition(state, DepositState::Finalized));
        }
        let event = sqlx::query_as::<
            _,
            (
                i64,
                i64,
                String,
                String,
                i32,
                String,
                String,
                String,
                Option<String>,
                BigDecimal,
            ),
        >(
            r#"
            SELECT chain_id, block_number, block_hash, transaction_hash, log_index,
                   vault_address, vault_deposit_id, sender_address, asset_address, actual_amount
            FROM ledger_deposit_chain_events
            WHERE deposit_id = $1
            "#,
        )
        .bind(finality.deposit_id)
        .fetch_one(&mut *tx)
        .await?;
        if event.0 != finality.first.chain_id
            || event.1 > finality.first.finalized_block_number
            || !event
                .2
                .eq_ignore_ascii_case(&finality.first.deposit_block_hash)
            || !event
                .3
                .eq_ignore_ascii_case(&finality.first.transaction_hash)
            || event.4 != finality.first.log_index
            || !event.5.eq_ignore_ascii_case(&finality.first.vault_address)
            || event.6 != finality.first.vault_deposit_id
            || !event.7.eq_ignore_ascii_case(&finality.first.sender_address)
            || stored.customer_reference != finality.first.beneficiary_reference
            || !optional_address_eq(&event.8, &finality.first.asset_address)
            || event.9 != finality.first.actual_amount
        {
            return Err(LedgerError::DepositFinalityDisagreement);
        }
        update_state(
            &mut tx,
            finality.deposit_id,
            state,
            DepositState::Finalized,
            "two independent RPC providers agreed under finalized tag",
            serde_json::to_value(finality).map_err(|error| {
                LedgerError::InvalidDeposit(format!("could not encode finality evidence: {error}"))
            })?,
        )
        .await?;
        let stored = load_deposit(&mut tx, finality.deposit_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn orphan_deposit(
        &self,
        deposit_id: Uuid,
        reason: &str,
        evidence: Value,
    ) -> Result<DepositRecord, LedgerError> {
        if reason.trim().is_empty() || !evidence.is_object() {
            return Err(LedgerError::InvalidDeposit(
                "orphan evidence and reason are required".to_owned(),
            ));
        }
        self.transition_deposit(deposit_id, DepositState::Orphaned, reason, evidence)
            .await
    }

    async fn transition_deposit(
        &self,
        deposit_id: Uuid,
        to: DepositState,
        reason: &str,
        evidence: Value,
    ) -> Result<DepositRecord, LedgerError> {
        let mut attempt = 0_u32;
        loop {
            match self
                .transition_deposit_once(deposit_id, to, reason, evidence.clone())
                .await
            {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    retry_backoff(attempt).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn transition_deposit_once(
        &self,
        deposit_id: Uuid,
        to: DepositState,
        reason: &str,
        evidence: Value,
    ) -> Result<DepositRecord, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let stored = load_deposit_for_update(&mut tx, deposit_id).await?;
        let from = DepositState::from_str(&stored.state)?;
        if from == to {
            tx.commit().await?;
            return stored.try_into();
        }
        if !allowed_transition(from, to) {
            return Err(invalid_transition(from, to));
        }
        update_state(&mut tx, deposit_id, from, to, reason, evidence).await?;
        let stored = load_deposit(&mut tx, deposit_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn credit_finalized_deposit(
        &self,
        deposit_id: Uuid,
        custody_account_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<CreditedDeposit, LedgerError> {
        let mut attempt = 0_u32;
        loop {
            match self
                .credit_finalized_deposit_once(deposit_id, custody_account_id, transaction_id)
                .await
            {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    retry_backoff(attempt).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn credit_finalized_deposit_once(
        &self,
        deposit_id: Uuid,
        custody_account_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<CreditedDeposit, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let deposit = load_creditable_deposit(&mut tx, deposit_id).await?;
        let state = DepositState::from_str(&deposit.state)?;
        validate_custody_account(&mut tx, custody_account_id, deposit.asset_id).await?;
        let idempotency_key = format!(
            "deposit-credit:{}:{}:{}",
            deposit.chain_id,
            deposit.transaction_hash.to_ascii_lowercase(),
            deposit.log_index
        );
        let request_hash = deposit_credit_hash(&deposit, custody_account_id, &idempotency_key);
        if state == DepositState::Credited {
            let (posted, stored_hash) = load_posted_transaction(
                &mut tx,
                deposit.credit_transaction_id.ok_or_else(|| {
                    LedgerError::InvalidDeposit(
                        "credited deposit has no journal transaction".to_owned(),
                    )
                })?,
            )
            .await?;
            if stored_hash != request_hash {
                return Err(LedgerError::IdempotencyConflict);
            }
            let record = creditable_record(&deposit)?;
            tx.commit().await?;
            return Ok(CreditedDeposit {
                deposit: record,
                transaction: posted,
            });
        }
        if state != DepositState::Finalized {
            return Err(invalid_transition(state, DepositState::Credited));
        }

        let command = PostTransaction {
            id: transaction_id,
            operation_type: "deposit_credit".to_owned(),
            idempotency_key,
            request_hash,
            external_reference: Some(json!({
                "deposit_id": deposit.id,
                "chain_id": deposit.chain_id,
                "transaction_hash": deposit.transaction_hash,
                "log_index": deposit.log_index,
            })),
            metadata: json!({
                "source": "verified_deposit_pipeline",
                "customer_reference": deposit.customer_reference,
            }),
            legs: vec![
                JournalLegInput {
                    account_id: custody_account_id,
                    amount: -deposit.amount.clone(),
                },
                JournalLegInput {
                    account_id: deposit.destination_account_id,
                    amount: deposit.amount.clone(),
                },
            ],
        };
        validate_command(&command)?;
        let posted = post_command_in_transaction(&mut tx, &command).await?;
        sqlx::query(
            r#"
            UPDATE ledger_deposits
            SET state = 'credited', credit_transaction_id = $2,
                updated_at = transaction_timestamp()
            WHERE id = $1 AND state = 'finalized'
            "#,
        )
        .bind(deposit_id)
        .bind(posted.transaction.id)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            deposit_id,
            Some(DepositState::Finalized),
            DepositState::Credited,
            Some("finalized custody event credited atomically"),
            json!({ "journal_transaction_id": posted.transaction.id }),
        )
        .await?;
        let record: DepositRecord = load_deposit(&mut tx, deposit_id).await?.try_into()?;
        tx.commit().await?;
        Ok(CreditedDeposit {
            deposit: record,
            transaction: posted.transaction,
        })
    }

    pub async fn deposit(&self, deposit_id: Uuid) -> Result<DepositRecord, LedgerError> {
        let mut tx = self.pool.begin().await?;
        let stored = load_deposit(&mut tx, deposit_id).await?;
        tx.commit().await?;
        stored.try_into()
    }
}

async fn set_serializable(tx: &mut Transaction<'_, Postgres>) -> Result<(), LedgerError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn find_existing_observations(
    tx: &mut Transaction<'_, Postgres>,
    observation: &ObserveDeposit,
) -> Result<Vec<StoredObservation>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredObservation>(
        r#"
        SELECT
            d.id, d.customer_reference, d.destination_account_id, d.asset_id,
            d.amount, d.state, d.credit_transaction_id, d.created_at, d.updated_at,
            e.chain_id, e.block_number, e.block_hash, e.transaction_hash,
            e.transaction_index, e.log_index, e.vault_address, e.vault_deposit_id,
            e.sender_address, e.asset_address, e.actual_amount
        FROM ledger_deposits d
        JOIN ledger_deposit_chain_events e ON e.deposit_id = d.id
        WHERE d.id = $1
           OR (e.chain_id = $2 AND lower(e.transaction_hash) = lower($3) AND e.log_index = $4)
           OR e.vault_deposit_id = $5
        FOR UPDATE OF d
        "#,
    )
    .bind(observation.deposit_id)
    .bind(observation.event.chain_id)
    .bind(&observation.event.transaction_hash)
    .bind(observation.event.log_index)
    .bind(&observation.event.vault_deposit_id)
    .fetch_all(&mut **tx)
    .await?)
}

fn observation_matches(stored: &StoredObservation, requested: &ObserveDeposit) -> bool {
    stored.id == requested.deposit_id
        && stored.customer_reference == requested.customer_reference
        && stored.destination_account_id == requested.destination_account_id
        && stored.asset_id == requested.asset_id
        && stored.amount == requested.amount
        && stored.chain_id == requested.event.chain_id
        && stored.block_number == requested.event.block_number
        && stored
            .block_hash
            .eq_ignore_ascii_case(&requested.event.block_hash)
        && stored
            .transaction_hash
            .eq_ignore_ascii_case(&requested.event.transaction_hash)
        && stored.transaction_index == requested.event.transaction_index
        && stored.log_index == requested.event.log_index
        && stored
            .vault_address
            .eq_ignore_ascii_case(&requested.event.vault_address)
        && stored.vault_deposit_id == requested.event.vault_deposit_id
        && stored
            .sender_address
            .eq_ignore_ascii_case(&requested.event.sender_address)
        && optional_address_eq(&stored.asset_address, &requested.event.asset_address)
        && stored.actual_amount == requested.event.actual_amount
}

fn stored_observation_record(stored: &StoredObservation) -> Result<DepositRecord, LedgerError> {
    StoredDeposit {
        id: stored.id,
        customer_reference: stored.customer_reference.clone(),
        destination_account_id: stored.destination_account_id,
        asset_id: stored.asset_id,
        amount: stored.amount.clone(),
        state: stored.state.clone(),
        credit_transaction_id: stored.credit_transaction_id,
        created_at: stored.created_at,
        updated_at: stored.updated_at,
    }
    .try_into()
}

async fn validate_destination_account(
    tx: &mut Transaction<'_, Postgres>,
    observation: &ObserveDeposit,
) -> Result<(), LedgerError> {
    let account = sqlx::query_as::<_, DestinationAccount>(
        "SELECT asset_id, owner_type, owner_reference, account_type FROM ledger_accounts WHERE id = $1",
    )
    .bind(observation.destination_account_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::UnknownAccount(
        observation.destination_account_id,
    ))?;
    if account.asset_id != observation.asset_id
        || account.owner_type != "customer"
        || account.owner_reference != observation.customer_reference
        || account.account_type != "customer_available"
    {
        return Err(LedgerError::InvalidDeposit(
            "destination must be the customer's available account for the observed asset"
                .to_owned(),
        ));
    }

    let asset = sqlx::query_as::<_, (Option<i64>, Option<String>)>(
        "SELECT chain_id, contract_address FROM ledger_assets WHERE id = $1",
    )
    .bind(observation.asset_id)
    .fetch_one(&mut **tx)
    .await?;
    if asset.0 != Some(observation.event.chain_id)
        || !optional_address_eq(&asset.1, &observation.event.asset_address)
    {
        return Err(LedgerError::InvalidDeposit(
            "chain event does not match the configured ledger asset".to_owned(),
        ));
    }
    Ok(())
}

async fn validate_custody_account(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    asset_id: Uuid,
) -> Result<(), LedgerError> {
    let account = sqlx::query_as::<_, (Uuid, String, String, bool)>(
        "SELECT asset_id, owner_type, account_type, allow_negative FROM ledger_accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::UnknownAccount(account_id))?;
    if account.0 != asset_id || account.1 != "custody" || account.2 != "custody_vault" || !account.3
    {
        return Err(LedgerError::InvalidDeposit(
            "deposit credit requires a negative-enabled custody vault account for the same asset"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn load_deposit(
    tx: &mut Transaction<'_, Postgres>,
    deposit_id: Uuid,
) -> Result<StoredDeposit, LedgerError> {
    sqlx::query_as::<_, StoredDeposit>(
        r#"
        SELECT id, customer_reference, destination_account_id, asset_id, amount,
               state, credit_transaction_id, created_at, updated_at
        FROM ledger_deposits WHERE id = $1
        "#,
    )
    .bind(deposit_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::DepositNotFound(deposit_id))
}

async fn load_deposit_for_update(
    tx: &mut Transaction<'_, Postgres>,
    deposit_id: Uuid,
) -> Result<StoredDeposit, LedgerError> {
    sqlx::query_as::<_, StoredDeposit>(
        r#"
        SELECT id, customer_reference, destination_account_id, asset_id, amount,
               state, credit_transaction_id, created_at, updated_at
        FROM ledger_deposits WHERE id = $1 FOR UPDATE
        "#,
    )
    .bind(deposit_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::DepositNotFound(deposit_id))
}

async fn load_creditable_deposit(
    tx: &mut Transaction<'_, Postgres>,
    deposit_id: Uuid,
) -> Result<CreditableDeposit, LedgerError> {
    sqlx::query_as::<_, CreditableDeposit>(
        r#"
        SELECT d.id, d.customer_reference, d.destination_account_id, d.asset_id,
               d.amount, d.state, d.credit_transaction_id, d.created_at, d.updated_at,
               e.chain_id, e.transaction_hash, e.log_index
        FROM ledger_deposits d
        JOIN ledger_deposit_chain_events e ON e.deposit_id = d.id
        WHERE d.id = $1
        FOR UPDATE OF d
        "#,
    )
    .bind(deposit_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::DepositNotFound(deposit_id))
}

fn creditable_record(stored: &CreditableDeposit) -> Result<DepositRecord, LedgerError> {
    StoredDeposit {
        id: stored.id,
        customer_reference: stored.customer_reference.clone(),
        destination_account_id: stored.destination_account_id,
        asset_id: stored.asset_id,
        amount: stored.amount.clone(),
        state: stored.state.clone(),
        credit_transaction_id: stored.credit_transaction_id,
        created_at: stored.created_at,
        updated_at: stored.updated_at,
    }
    .try_into()
}

async fn load_posted_transaction(
    tx: &mut Transaction<'_, Postgres>,
    transaction_id: Uuid,
) -> Result<(PostedTransaction, [u8; 32]), LedgerError> {
    let stored = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, Vec<u8>)>(
        "SELECT id, idempotency_key, created_at, request_hash FROM ledger_journal_transactions WHERE id = $1",
    )
    .bind(transaction_id)
    .fetch_one(&mut **tx)
    .await?;
    let request_hash: [u8; 32] = stored.3.try_into().map_err(|_| {
        LedgerError::InvalidDeposit("stored deposit credit hash is malformed".to_owned())
    })?;
    Ok((
        PostedTransaction {
            id: stored.0,
            idempotency_key: stored.1,
            created_at: stored.2,
        },
        request_hash,
    ))
}

async fn update_state(
    tx: &mut Transaction<'_, Postgres>,
    deposit_id: Uuid,
    from: DepositState,
    to: DepositState,
    reason: &str,
    evidence: Value,
) -> Result<(), LedgerError> {
    let changed = sqlx::query(
        "UPDATE ledger_deposits SET state = $3, updated_at = transaction_timestamp() WHERE id = $1 AND state = $2",
    )
    .bind(deposit_id)
    .bind(from.as_str())
    .bind(to.as_str())
    .execute(&mut **tx)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(invalid_transition(from, to));
    }
    insert_history(tx, deposit_id, Some(from), to, Some(reason), evidence).await
}

async fn insert_history(
    tx: &mut Transaction<'_, Postgres>,
    deposit_id: Uuid,
    from: Option<DepositState>,
    to: DepositState,
    reason: Option<&str>,
    evidence: Value,
) -> Result<(), LedgerError> {
    sqlx::query(
        r#"
        INSERT INTO ledger_deposit_state_history
            (deposit_id, from_state, to_state, reason, evidence)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(deposit_id)
    .bind(from.map(DepositState::as_str))
    .bind(to.as_str())
    .bind(reason)
    .bind(evidence)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_observation(observation: &ObserveDeposit) -> Result<(), LedgerError> {
    if observation.customer_reference.trim().is_empty()
        || observation.customer_reference.len() > 255
        || observation.amount <= BigDecimal::zero()
        || !observation.amount.is_integer()
        || observation.amount != observation.event.actual_amount
        || observation.event.chain_id <= 0
        || observation.event.block_number < 0
        || observation.event.transaction_index < 0
        || observation.event.log_index < 0
        || observation.event.vault_deposit_id.trim().is_empty()
        || !is_hex_bytes(&observation.event.block_hash, 32)
        || !is_hex_bytes(&observation.event.transaction_hash, 32)
        || !is_hex_bytes(&observation.event.vault_address, 20)
        || !is_hex_bytes(&observation.event.sender_address, 20)
        || observation
            .event
            .asset_address
            .as_ref()
            .is_some_and(|address| !is_hex_bytes(address, 20))
    {
        return Err(LedgerError::InvalidDeposit(
            "observation contains invalid identity, amount, or chain evidence".to_owned(),
        ));
    }
    Ok(())
}

fn validate_finality_agreement(finality: &FinalizeDeposit) -> Result<(), LedgerError> {
    let first = &finality.first;
    let second = &finality.second;
    let provider_names_valid = !first.provider.trim().is_empty()
        && !second.provider.trim().is_empty()
        && !first.provider.eq_ignore_ascii_case(&second.provider);
    let evidence_valid = first.finalized_block_number >= 0
        && first.chain_id > 0
        && is_hex_bytes(&first.finalized_block_hash, 32)
        && is_hex_bytes(&first.deposit_block_hash, 32)
        && is_hex_bytes(&first.transaction_hash, 32)
        && first.log_index >= 0
        && is_hex_bytes(&first.vault_address, 20)
        && is_hex_bytes(&first.sender_address, 20)
        && first
            .asset_address
            .as_ref()
            .is_none_or(|address| is_hex_bytes(address, 20))
        && !first.vault_deposit_id.trim().is_empty()
        && !first.beneficiary_reference.trim().is_empty()
        && first.actual_amount > BigDecimal::zero()
        && first.actual_amount.is_integer();
    let providers_agree = first.chain_id == second.chain_id
        && first.finalized_block_number == second.finalized_block_number
        && first
            .finalized_block_hash
            .eq_ignore_ascii_case(&second.finalized_block_hash)
        && first
            .deposit_block_hash
            .eq_ignore_ascii_case(&second.deposit_block_hash)
        && first
            .transaction_hash
            .eq_ignore_ascii_case(&second.transaction_hash)
        && first.log_index == second.log_index
        && first
            .vault_address
            .eq_ignore_ascii_case(&second.vault_address)
        && first.vault_deposit_id == second.vault_deposit_id
        && first
            .sender_address
            .eq_ignore_ascii_case(&second.sender_address)
        && first.beneficiary_reference == second.beneficiary_reference
        && optional_address_eq(&first.asset_address, &second.asset_address)
        && first.actual_amount == second.actual_amount;
    if !provider_names_valid || !evidence_valid || !providers_agree {
        return Err(LedgerError::DepositFinalityDisagreement);
    }
    Ok(())
}

fn allowed_transition(from: DepositState, to: DepositState) -> bool {
    matches!(
        (from, to),
        (DepositState::Observed, DepositState::Confirming)
            | (DepositState::Observed, DepositState::Rejected)
            | (DepositState::Observed, DepositState::Orphaned)
            | (DepositState::Observed, DepositState::Quarantined)
            | (DepositState::Confirming, DepositState::Rejected)
            | (DepositState::Confirming, DepositState::Orphaned)
            | (DepositState::Confirming, DepositState::Quarantined)
            | (DepositState::Finalized, DepositState::Orphaned)
            | (DepositState::Finalized, DepositState::Quarantined)
    )
}

fn invalid_transition(from: DepositState, to: DepositState) -> LedgerError {
    LedgerError::InvalidDepositTransition {
        from: from.as_str().to_owned(),
        to: to.as_str().to_owned(),
    }
}

fn deposit_credit_hash(
    deposit: &CreditableDeposit,
    custody_account_id: Uuid,
    idempotency_key: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for value in [
        deposit.id.as_bytes().as_slice(),
        deposit.destination_account_id.as_bytes().as_slice(),
        custody_account_id.as_bytes().as_slice(),
        idempotency_key.as_bytes(),
        deposit.amount.normalized().to_string().as_bytes(),
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

fn optional_address_eq(left: &Option<String>, right: &Option<String>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn is_hex_bytes(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

async fn retry_backoff(attempt: u32) {
    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
}

fn is_unique_violation(error: &LedgerError) -> bool {
    matches!(
        error,
        LedgerError::Database(sqlx::Error::Database(database_error))
            if database_error.code().as_deref() == Some("23505")
    )
}

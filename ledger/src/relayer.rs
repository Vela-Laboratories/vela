use crate::{Ledger, LedgerError};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use sha3::{Digest, Keccak256};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReserveRelayerNonce {
    pub reservation_id: Uuid,
    pub withdrawal_id: Uuid,
    pub sender_address: String,
    pub primary_pending_nonce: u64,
    pub secondary_pending_nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas_wei: u128,
    pub max_priority_fee_per_gas_wei: u128,
    pub maximum_cost_wei: u128,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub allocated_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct RelayerNonceReservation {
    pub id: Uuid,
    pub withdrawal_id: Uuid,
    pub chain_id: i64,
    pub sender_address: String,
    pub sender_nonce: BigDecimal,
    pub gas_limit: i64,
    pub max_fee_per_gas_wei: BigDecimal,
    pub max_priority_fee_per_gas_wei: BigDecimal,
    pub maximum_cost_wei: BigDecimal,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub allocated_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PrepareRelayerTransaction {
    pub prepared_id: Uuid,
    pub reservation_id: Uuid,
    pub transaction_hash: String,
    pub signed_transaction: Vec<u8>,
    pub prepared_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct PreparedRelayerTransaction {
    pub id: Uuid,
    pub reservation_id: Uuid,
    pub transaction_hash: String,
    pub signed_transaction: Vec<u8>,
    pub prepared_by: String,
    pub prepared_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayerSubmissionOutcome {
    Accepted,
    Uncertain,
}

impl RelayerSubmissionOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Uncertain => "uncertain",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecordRelayerSubmissionResult {
    pub prepared_id: Uuid,
    pub outcome: RelayerSubmissionOutcome,
    pub provider_name: String,
    pub provider_reference: Option<String>,
    pub submitted_by: String,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct RelayerSubmissionResult {
    pub prepared_id: Uuid,
    pub outcome: String,
    pub provider_name: String,
    pub provider_reference: Option<String>,
    pub submitted_by: String,
    pub submitted_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
}

impl Ledger {
    pub async fn prepare_relayer_transaction(
        &self,
        command: &PrepareRelayerTransaction,
    ) -> Result<PreparedRelayerTransaction, LedgerError> {
        let computed_hash = format!(
            "0x{}",
            hex::encode(Keccak256::digest(&command.signed_transaction))
        );
        if command.signed_transaction.is_empty()
            || command.signed_transaction.len() > 32768
            || command.transaction_hash != computed_hash
            || command.prepared_by.trim().is_empty()
        {
            return Err(LedgerError::InvalidWithdrawal(
                "prepared relayer transaction has invalid signed bytes or hash".to_owned(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        if let Some(stored) = sqlx::query_as::<_, PreparedRelayerTransaction>(
            "SELECT * FROM ledger_relayer_prepared_transactions WHERE reservation_id = $1 FOR UPDATE",
        )
        .bind(command.reservation_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if stored.id != command.prepared_id
                || stored.transaction_hash != command.transaction_hash
                || stored.signed_transaction != command.signed_transaction
                || stored.prepared_by != command.prepared_by
            {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return Ok(stored);
        }
        sqlx::query(
            "INSERT INTO ledger_relayer_prepared_transactions (id, reservation_id, transaction_hash, signed_transaction, prepared_by) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(command.prepared_id)
        .bind(command.reservation_id)
        .bind(&command.transaction_hash)
        .bind(&command.signed_transaction)
        .bind(&command.prepared_by)
        .execute(&mut *tx)
        .await?;
        let stored = sqlx::query_as::<_, PreparedRelayerTransaction>(
            "SELECT * FROM ledger_relayer_prepared_transactions WHERE id = $1",
        )
        .bind(command.prepared_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn record_relayer_submission_result(
        &self,
        command: &RecordRelayerSubmissionResult,
    ) -> Result<RelayerSubmissionResult, LedgerError> {
        if command.provider_name.trim().is_empty()
            || command.submitted_by.trim().is_empty()
            || matches!(command.outcome, RelayerSubmissionOutcome::Accepted)
                && command
                    .provider_reference
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
        {
            return Err(LedgerError::InvalidWithdrawal(
                "relayer submission outcome requires provider evidence".to_owned(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        if let Some(stored) = sqlx::query_as::<_, RelayerSubmissionResult>(
            "SELECT * FROM ledger_relayer_submission_results WHERE prepared_id = $1 FOR UPDATE",
        )
        .bind(command.prepared_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if stored.outcome != command.outcome.as_str()
                || stored.provider_name != command.provider_name
                || stored.provider_reference != command.provider_reference
                || stored.submitted_by != command.submitted_by
                || stored.submitted_at.timestamp_micros() != command.submitted_at.timestamp_micros()
            {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return Ok(stored);
        }
        sqlx::query(
            "INSERT INTO ledger_relayer_submission_results (prepared_id, outcome, provider_name, provider_reference, submitted_by, submitted_at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(command.prepared_id)
        .bind(command.outcome.as_str())
        .bind(&command.provider_name)
        .bind(&command.provider_reference)
        .bind(&command.submitted_by)
        .bind(command.submitted_at)
        .execute(&mut *tx)
        .await?;
        let stored = sqlx::query_as::<_, RelayerSubmissionResult>(
            "SELECT * FROM ledger_relayer_submission_results WHERE prepared_id = $1",
        )
        .bind(command.prepared_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn reserve_relayer_nonce(
        &self,
        command: &ReserveRelayerNonce,
    ) -> Result<RelayerNonceReservation, LedgerError> {
        if command.primary_pending_nonce != command.secondary_pending_nonce
            || command.gas_limit == 0
            || command.gas_limit > i64::MAX as u64
            || command.max_fee_per_gas_wei == 0
            || command.max_priority_fee_per_gas_wei == 0
            || command.max_priority_fee_per_gas_wei > command.max_fee_per_gas_wei
            || command
                .max_fee_per_gas_wei
                .checked_mul(u128::from(command.gas_limit))
                != Some(command.maximum_cost_wei)
            || command.primary_provider.trim().is_empty()
            || command.secondary_provider.trim().is_empty()
            || command
                .primary_provider
                .eq_ignore_ascii_case(&command.secondary_provider)
            || command.allocated_by.trim().is_empty()
            || !is_lowercase_address(&command.sender_address)
        {
            return Err(LedgerError::InvalidWithdrawal(
                "invalid relayer nonce or bounded fee decision".to_owned(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query_as::<_, RelayerNonceReservation>(
            "SELECT * FROM ledger_relayer_nonce_reservations WHERE withdrawal_id = $1 FOR UPDATE",
        )
        .bind(command.withdrawal_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(stored) = existing {
            if !same_reservation(&stored, command) {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return Ok(stored);
        }
        sqlx::query(
            r#"INSERT INTO ledger_relayer_nonce_reservations (
                id, withdrawal_id, chain_id, sender_address, sender_nonce,
                gas_limit, max_fee_per_gas_wei, max_priority_fee_per_gas_wei,
                maximum_cost_wei, primary_provider, secondary_provider, allocated_by
            ) VALUES ($1, $2, 84532, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        )
        .bind(command.reservation_id)
        .bind(command.withdrawal_id)
        .bind(&command.sender_address)
        .bind(BigDecimal::from(command.primary_pending_nonce))
        .bind(command.gas_limit as i64)
        .bind(BigDecimal::from(command.max_fee_per_gas_wei))
        .bind(BigDecimal::from(command.max_priority_fee_per_gas_wei))
        .bind(BigDecimal::from(command.maximum_cost_wei))
        .bind(&command.primary_provider)
        .bind(&command.secondary_provider)
        .bind(&command.allocated_by)
        .execute(&mut *tx)
        .await?;
        let stored = sqlx::query_as::<_, RelayerNonceReservation>(
            "SELECT * FROM ledger_relayer_nonce_reservations WHERE withdrawal_id = $1",
        )
        .bind(command.withdrawal_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(stored)
    }
}

fn is_lowercase_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn same_reservation(stored: &RelayerNonceReservation, command: &ReserveRelayerNonce) -> bool {
    stored.id == command.reservation_id
        && stored.withdrawal_id == command.withdrawal_id
        && stored.chain_id == 84532
        && stored.sender_address == command.sender_address
        && stored.sender_nonce == command.primary_pending_nonce
        && stored.gas_limit == command.gas_limit as i64
        && stored.max_fee_per_gas_wei == command.max_fee_per_gas_wei
        && stored.max_priority_fee_per_gas_wei == command.max_priority_fee_per_gas_wei
        && stored.maximum_cost_wei == command.maximum_cost_wei
        && stored.primary_provider == command.primary_provider
        && stored.secondary_provider == command.secondary_provider
        && stored.allocated_by == command.allocated_by
}

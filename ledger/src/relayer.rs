use crate::{Ledger, LedgerError};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
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

impl Ledger {
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

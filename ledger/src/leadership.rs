use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{Ledger, LedgerError, PostTransaction, PostedTransaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineLeaderToken {
    pub owner_id: Uuid,
    pub generation: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineLeadership {
    pub token: EngineLeaderToken,
    pub lease_expires_at: DateTime<Utc>,
}

impl Ledger {
    pub async fn acquire_engine_leadership(
        &self,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<Option<EngineLeadership>, LedgerError> {
        let lease_ms = lease_millis(lease_duration)?;
        let row = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
            r#"
            UPDATE ledger_engine_leadership
            SET owner_id = $1,
                generation = generation + 1,
                lease_expires_at = transaction_timestamp() + ($2 * interval '1 millisecond'),
                acquired_at = transaction_timestamp(),
                renewed_at = transaction_timestamp()
            WHERE singleton
              AND (owner_id IS NULL OR lease_expires_at <= transaction_timestamp())
            RETURNING generation, lease_expires_at
            "#,
        )
        .bind(owner_id)
        .bind(lease_ms)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(generation, lease_expires_at)| EngineLeadership {
            token: EngineLeaderToken {
                owner_id,
                generation,
            },
            lease_expires_at,
        }))
    }

    pub async fn renew_engine_leadership(
        &self,
        token: EngineLeaderToken,
        lease_duration: Duration,
    ) -> Result<Option<DateTime<Utc>>, LedgerError> {
        let lease_ms = lease_millis(lease_duration)?;
        sqlx::query_scalar(
            r#"
            UPDATE ledger_engine_leadership
            SET lease_expires_at = transaction_timestamp() + ($3 * interval '1 millisecond'),
                renewed_at = transaction_timestamp()
            WHERE singleton AND owner_id = $1 AND generation = $2
              AND lease_expires_at > transaction_timestamp()
            RETURNING lease_expires_at
            "#,
        )
        .bind(token.owner_id)
        .bind(token.generation)
        .bind(lease_ms)
        .fetch_optional(&self.pool)
        .await
        .map_err(LedgerError::from)
    }

    pub async fn validate_engine_leadership(
        &self,
        token: EngineLeaderToken,
    ) -> Result<bool, LedgerError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM ledger_engine_leadership
                WHERE singleton AND owner_id = $1 AND generation = $2
                  AND lease_expires_at > transaction_timestamp()
            )
            "#,
        )
        .bind(token.owner_id)
        .bind(token.generation)
        .fetch_one(&self.pool)
        .await
        .map_err(LedgerError::from)
    }

    pub async fn release_engine_leadership(
        &self,
        token: EngineLeaderToken,
    ) -> Result<bool, LedgerError> {
        let result = sqlx::query(
            r#"
            UPDATE ledger_engine_leadership
            SET owner_id = NULL, lease_expires_at = NULL, renewed_at = transaction_timestamp()
            WHERE singleton AND owner_id = $1 AND generation = $2
            "#,
        )
        .bind(token.owner_id)
        .bind(token.generation)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn post_as_engine_leader(
        &self,
        command: &PostTransaction,
        token: EngineLeaderToken,
    ) -> Result<PostedTransaction, LedgerError> {
        self.post_authorized(command, None, Some(token)).await
    }
}

pub(crate) async fn assert_engine_leadership_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    token: EngineLeaderToken,
) -> Result<(), LedgerError> {
    let row = sqlx::query_as::<_, (Option<Uuid>, i64, bool)>(
        r#"
        SELECT owner_id, generation, lease_expires_at > transaction_timestamp()
        FROM ledger_engine_leadership WHERE singleton FOR UPDATE
        "#,
    )
    .fetch_one(&mut **tx)
    .await?;
    if row.0 != Some(token.owner_id) || row.1 != token.generation || !row.2 {
        return Err(LedgerError::EngineLeadershipLost);
    }
    Ok(())
}

fn lease_millis(duration: Duration) -> Result<i64, LedgerError> {
    let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
    if millis < 100 {
        return Err(LedgerError::InvalidEngineLease(
            "lease duration must be at least 100ms".to_owned(),
        ));
    }
    Ok(millis)
}

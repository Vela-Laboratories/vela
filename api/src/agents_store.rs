//! PostgreSQL authority for delegated-agent permissions. Every operation
//! locks the same namespace/agent key, including the first registration.

use crate::agents::{AgentDelegation, CapabilityScope, OrderScopeCheck};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use types::UserId;

#[derive(Debug, thiserror::Error)]
pub enum AgentStoreError {
    #[error("{0}")]
    Conflict(&'static str),
    #[error("delegation storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("invalid persisted delegation")]
    InvalidRecord,
}

#[derive(Clone)]
pub struct AgentStore {
    pool: PgPool,
    namespace: String,
}

#[derive(FromRow)]
struct DelegationRow {
    agent: String,
    master: String,
    expires_at_ms: String,
    max_notional_per_order: String,
    nonce: String,
    revoked: bool,
    scope: serde_json::Value,
}

const SELECT_DELEGATION: &str = "SELECT agent, master, expires_at_ms::text, max_notional_per_order::text, nonce::text, revoked, scope FROM api_agent_delegations WHERE namespace = $1 AND agent = $2";

impl TryFrom<DelegationRow> for AgentDelegation {
    type Error = AgentStoreError;
    fn try_from(row: DelegationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            agent: UserId::from_hex(&row.agent).map_err(|_| AgentStoreError::InvalidRecord)?,
            master: UserId::from_hex(&row.master).map_err(|_| AgentStoreError::InvalidRecord)?,
            expires_at_ms: row
                .expires_at_ms
                .parse()
                .map_err(|_| AgentStoreError::InvalidRecord)?,
            max_notional_per_order: row
                .max_notional_per_order
                .parse()
                .map_err(|_| AgentStoreError::InvalidRecord)?,
            nonce: row
                .nonce
                .parse()
                .map_err(|_| AgentStoreError::InvalidRecord)?,
            revoked: row.revoked,
            scope: serde_json::from_value::<CapabilityScope>(row.scope)
                .map_err(|_| AgentStoreError::InvalidRecord)?,
        })
    }
}

impl AgentStore {
    pub fn new(pool: PgPool, namespace: String) -> Self {
        Self { pool, namespace }
    }

    async fn lock(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        agent: &UserId,
    ) -> Result<(), AgentStoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("{}:{}", self.namespace, agent.to_hex()))
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn read(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        agent: &UserId,
    ) -> Result<Option<AgentDelegation>, AgentStoreError> {
        sqlx::query_as::<_, DelegationRow>(SELECT_DELEGATION)
            .bind(&self.namespace)
            .bind(agent.to_hex())
            .fetch_optional(&mut **tx)
            .await?
            .map(AgentDelegation::try_from)
            .transpose()
    }

    pub async fn register_signed(
        &self,
        delegation: &AgentDelegation,
        message: &str,
        signature: &str,
    ) -> Result<(), AgentStoreError> {
        let mut tx = self.pool.begin().await?;
        self.lock(&mut tx, &delegation.agent).await?;
        if let Some(previous) = self.read(&mut tx, &delegation.agent).await? {
            if previous.master != delegation.master {
                return Err(AgentStoreError::Conflict(
                    "agent already belongs to another master",
                ));
            }
            if delegation.nonce <= previous.nonce {
                return Err(AgentStoreError::Conflict("delegation nonce must increase"));
            }
        }
        sqlx::query("INSERT INTO api_agent_delegations (namespace, agent, master, expires_at_ms, max_notional_per_order, nonce, revoked, scope) VALUES ($1,$2,$3,$4::text::numeric,$5::text::numeric,$6::text::numeric,false,$7) ON CONFLICT (namespace,agent) DO UPDATE SET expires_at_ms=EXCLUDED.expires_at_ms, max_notional_per_order=EXCLUDED.max_notional_per_order, nonce=EXCLUDED.nonce, revoked=false, scope=EXCLUDED.scope, updated_at=now()")
            .bind(&self.namespace).bind(delegation.agent.to_hex()).bind(delegation.master.to_hex())
            .bind(delegation.expires_at_ms.to_string()).bind(delegation.max_notional_per_order.to_string()).bind(delegation.nonce.to_string())
            .bind(serde_json::to_value(&delegation.scope).map_err(|_| AgentStoreError::InvalidRecord)?)
            .execute(&mut *tx).await?;
        self.record_event(
            &mut tx,
            &delegation.master,
            &delegation.agent,
            delegation.nonce,
            "register",
            message,
            signature,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_event(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        master: &UserId,
        agent: &UserId,
        nonce: u64,
        action: &str,
        message: &str,
        signature: &str,
    ) -> Result<(), AgentStoreError> {
        sqlx::query("INSERT INTO api_agent_events (namespace,agent,master,nonce,action,message,signature) VALUES ($1,$2,$3,$4::text::numeric,$5,$6,$7)")
            .bind(&self.namespace).bind(agent.to_hex()).bind(master.to_hex()).bind(nonce.to_string())
            .bind(action).bind(message).bind(signature).execute(&mut **tx).await?;
        Ok(())
    }

    pub async fn agents_for(
        &self,
        master: &UserId,
    ) -> Result<Vec<AgentDelegation>, AgentStoreError> {
        let rows = sqlx::query_as::<_, DelegationRow>("SELECT agent, master, expires_at_ms::text, max_notional_per_order::text, nonce::text, revoked, scope FROM api_agent_delegations WHERE namespace=$1 AND master=$2 ORDER BY agent")
            .bind(&self.namespace).bind(master.to_hex()).fetch_all(&self.pool).await?;
        rows.into_iter().map(AgentDelegation::try_from).collect()
    }

    pub async fn revoke_signed(
        &self,
        master: &UserId,
        agent: &UserId,
        nonce: u64,
        message: &str,
        signature: &str,
    ) -> Result<bool, AgentStoreError> {
        let mut tx = self.pool.begin().await?;
        self.lock(&mut tx, agent).await?;
        let Some(previous) = self.read(&mut tx, agent).await? else {
            return Ok(false);
        };
        if previous.master != *master || nonce <= previous.nonce {
            return Ok(false);
        }
        sqlx::query("UPDATE api_agent_delegations SET revoked=true, nonce=$3::text::numeric, updated_at=now() WHERE namespace=$1 AND agent=$2")
            .bind(&self.namespace).bind(agent.to_hex()).bind(nonce.to_string())
            .execute(&mut *tx).await?;
        self.record_event(&mut tx, master, agent, nonce, "revoke", message, signature)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Check an already reserved parent against current authority without consuming cap buckets.
    pub async fn authorize_continuation(
        &self,
        master: &UserId,
        agent: &UserId,
        reserved_notional: u64,
        now_ms: u64,
        order: &OrderScopeCheck<'_>,
    ) -> Result<(), AgentStoreError> {
        let mut tx = self.pool.begin().await?;
        self.lock(&mut tx, agent).await?;
        let delegation = self
            .read(&mut tx, agent)
            .await?
            .ok_or(AgentStoreError::Conflict("unknown agent"))?;
        if delegation.master != *master
            || delegation.revoked
            || delegation.expires_at_ms <= now_ms
            || reserved_notional > delegation.max_notional_per_order
        {
            return Err(AgentStoreError::Conflict(
                "delegation does not authorize continuation",
            ));
        }
        delegation
            .scope
            .check_order_static(order.market, order.side, order.order_type)
            .map_err(AgentStoreError::Conflict)?;
        for (seconds, cap) in [
            (3600i64, delegation.scope.max_notional_per_hour),
            (86400i64, delegation.scope.max_notional_per_day),
        ] {
            let Some(cap) = cap else { continue };
            let bucket = now_ms / 1000 / seconds as u64;
            let used: Option<String> = sqlx::query_scalar("SELECT notional::text FROM api_agent_notional_buckets WHERE namespace=$1 AND agent=$2 AND bucket_seconds=$3 AND bucket_id=$4::text::numeric")
                .bind(&self.namespace).bind(agent.to_hex()).bind(seconds).bind(bucket.to_string()).fetch_optional(&mut *tx).await?;
            let used: u128 = used
                .as_deref()
                .unwrap_or("0")
                .parse()
                .map_err(|_| AgentStoreError::InvalidRecord)?;
            if reserved_notional > cap || used > u128::from(cap) {
                return Err(AgentStoreError::Conflict(
                    "delegated continuation cap exceeded",
                ));
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Serialize permission reads and cap consumption with revocations.
    /// Database failure rejects the delegation; no cached fallback is used.
    pub async fn authorize(
        &self,
        master: &UserId,
        agent: &UserId,
        notional: u64,
        now_ms: u64,
        order: Option<&OrderScopeCheck<'_>>,
    ) -> Result<(), AgentStoreError> {
        let mut tx = self.pool.begin().await?;
        self.lock(&mut tx, agent).await?;
        let delegation = self
            .read(&mut tx, agent)
            .await?
            .ok_or(AgentStoreError::Conflict("unknown agent"))?;
        if delegation.master != *master
            || delegation.revoked
            || delegation.expires_at_ms <= now_ms
            || notional > delegation.max_notional_per_order
        {
            return Err(AgentStoreError::Conflict(
                "delegation does not authorize request",
            ));
        }
        if let Some(order) = order {
            delegation
                .scope
                .check_order_static(order.market, order.side, order.order_type)
                .map_err(AgentStoreError::Conflict)?;
            for (seconds, cap) in [
                (3600i64, delegation.scope.max_notional_per_hour),
                (86400i64, delegation.scope.max_notional_per_day),
            ] {
                let Some(cap) = cap else { continue };
                let bucket_id = now_ms / 1000 / seconds as u64;
                let used: Option<String> = sqlx::query_scalar("SELECT notional::text FROM api_agent_notional_buckets WHERE namespace=$1 AND agent=$2 AND bucket_seconds=$3 AND bucket_id=$4::text::numeric")
                    .bind(&self.namespace).bind(agent.to_hex()).bind(seconds).bind(bucket_id.to_string())
                    .fetch_optional(&mut *tx).await?;
                let used: u128 = used
                    .as_deref()
                    .unwrap_or("0")
                    .parse()
                    .map_err(|_| AgentStoreError::InvalidRecord)?;
                let next = used + u128::from(notional);
                if next > u128::from(cap) {
                    return Err(AgentStoreError::Conflict("delegated notional cap exceeded"));
                }
                sqlx::query("INSERT INTO api_agent_notional_buckets (namespace,agent,bucket_seconds,bucket_id,notional) VALUES ($1,$2,$3,$4::text::numeric,$5::text::numeric) ON CONFLICT (namespace,agent,bucket_seconds,bucket_id) DO UPDATE SET notional=EXCLUDED.notional")
                    .bind(&self.namespace).bind(agent.to_hex()).bind(seconds).bind(bucket_id.to_string()).bind(next.to_string())
                    .execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

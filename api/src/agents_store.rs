//! PostgreSQL authority for delegated-agent permissions. Every operation
//! locks the same namespace/agent key, including the first registration.

use crate::agents::{AgentDelegation, CapabilityScope};
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
}

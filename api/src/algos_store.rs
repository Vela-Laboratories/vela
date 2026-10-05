//! Durable TWAP intents. Uncertain children are never blindly retried.
use crate::{algo_request::TwapRequest, algos::AlgoStatus};
use ledger::{assert_engine_leadership_in_transaction, EngineLeaderToken};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use types::UserId;

#[derive(Debug, thiserror::Error)]
pub enum AlgoStoreError {
    #[error("algorithm storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("algorithm leadership unavailable")]
    Leadership(#[from] ledger::LedgerError),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("invalid stored algorithm")]
    Invalid,
}
#[derive(Clone)]
pub struct AlgoStore {
    pool: PgPool,
    namespace: String,
    token: EngineLeaderToken,
}
#[derive(Debug, Clone, FromRow)]
struct ParentRow {
    id: i64,
    signer: String,
    request: serde_json::Value,
    quantity_tick: String,
    started_at_ms: String,
    filled_quantity: String,
    next_slice: i32,
    cancel_requested: bool,
    status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurableParent {
    pub id: u64,
    pub signer: String,
    pub request: TwapRequest,
    pub quantity_tick: u64,
    pub started_at_ms: u64,
    pub filled_quantity: u64,
    pub next_slice: u32,
    pub cancel_requested: bool,
    pub status: AlgoStatus,
}
impl TryFrom<ParentRow> for DurableParent {
    type Error = AlgoStoreError;
    fn try_from(r: ParentRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: r.id.try_into().map_err(|_| AlgoStoreError::Invalid)?,
            signer: r.signer,
            request: serde_json::from_value(r.request).map_err(|_| AlgoStoreError::Invalid)?,
            quantity_tick: r
                .quantity_tick
                .parse()
                .map_err(|_| AlgoStoreError::Invalid)?,
            started_at_ms: r
                .started_at_ms
                .parse()
                .map_err(|_| AlgoStoreError::Invalid)?,
            filled_quantity: r
                .filled_quantity
                .parse()
                .map_err(|_| AlgoStoreError::Invalid)?,
            next_slice: r
                .next_slice
                .try_into()
                .map_err(|_| AlgoStoreError::Invalid)?,
            cancel_requested: r.cancel_requested,
            status: serde_json::from_value(serde_json::Value::String(r.status))
                .map_err(|_| AlgoStoreError::Invalid)?,
        })
    }
}
const SELECT: &str = "SELECT id,signer,request,quantity_tick::text,started_at_ms::text,filled_quantity::text,next_slice,cancel_requested,status FROM api_algo_parents";
impl AlgoStore {
    pub fn new(pool: PgPool, namespace: String, token: EngineLeaderToken) -> Self {
        Self {
            pool,
            namespace,
            token,
        }
    }
    async fn transaction(&self) -> Result<Transaction<'_, Postgres>, AlgoStoreError> {
        let mut tx = self.pool.begin().await?;
        assert_engine_leadership_in_transaction(&mut tx, self.token).await?;
        Ok(tx)
    }
    async fn event(
        tx: &mut Transaction<'_, Postgres>,
        id: i64,
        action: &str,
        detail: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO api_algo_events(parent_id,action,detail) VALUES ($1,$2,$3)")
            .bind(id)
            .bind(action)
            .bind(detail)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
    pub async fn admit(
        &self,
        request: &TwapRequest,
        signer: &UserId,
        tick: u64,
        now_ms: u64,
    ) -> Result<DurableParent, AlgoStoreError> {
        request.validate().map_err(AlgoStoreError::Conflict)?;
        let mut tx = self.transaction().await?;
        let owner = request.address.to_ascii_lowercase();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("{}:algo:{}", self.namespace, owner))
            .execute(&mut *tx)
            .await?;
        let end = request.nonce + u64::from(request.slice_count());
        let overlap: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_algo_parents WHERE namespace=$1 AND owner=$2 AND nonce <= $3::text::numeric AND nonce_end >= $4::text::numeric)")
            .bind(&self.namespace).bind(&owner).bind(end.to_string()).bind(request.nonce.to_string()).fetch_one(&mut *tx).await?;
        if overlap {
            return Err(AlgoStoreError::Conflict(
                "algorithm nonce range already reserved",
            ));
        }
        let payload = serde_json::to_value(request).map_err(|_| AlgoStoreError::Invalid)?;
        let id: i64 = sqlx::query_scalar("INSERT INTO api_algo_parents(namespace,owner,signer,request,quantity_tick,started_at_ms,nonce,nonce_end) VALUES ($1,$2,$3,$4,$5::text::numeric,$6::text::numeric,$7::text::numeric,$8::text::numeric) RETURNING id")
            .bind(&self.namespace).bind(owner).bind(signer.to_hex()).bind(payload).bind(tick.to_string()).bind(now_ms.to_string()).bind(request.nonce.to_string()).bind(end.to_string()).fetch_one(&mut *tx).await?;
        Self::event(&mut tx,id,"admit",serde_json::json!({"signing_message":String::from_utf8_lossy(&request.signing_message())})).await?;
        tx.commit().await?;
        self.get(id as u64).await?.ok_or(AlgoStoreError::Invalid)
    }
    pub async fn get(&self, id: u64) -> Result<Option<DurableParent>, AlgoStoreError> {
        let id = i64::try_from(id).map_err(|_| AlgoStoreError::Invalid)?;
        sqlx::query_as::<_, ParentRow>(&format!("{SELECT} WHERE namespace=$1 AND id=$2"))
            .bind(&self.namespace)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(DurableParent::try_from)
            .transpose()
    }
    pub async fn list(&self, owner: Option<&str>) -> Result<Vec<DurableParent>, AlgoStoreError> {
        let rows = sqlx::query_as::<_, ParentRow>(&format!(
            "{SELECT} WHERE namespace=$1 AND ($2::text IS NULL OR owner=$2) ORDER BY id DESC"
        ))
        .bind(&self.namespace)
        .bind(owner.map(str::to_ascii_lowercase))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(DurableParent::try_from).collect()
    }
}

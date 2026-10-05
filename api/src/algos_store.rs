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
        if tick == 0 || !request.quantity.is_multiple_of(tick) {
            return Err(AlgoStoreError::Conflict("invalid lot size"));
        }
        if request.quantity / tick < u64::from(request.slice_count()) {
            return Err(AlgoStoreError::Conflict("not enough lots for schedule"));
        }
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
    pub async fn start_child(
        &self,
        id: u64,
        index: u32,
        quantity: u64,
    ) -> Result<bool, AlgoStoreError> {
        let mut tx = self.transaction().await?;
        let row = sqlx::query_as::<_, ParentRow>(&format!(
            "{SELECT} WHERE namespace=$1 AND id=$2 FOR UPDATE"
        ))
        .bind(&self.namespace)
        .bind(id as i64)
        .fetch_one(&mut *tx)
        .await?;
        let parent = DurableParent::try_from(row)?;
        if parent.status != AlgoStatus::Running || parent.cancel_requested {
            return Ok(false);
        }
        if parent.next_slice != index || index >= parent.request.slice_count() {
            return Err(AlgoStoreError::Conflict("unexpected child index"));
        }
        let units = parent.request.quantity / parent.quantity_tick;
        let expected = (units / u64::from(parent.request.slice_count())
            + u64::from(u64::from(index) < units % u64::from(parent.request.slice_count())))
            * parent.quantity_tick;
        if quantity != expected || quantity == 0 {
            return Err(AlgoStoreError::Conflict(
                "child quantity differs from signed schedule",
            ));
        }
        let result = sqlx::query("INSERT INTO api_algo_children(parent_id,slice_index,client_order_id,quantity,state) VALUES ($1,$2,$3,$4::text::numeric,'started') ON CONFLICT (parent_id,slice_index) DO NOTHING")
            .bind(id as i64).bind(index as i32).bind(child_client_id(id,index)).bind(quantity.to_string()).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            return Err(AlgoStoreError::Conflict("child intent already dispatched"));
        }
        Self::event(
            &mut tx,
            id as i64,
            "child_started",
            serde_json::json!({"index":index,"quantity":quantity}),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn complete_child(
        &self,
        id: u64,
        index: u32,
        filled: u64,
        response_hash: &[u8; 32],
    ) -> Result<DurableParent, AlgoStoreError> {
        let mut tx = self.transaction().await?;
        let row = sqlx::query_as::<_, ParentRow>(&format!(
            "{SELECT} WHERE namespace=$1 AND id=$2 FOR UPDATE"
        ))
        .bind(&self.namespace)
        .bind(id as i64)
        .fetch_one(&mut *tx)
        .await?;
        let parent = DurableParent::try_from(row)?;
        let (state, previous, hash): (String,Option<String>,Option<Vec<u8>>) = sqlx::query_as("SELECT state,filled_quantity::text,response_hash FROM api_algo_children WHERE parent_id=$1 AND slice_index=$2 FOR UPDATE")
            .bind(id as i64).bind(index as i32).fetch_one(&mut *tx).await?;
        if state == "complete" {
            if previous.as_deref() != Some(filled.to_string().as_str())
                || hash.as_deref() != Some(response_hash.as_slice())
            {
                return Err(AlgoStoreError::Conflict("completed child outcome differs"));
            }
            return Ok(parent);
        }
        if parent.next_slice != index {
            return Err(AlgoStoreError::Conflict(
                "child progress is out of sequence",
            ));
        }
        let total = parent
            .filled_quantity
            .checked_add(filled)
            .filter(|v| *v <= parent.request.quantity)
            .ok_or(AlgoStoreError::Invalid)?;
        sqlx::query("UPDATE api_algo_children SET state='complete',filled_quantity=$3::text::numeric,response_hash=$4,updated_at=now() WHERE parent_id=$1 AND slice_index=$2")
            .bind(id as i64).bind(index as i32).bind(filled.to_string()).bind(response_hash.as_slice()).execute(&mut *tx).await?;
        let next = index + 1;
        let status = if parent.cancel_requested {
            "Canceled"
        } else if total == parent.request.quantity {
            "Completed"
        } else if next >= parent.request.slice_count() {
            "Incomplete"
        } else {
            "Running"
        };
        sqlx::query("UPDATE api_algo_parents SET filled_quantity=$2::text::numeric,next_slice=$3,status=$4,updated_at=now() WHERE id=$1")
            .bind(id as i64).bind(total.to_string()).bind(next as i32).bind(status).execute(&mut *tx).await?;
        Self::event(&mut tx,id as i64,"child_complete",serde_json::json!({"index":index,"filled_quantity":filled,"response_hash":hex::encode(response_hash)})).await?;
        tx.commit().await?;
        self.get(id).await?.ok_or(AlgoStoreError::Invalid)
    }
}

/// Valid engine client IDs bind each persisted intent to one child request.
pub fn child_client_id(parent: u64, index: u32) -> String {
    format!("twap-{parent}-{index}")
}

impl AlgoStore {
    pub async fn cancel(
        &self,
        id: u64,
        owner: &str,
        proof: serde_json::Value,
    ) -> Result<bool, AlgoStoreError> {
        let mut tx = self.transaction().await?;
        let changed = sqlx::query("UPDATE api_algo_parents SET cancel_requested=TRUE,updated_at=now() WHERE namespace=$1 AND id=$2 AND owner=$3 AND status IN ('Running','RecoveryRequired') AND NOT cancel_requested")
            .bind(&self.namespace).bind(id as i64).bind(owner.to_ascii_lowercase()).execute(&mut *tx).await?.rows_affected() == 1;
        if changed {
            Self::event(&mut tx, id as i64, "cancel", proof).await?;
        }
        tx.commit().await?;
        Ok(changed)
    }
    pub async fn finish(
        &self,
        id: u64,
        status: AlgoStatus,
        reason: &str,
    ) -> Result<(), AlgoStoreError> {
        if status == AlgoStatus::Running {
            return Err(AlgoStoreError::Conflict("terminal status required"));
        }
        let mut tx = self.transaction().await?;
        let status = serde_json::to_value(status).map_err(|_| AlgoStoreError::Invalid)?;
        let changed = sqlx::query("UPDATE api_algo_parents SET status=$3,updated_at=now() WHERE namespace=$1 AND id=$2 AND status IN ('Running','RecoveryRequired')")
            .bind(&self.namespace).bind(id as i64).bind(status.as_str().ok_or(AlgoStoreError::Invalid)?).execute(&mut *tx).await?.rows_affected();
        if changed == 1 {
            Self::event(
                &mut tx,
                id as i64,
                "terminal",
                serde_json::json!({"status":status,"reason":reason}),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn mark_uncertain(&self, id: u64, index: u32) -> Result<(), AlgoStoreError> {
        let mut tx = self.transaction().await?;
        sqlx::query("UPDATE api_algo_children SET state='indeterminate',updated_at=now() WHERE parent_id=$1 AND slice_index=$2 AND state='started'")
            .bind(id as i64).bind(index as i32).execute(&mut *tx).await?;
        sqlx::query("UPDATE api_algo_parents SET status='RecoveryRequired',updated_at=now() WHERE namespace=$1 AND id=$2 AND status='Running'")
            .bind(&self.namespace).bind(id as i64).execute(&mut *tx).await?;
        Self::event(
            &mut tx,
            id as i64,
            "child_indeterminate",
            serde_json::json!({"index":index}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

impl AlgoStore {
    /// Resolve only outcomes bound to immutable financial receipt evidence.
    /// A missing receipt never authorizes repeating a dispatched child.
    pub async fn recover(&self) -> Result<Vec<DurableParent>, AlgoStoreError> {
        use sha2::{Digest, Sha256};
        for parent in self.list(None).await? {
            if !matches!(
                parent.status,
                AlgoStatus::Running | AlgoStatus::RecoveryRequired
            ) {
                continue;
            }
            let pending: Vec<(i32,String)> = sqlx::query_as("SELECT slice_index,client_order_id FROM api_algo_children WHERE parent_id=$1 AND state<>'complete' ORDER BY slice_index")
                .bind(parent.id as i64).fetch_all(&self.pool).await?;
            for (index, client_id) in pending {
                let receipts: Vec<serde_json::Value> = sqlx::query_scalar("SELECT metadata FROM ledger_journal_transactions WHERE jsonb_path_exists(metadata, '$.effect_set.response_commitments[*].OrderPosted ? (@.client_order_id == $client_id)', jsonb_build_object('client_id',$1::text))")
                    .bind(&client_id).fetch_all(&self.pool).await?;
                if receipts.len() != 1 {
                    self.mark_uncertain(parent.id, index as u32).await?;
                    continue;
                }
                let effects: engine::FinancialEffectSet =
                    serde_json::from_value(receipts[0]["effect_set"].clone())
                        .map_err(|_| AlgoStoreError::Invalid)?;
                let ids: Vec<_> = effects
                    .response_commitments
                    .iter()
                    .filter_map(|response| match response {
                        types::Response::OrderPosted(order)
                            if order.client_order_id.as_deref() == Some(&client_id) =>
                        {
                            Some(order.order_id)
                        }
                        _ => None,
                    })
                    .collect();
                if ids.len() != 1 {
                    return Err(AlgoStoreError::Invalid);
                }
                let filled = effects
                    .fills
                    .iter()
                    .filter(|fill| fill.taker_order_id == ids[0])
                    .try_fold(0u64, |total, fill| {
                        total
                            .checked_add(fill.quantity)
                            .ok_or(AlgoStoreError::Invalid)
                    })?;
                let hash: [u8; 32] = Sha256::digest(
                    serde_json::to_vec(&receipts[0]).map_err(|_| AlgoStoreError::Invalid)?,
                )
                .into();
                self.complete_child(parent.id, index as u32, filled, &hash)
                    .await?;
            }
        }
        self.list(None).await
    }
}

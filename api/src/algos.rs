//! Server-side execution algorithms.
//!
//! v1 ships **TWAP** as the first native algo. Every serious venue offers
//! server-side TWAP (Hyperliquid, Kraken, Binance) because client-side
//! slicing loops are fragile — a disconnected client's TWAP dies mid-run,
//! and every child order pays a round-trip signature. Native TWAP also
//! gives Vela's toxicity scorer visibility into child slices, which is a
//! differentiator: we can toxicity-score the parent algo as a coherent
//! flow instead of scoring each unrelated child fill in isolation.
//!
//! Design
//! ------
//! - The client submits a **parent** with total quantity, direction,
//!   duration, and a required price limit. The parent has a stable
//!   `parent_id` (u64) the client can query and cancel by.
//! - A tokio task is spawned per parent. It divides `total_quantity`
//!   into `slices` equal chunks (default 12; overridable via param)
//!   and schedules absolute monotonic deadlines for each slice. Each slice
//!   submits an **IOC** child order bounded
//!   by `price_limit`, and records the fill quantity into the parent.
//! - Cancel: `POST /orders/algo/cancel { parent_id }` flips an atomic
//!   flag and wakes the scheduler; the task observes it before the next slice and
//!   exits.
//! - v1 does **not** randomize slice timing. That's a follow-up. Any
//!   randomization must be documented and deterministic (seed-based)
//!   so users can reason about behavior.
//! - v1 does **not** run VWAP, scale orders, or stops. Those come in
//!   later commits on this same substrate.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use types::{OrderSide, OrderType, PostOrderRequest};

use crate::AppState;

/// Tracks schedulers through shutdown without canceling their durable parents.
#[derive(Default)]
pub struct AlgoTasks {
    stopped: std::sync::Mutex<bool>,
    cancellation: tokio_util::sync::CancellationToken,
    tracker: tokio_util::task::TaskTracker,
}
impl AlgoTasks {
    pub fn spawn(&self, state: Arc<AppState>, parent: Arc<TwapParent>) {
        let stopped = self.stopped.lock().unwrap();
        if !*stopped {
            self.tracker.spawn(run_twap_task(state, parent));
        }
    }
    pub fn is_stopping(&self) -> bool {
        self.cancellation.is_cancelled()
    }
    pub async fn drain(&self, timeout: Duration) -> bool {
        {
            let mut stopped = self.stopped.lock().unwrap();
            *stopped = true;
            self.cancellation.cancel();
            self.tracker.close();
        }
        tokio::time::timeout(timeout, self.tracker.wait())
            .await
            .is_ok()
    }
}

/// Reserve a signed parent nonce atomically. Entries live for the process lifetime.
pub fn reserve_parent_nonce(
    state: &AppState,
    address: &str,
    nonce: u64,
) -> Result<(), &'static str> {
    match state
        .algo_nonces
        .entry((address.to_ascii_lowercase(), nonce))
    {
        dashmap::mapref::entry::Entry::Occupied(_) => Err("TWAP parent nonce already used"),
        dashmap::mapref::entry::Entry::Vacant(entry) => {
            entry.insert(());
            Ok(())
        }
    }
}

/// Monotonic parent-id source. Cheap and lock-free.
pub static NEXT_PARENT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_parent_id() -> u64 {
    NEXT_PARENT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Use the same reserved notional at admission and on every continuation.
pub fn authorized_parent_notional(price_limit: Option<u64>, quantity: u64) -> u64 {
    price_limit
        .map(|price| crate::agents::order_notional_micro(price, quantity))
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlgoStatus {
    Running,
    Completed,
    Incomplete,
    Canceled,
    Failed,
    RecoveryRequired,
}

#[derive(Debug)]
pub struct TwapParent {
    pub parent_id: u64,
    pub authorized_signer: types::UserId,
    pub user_address: String,
    pub market: String,
    pub side: OrderSide,
    /// Total quantity to work over the duration.
    pub total_quantity: u64,
    /// Filled so far across all child slices.
    pub filled_quantity: AtomicU64,
    /// Optional worst-price bound. `None` = execute at any price.
    pub price_limit: Option<u64>,
    pub duration_secs: u64,
    pub slices: u32,
    pub next_slice: AtomicU32,
    pub quantity_tick: u64,
    pub started_at_ms: u64,
    pub status: std::sync::Mutex<AlgoStatus>,
    /// Set by `cancel_algo` to signal the slicer to exit before its next
    /// slice.
    pub cancel_flag: AtomicBool,
    pub cancel_notify: tokio::sync::Notify,
    /// Base nonce for the child order sequence. Slice N uses
    /// `child_nonce_base + N` so all children carry unique nonces.
    pub child_nonce_base: u64,
    /// Signature-less internal orders. The parent authorization was
    /// checked at submission time; child orders bypass signature
    /// verification (submitted through the internal API path, not the
    /// public one). See `submit_child_order` for the internal path.
    pub _phantom_signature: (),
}

impl TwapParent {
    pub fn from_durable(
        parent: &crate::algos_store::DurableParent,
    ) -> Result<Arc<Self>, crate::algos_store::AlgoStoreError> {
        use crate::algos_store::AlgoStoreError;
        parent
            .request
            .validate()
            .map_err(AlgoStoreError::Conflict)?;
        if parent.quantity_tick == 0 {
            return Err(AlgoStoreError::Invalid);
        }
        Ok(Arc::new(Self {
            parent_id: parent.id,
            authorized_signer: types::UserId::from_hex(&parent.signer)
                .map_err(|_| AlgoStoreError::Invalid)?,
            user_address: parent.request.address.to_ascii_lowercase(),
            market: parent.request.market.clone(),
            side: parent.request.side,
            total_quantity: parent.request.quantity,
            filled_quantity: AtomicU64::new(parent.filled_quantity),
            price_limit: parent.request.price_limit,
            duration_secs: parent.request.duration_secs,
            slices: parent.request.slice_count(),
            next_slice: AtomicU32::new(parent.next_slice),
            quantity_tick: parent.quantity_tick,
            started_at_ms: parent.started_at_ms,
            status: std::sync::Mutex::new(parent.status),
            cancel_flag: AtomicBool::new(parent.cancel_requested),
            cancel_notify: tokio::sync::Notify::new(),
            child_nonce_base: parent.request.nonce + 1,
            _phantom_signature: (),
        }))
    }

    pub fn publish_durable(&self, parent: &crate::algos_store::DurableParent) {
        self.filled_quantity
            .store(parent.filled_quantity, Ordering::Release);
        self.next_slice.store(parent.next_slice, Ordering::Release);
        if parent.cancel_requested {
            self.cancel_flag.store(true, Ordering::Release);
        }
        *self.status.lock().unwrap() = parent.status;
    }

    pub fn request_cancel(&self) -> bool {
        let status = self.status.lock().unwrap();
        if *status != AlgoStatus::Running {
            return false;
        }
        self.cancel_flag.store(true, Ordering::Release);
        self.cancel_notify.notify_one();
        true
    }

    pub fn snapshot_status(&self) -> AlgoStatus {
        *self.status.lock().unwrap()
    }
}

/// A snapshot of TwapParent state safe to serialize via the HTTP API.
#[derive(Debug, Serialize)]
pub struct TwapParentSnapshot {
    pub parent_id: u64,
    pub user: String,
    pub market: String,
    pub side: String,
    pub total_quantity: u64,
    pub filled_quantity: u64,
    pub remaining_quantity: u64,
    pub price_limit: Option<u64>,
    pub duration_secs: u64,
    pub slices: u32,
    pub started_at_ms: u64,
    pub status: AlgoStatus,
}

impl TwapParentSnapshot {
    pub fn from(p: &TwapParent) -> Self {
        Self {
            parent_id: p.parent_id,
            user: p.user_address.clone(),
            market: p.market.clone(),
            side: format!("{:?}", p.side).to_lowercase(),
            total_quantity: p.total_quantity,
            filled_quantity: p.filled_quantity.load(Ordering::Relaxed),
            remaining_quantity: p
                .total_quantity
                .saturating_sub(p.filled_quantity.load(Ordering::Relaxed)),
            price_limit: p.price_limit,
            duration_secs: p.duration_secs,
            slices: p.slices,
            started_at_ms: p.started_at_ms,
            status: p.snapshot_status(),
        }
    }
}

/// Parent registry shared through AppState. DashMap keyed by parent_id.
pub type AlgoRegistry = Arc<DashMap<u64, Arc<TwapParent>>>;

/// Submit a single child order through the internal dispatcher, bypassing
/// public signature verification. The parent's ownership check happened
/// when the parent was created; child orders inherit that authorization.
///
/// Returns the filled quantity from the response so the parent can
/// accumulate `filled_quantity`.
#[derive(Debug)]
struct ChildOutcome {
    filled: u64,
    hash: [u8; 32],
    rejected: bool,
}

#[allow(clippy::too_many_arguments)]
async fn submit_child_order(
    state: &AppState,
    user: types::UserId,
    market: types::MarketId,
    side: OrderSide,
    price: u64,
    quantity: u64,
    nonce: u64,
    client_order_id: Option<String>,
) -> Result<ChildOutcome, &'static str> {
    let req = PostOrderRequest {
        user,
        market,
        side,
        // IOC so unfilled portion cancels; parent accumulates the miss
        // by not incrementing filled_quantity to total on that slice.
        order_type: OrderType::ImmediateOrCancel,
        price,
        quantity,
        nonce,
        client_order_id,
        signature: vec![],
        stp: Default::default(),
        min_quantity: None,
        display_quantity: None,
    };

    let (responder, resp_rx) = tokio::sync::oneshot::channel();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let channel_item = engine::batch_dispatcher::BatchedRequest {
        request: types::Request::PostOrder(req),
        ts,
        responder,
        decryption_proof: None,
    };
    tokio::time::timeout(
        Duration::from_millis(500),
        state.order_tx.send(channel_item),
    )
    .await
    .map_err(|_| "child queue timeout")?
    .map_err(|_| "child dispatcher closed")?;
    let responses = tokio::time::timeout(Duration::from_secs(5), resp_rx)
        .await
        .map_err(|_| "child outcome indeterminate")?
        .map_err(|_| "child response dropped")?;
    summarize_child(&responses)
}

fn summarize_child(responses: &[types::Response]) -> Result<ChildOutcome, &'static str> {
    use sha2::{Digest, Sha256};
    let rejected = responses
        .iter()
        .any(|r| matches!(r, types::Response::Error(_)));
    if responses.iter().any(|r| matches!(r,types::Response::Error(error) if error.code == types::ErrorCode::InternalError)) {
        return Err("child outcome indeterminate");
    }
    let filled = responses
        .iter()
        .try_fold(0u64, |total, response| match response {
            types::Response::OrderFilled(fill) => total
                .checked_add(fill.quantity)
                .ok_or("child fill quantity overflow"),
            _ => Ok(total),
        })?;
    if rejected && filled > 0 {
        return Err("child outcome indeterminate");
    }
    let encoded = serde_json::to_vec(responses).map_err(|_| "invalid child response")?;
    Ok(ChildOutcome {
        filled,
        hash: Sha256::digest(encoded).into(),
        rejected,
    })
}

/// Long-running task: slices the parent's total quantity into `slices`
/// child orders and submits them at regular intervals until the parent
/// is complete, canceled, or its duration elapses.
///
/// Marks the parent's status to `Completed`/`Canceled` on exit. On any
/// internal error (channel closed, user id parse fail) the status
/// flips to `Failed` and the task exits.
pub async fn run_twap_task(state: Arc<AppState>, parent: Arc<TwapParent>) {
    let user_id = match types::UserId::from_hex(&parent.user_address) {
        Ok(user) => user,
        Err(_) => {
            finish_parent(&state, &parent, AlgoStatus::Failed, "invalid owner").await;
            return;
        }
    };
    let market = types::MarketId(parent.market.clone());
    let now_ms = wall_time_ms();
    let instant = tokio::time::Instant::now();
    let started = if now_ms >= parent.started_at_ms {
        instant
            .checked_sub(Duration::from_millis(now_ms - parent.started_at_ms))
            .unwrap_or(instant)
    } else {
        instant + Duration::from_millis(parent.started_at_ms - now_ms)
    };
    let deadline = started + Duration::from_secs(parent.duration_secs);
    let units = parent.total_quantity / parent.quantity_tick;
    let count = u64::from(parent.slices);
    let first = parent.next_slice.load(Ordering::Acquire);
    for index in first..parent.slices {
        if state.algo_tasks.is_stopping() {
            return;
        }
        let offset = parent.duration_secs * 1_000 * u64::from(index) / count;
        tokio::select! {
            biased;
            _ = state.algo_tasks.cancellation.cancelled() => return,
            _ = tokio::time::sleep_until(started+Duration::from_millis(offset)) => {},
            _ = parent.cancel_notify.notified() => {},
        }
        if state.algo_tasks.is_stopping() || !state.storage_health.is_healthy() {
            return;
        }
        if let Some(store) = &state.algo_store {
            match store.get(parent.parent_id).await {
                Ok(Some(row)) => parent.publish_durable(&row),
                _ => {
                    fence_parent(&state, &parent, "algorithm read failed");
                    return;
                }
            }
            if parent.snapshot_status() != AlgoStatus::Running {
                return;
            }
        }
        if parent.cancel_flag.load(Ordering::Acquire) {
            finish_parent(&state, &parent, AlgoStatus::Canceled, "owner canceled").await;
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        if state
            .agents
            .authorize_continuation(
                &user_id,
                &parent.authorized_signer,
                authorized_parent_notional(parent.price_limit, parent.total_quantity),
                wall_time_ms(),
                &market,
                parent.side,
            )
            .await
            .is_err()
        {
            finish_parent(
                &state,
                &parent,
                AlgoStatus::Canceled,
                "delegated authority unavailable",
            )
            .await;
            return;
        }
        if state.algo_tasks.is_stopping() {
            return;
        }
        let quantity =
            (units / count + u64::from(u64::from(index) < units % count)) * parent.quantity_tick;
        if let Some(store) = &state.algo_store {
            match store.start_child(parent.parent_id, index, quantity).await {
                Ok(true) => {}
                Ok(false) => return,
                Err(_) => {
                    fence_parent(&state, &parent, "child intent could not be recorded");
                    return;
                }
            }
        }
        let price = parent.price_limit.unwrap_or(match parent.side {
            OrderSide::Bid => u64::MAX / 2,
            OrderSide::Ask => 1,
        });
        let outcome = submit_child_order(
            &state,
            user_id.clone(),
            market.clone(),
            parent.side,
            price,
            quantity,
            parent.child_nonce_base + u64::from(index),
            Some(crate::algos_store::child_client_id(parent.parent_id, index)),
        )
        .await;
        let outcome = match outcome {
            Ok(outcome) if outcome.filled <= quantity => outcome,
            _ => {
                if let Some(store) = &state.algo_store {
                    if store.mark_uncertain(parent.parent_id, index).await.is_err() {
                        fence_parent(&state, &parent, "uncertain child could not be persisted");
                    }
                    *parent.status.lock().unwrap() = AlgoStatus::RecoveryRequired;
                } else {
                    *parent.status.lock().unwrap() = AlgoStatus::Failed;
                }
                return;
            }
        };
        if let Some(store) = &state.algo_store {
            match store
                .complete_child(
                    parent.parent_id,
                    index,
                    outcome.filled,
                    &outcome.hash,
                    outcome.rejected,
                )
                .await
            {
                Ok(row) => parent.publish_durable(&row),
                Err(_) => {
                    fence_parent(&state, &parent, "child result could not be persisted");
                    return;
                }
            }
            if parent.snapshot_status() != AlgoStatus::Running {
                return;
            }
        } else {
            parent
                .filled_quantity
                .fetch_add(outcome.filled, Ordering::Relaxed);
            parent.next_slice.store(index + 1, Ordering::Release);
            if outcome.rejected {
                *parent.status.lock().unwrap() = AlgoStatus::Failed;
                return;
            }
        }
    }
    let status = if parent.cancel_flag.load(Ordering::Acquire) {
        AlgoStatus::Canceled
    } else if parent.filled_quantity.load(Ordering::Acquire) == parent.total_quantity {
        AlgoStatus::Completed
    } else {
        AlgoStatus::Incomplete
    };
    finish_parent(&state, &parent, status, "schedule ended").await;
}
fn wall_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn fence_parent(state: &AppState, parent: &TwapParent, reason: &str) {
    state.storage_health.fence("algorithm_storage", reason);
    *parent.status.lock().unwrap() = AlgoStatus::RecoveryRequired;
}
async fn finish_parent(state: &AppState, parent: &TwapParent, status: AlgoStatus, reason: &str) {
    if let Some(store) = &state.algo_store {
        if store
            .finish(parent.parent_id, status, reason)
            .await
            .is_err()
        {
            fence_parent(state, parent, "terminal outcome could not be persisted");
            return;
        }
    }
    *parent.status.lock().unwrap() = status;
}

pub async fn admit_parent(
    state: &AppState,
    request: &crate::algo_request::TwapRequest,
    signer: types::UserId,
    tick: u64,
    now_ms: u64,
) -> Result<Arc<TwapParent>, crate::algos_store::AlgoStoreError> {
    use crate::algos_store::AlgoStoreError;
    if state.algo_tasks.is_stopping() {
        return Err(AlgoStoreError::Conflict("execution is shutting down"));
    }
    if let Some(store) = &state.algo_store {
        return TwapParent::from_durable(&store.admit(request, &signer, tick, now_ms).await?);
    }
    if state.ledger.is_some() {
        return Err(AlgoStoreError::Conflict("durable execution unavailable"));
    }
    reserve_parent_nonce(state, &request.address, request.nonce)
        .map_err(AlgoStoreError::Conflict)?;
    Ok(Arc::new(TwapParent {
        parent_id: next_parent_id(),
        authorized_signer: signer,
        user_address: request.address.to_ascii_lowercase(),
        market: request.market.clone(),
        side: request.side,
        total_quantity: request.quantity,
        filled_quantity: AtomicU64::new(0),
        price_limit: request.price_limit,
        duration_secs: request.duration_secs,
        slices: request.slice_count(),
        next_slice: AtomicU32::new(0),
        quantity_tick: tick,
        started_at_ms: now_ms,
        status: std::sync::Mutex::new(AlgoStatus::Running),
        cancel_flag: AtomicBool::new(false),
        cancel_notify: tokio::sync::Notify::new(),
        child_nonce_base: request.nonce + 1,
        _phantom_signature: (),
    }))
}

pub async fn load_parent(
    state: &AppState,
    id: u64,
) -> Result<Option<Arc<TwapParent>>, crate::algos_store::AlgoStoreError> {
    if let Some(store) = &state.algo_store {
        return store
            .get(id)
            .await?
            .map(|row| TwapParent::from_durable(&row))
            .transpose();
    }
    Ok(state.algos.get(&id).map(|p| Arc::clone(&p)))
}

/// Called before serving traffic, after ledger and engine recovery admission.
pub async fn recover_durable(
    state: Arc<AppState>,
) -> Result<(), crate::algos_store::AlgoStoreError> {
    let Some(store) = &state.algo_store else {
        return Ok(());
    };
    let parents = store
        .recover()
        .await?
        .iter()
        .filter(|row| row.status == AlgoStatus::Running)
        .map(TwapParent::from_durable)
        .collect::<Result<Vec<_>, _>>()?;
    for parent in parents {
        state.algos.insert(parent.parent_id, Arc::clone(&parent));
        state.algo_tasks.spawn(Arc::clone(&state), parent);
    }
    Ok(())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn continuation_uses_admission_notional_and_fails_closed_without_price() {
        assert_eq!(
            authorized_parent_notional(Some(2_000_000), 3_000_000),
            6_000_000
        );
        assert_eq!(authorized_parent_notional(None, 3_000_000), u64::MAX);
        assert_eq!(
            authorized_parent_notional(Some(u64::MAX), u64::MAX),
            u64::MAX
        );
    }

    #[tokio::test]
    async fn shutdown_wakes_waiting_schedules_and_waits_for_inflight_completion() {
        let tasks = AlgoTasks::default();
        let cancellation = tasks.cancellation.clone();
        let completed = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&completed);
        tasks.tracker.spawn(async move {
            cancellation.cancelled().await;
            // Model a child already submitted: its result must publish before drain finishes.
            tokio::task::yield_now().await;
            observed.store(true, Ordering::Release);
        });
        assert!(tasks.drain(Duration::from_secs(1)).await);
        assert!(completed.load(Ordering::Acquire));
        assert!(tasks.is_stopping());
        assert!(*tasks.stopped.lock().unwrap());
    }

    #[tokio::test]
    async fn undrained_child_prevents_clean_shutdown() {
        let tasks = AlgoTasks::default();
        let handle = tasks.tracker.spawn(std::future::pending::<()>());
        assert!(!tasks.drain(Duration::from_millis(10)).await);
        handle.abort();
        assert!(tasks.drain(Duration::from_secs(1)).await);
    }
}

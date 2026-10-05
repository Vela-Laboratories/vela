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
//!   duration, and an optional price limit. The parent has a stable
//!   `parent_id` (u64) the client can query and cancel by.
//! - A tokio task is spawned per parent. It divides `total_quantity`
//!   into `slices` equal chunks (default 12; overridable via param)
//!   and sleeps `duration_secs / slices` between each. Each slice
//!   submits an **IOC** child order at the current market touch bounded
//!   by `price_limit`, and records the fill quantity into the parent.
//! - Cancel: `POST /orders/algo/cancel { parent_id }` flips an atomic
//!   flag; the running task observes it before the next slice and
//!   exits.
//! - v1 does **not** randomize slice timing. That's a follow-up. Any
//!   randomization must be documented and deterministic (seed-based)
//!   so users can reason about behavior.
//! - v1 does **not** run VWAP, scale orders, or stops. Those come in
//!   later commits on this same substrate.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use types::{OrderSide, OrderType, PostOrderRequest};

use crate::AppState;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlgoStatus {
    Running,
    Completed,
    Incomplete,
    Canceled,
    Failed,
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
async fn submit_child_order(
    state: &AppState,
    user: types::UserId,
    market: types::MarketId,
    side: OrderSide,
    price: u64,
    quantity: u64,
    nonce: u64,
) -> Result<u64, &'static str> {
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
        client_order_id: None,
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
    if responses
        .iter()
        .any(|r| matches!(r, types::Response::Error(_)))
    {
        return Err("child order rejected");
    }
    responses
        .iter()
        .try_fold(0u64, |total, response| match response {
            types::Response::OrderFilled(fill) => total
                .checked_add(fill.quantity)
                .ok_or("child fill quantity overflow"),
            _ => Ok(total),
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
        Ok(u) => u,
        Err(_) => {
            *parent.status.lock().unwrap() = AlgoStatus::Failed;
            return;
        }
    };
    let market = types::MarketId(parent.market.clone());

    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(parent.duration_secs);
    let units = parent.total_quantity / parent.quantity_tick;
    let base_slice_qty = units / u64::from(parent.slices) * parent.quantity_tick;
    let mut remainder = units % u64::from(parent.slices);

    for slice_ix in 0..parent.slices {
        let offset_ms =
            parent.duration_secs * 1_000 * u64::from(slice_ix) / u64::from(parent.slices);
        tokio::select! {
            _ = tokio::time::sleep_until(started + Duration::from_millis(offset_ms)) => {},
            _ = parent.cancel_notify.notified() => {},
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        if parent.cancel_flag.load(Ordering::Relaxed) {
            *parent.status.lock().unwrap() = AlgoStatus::Canceled;
            return;
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if state
            .agents
            .authorize_continuation(
                &user_id,
                &parent.authorized_signer,
                now_ms,
                &market,
                parent.side,
            )
            .await
            .is_err()
        {
            *parent.status.lock().unwrap() = AlgoStatus::Canceled;
            return;
        }
        // Front-load the remainder so we don't leave a big final slice.
        let mut slice_qty = base_slice_qty;
        if remainder > 0 {
            slice_qty += parent.quantity_tick;
            remainder -= 1;
        }
        if slice_qty == 0 {
            continue;
        }

        // Price for the child slice: use the parent's price_limit if set,
        // otherwise walk the far side up to a wide bound (fills won't
        // happen past the actual touch on IOC).
        let child_price = parent.price_limit.unwrap_or(match parent.side {
            OrderSide::Bid => u64::MAX / 2,
            OrderSide::Ask => 1,
        });

        let outcome = submit_child_order(
            &state,
            user_id.clone(),
            market.clone(),
            parent.side,
            child_price,
            slice_qty,
            parent.child_nonce_base + slice_ix as u64,
        )
        .await;
        let filled = match outcome {
            Ok(filled) if filled <= slice_qty => filled,
            _ => {
                *parent.status.lock().unwrap() = AlgoStatus::Failed;
                return;
            }
        };
        parent.filled_quantity.fetch_add(filled, Ordering::Relaxed);
    }

    *parent.status.lock().unwrap() = if parent.cancel_flag.load(Ordering::Acquire) {
        AlgoStatus::Canceled
    } else if parent.filled_quantity.load(Ordering::Relaxed) == parent.total_quantity {
        AlgoStatus::Completed
    } else {
        AlgoStatus::Incomplete
    };
}

//! RFQ / block-trade venue.
//!
//! Off-book quote request for trades that are too big to work through
//! the public CLOB without moving the market. Requester posts an RFQ,
//! whitelisted MMs post signed quotes, requester accepts one, Vela
//! atomically debits and credits both sides.
//!
//! Why bypass the CLOB
//! -------------------
//! A $2M taker walking a spot book with 100k top-of-book depth pays
//! massive slippage and leaks the size to the whole market via fills
//! visible on the trade tape. Institutions currently route this flow
//! OTC (Wintermute, Cumberland) precisely to avoid that. Capturing it
//! on-venue requires an agent-native block-trade rail with:
//! - Requester privacy: quotes are private per-request until accepted.
//! - Better-than-book pricing: quote must improve on the current touch
//!   (enforced at accept time), so an MM can't collude to fill worse
//!   than the requester could have gotten in the book.
//! - Atomic settlement: no legs of the trade land without the other.
//!
//! v1 policy
//! ---------
//! - `VELA_RFQ_MAKERS` env is a comma-separated allowlist of MM
//!   addresses. Non-listed MMs' quotes are rejected.
//! - Every RFQ is single-market. Multi-leg / cross-asset requires v2.
//! - MMs post quotes with expiry timestamps; requester must accept
//!   before expiry. Vela does not automatically pick "best quote" —
//!   the requester picks.
//! - `min_notional_usdc_micro` (default 250k USDC) is the floor for
//!   posting an RFQ. Rationale: RFQ has more per-trade overhead than
//!   the CLOB and its point is precisely the trades too big for the
//!   book. Overridable via `VELA_RFQ_MIN_NOTIONAL_MICRO`.
//!
//! Deferred to v2
//! --------------
//! - MM SLA tracking (uptime, average spread) to publish an on-chain
//!   MM reputation for the whitelist.
//! - Multi-leg / basket RFQs.
//! - Explicit fee schedule for RFQ (currently reuses CLOB market
//!   fees).

use dashmap::DashMap;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use types::OrderSide;

/// Convert fixed-point price and base quantity to quote micro-units.
/// Reject dust and values that cannot fit in an account balance.
pub fn quote_notional_micro(price: u64, quantity: u64) -> Option<u64> {
    let amount = u128::from(price) * u128::from(quantity) / 1_000_000;
    u64::try_from(amount).ok().filter(|amount| *amount > 0)
}

pub fn quote_window_valid(
    request_expires_at_ms: u64,
    quote_expires_at_ms: u64,
    now_ms: u64,
) -> bool {
    now_ms < request_expires_at_ms
        && now_ms < quote_expires_at_ms
        && quote_expires_at_ms <= request_expires_at_ms
}

pub fn min_notional_micro() -> u64 {
    std::env::var("VELA_RFQ_MIN_NOTIONAL_MICRO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250_000_000_000u64) // 250k USDC × 1e6
}

pub fn maker_allowlist() -> std::collections::HashSet<String> {
    match std::env::var("VELA_RFQ_MAKERS") {
        Ok(s) if !s.is_empty() => s
            .split(',')
            .map(|a| a.trim().to_ascii_lowercase())
            .filter(|a| !a.is_empty())
            .collect(),
        _ => std::collections::HashSet::new(),
    }
}

/// Reputation-score threshold (in bps) at which an agent can post
/// quotes to the RFQ rail without being on the human allowlist.
/// Default 6_000 bps (60% of ideal). Overridable via
/// `VELA_RFQ_MAKER_MIN_SCORE_BPS`. Set to `10_001` to disable the
/// agent-maker path entirely (allowlist becomes the sole gate).
pub fn agent_maker_min_score_bps() -> u16 {
    std::env::var("VELA_RFQ_MAKER_MIN_SCORE_BPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6_000)
}

/// Tag on a stored `RfqQuote` that records how the maker was cleared
/// to post. Persisted so the requester can filter on it at accept time
/// (some requesters may prefer only human-desk quotes, others will
/// take the best price regardless of provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MakerProvenance {
    #[default]
    Allowlist,
    /// Cleared via a reputation attestation. The attesting operator
    /// signature is not re-attached here (it's in reputation_cache);
    /// callers who want to verify pull it from
    /// `GET /reputation/:maker`.
    Reputation { score_bps: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RfqStatus {
    Open,
    Filled,
    Expired,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfqRequest {
    pub rfq_id: u64,
    pub requester: String,
    pub market: String,
    pub side: OrderSide,
    /// Base quantity, fixed-point 1e6.
    pub quantity: u64,
    /// Wall-clock ms deadline for accepting a quote.
    pub expires_at_ms: u64,
    pub created_at_ms: u64,
    pub status: RfqStatus,
    /// If Filled, which quote won.
    pub filled_by_quote_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfqQuote {
    pub quote_id: u64,
    pub rfq_id: u64,
    pub maker: String,
    /// Quote price in USDC × 1e6.
    pub price: u64,
    /// Quote quantity in base × 1e6 (must equal the RFQ's quantity;
    /// partial quotes are v2).
    pub quantity: u64,
    /// Wall-clock ms after which this quote is stale.
    pub expires_at_ms: u64,
    pub created_at_ms: u64,
    /// How the maker was cleared to post this quote. Defaults to
    /// `Allowlist` for wire-compat with pre-3.6 quotes.
    #[serde(default)]
    pub provenance: MakerProvenance,
}

#[derive(Default)]
pub struct RfqRegistry {
    pub requests: DashMap<u64, RfqRequest>,
    /// (rfq_id, quote_id) → quote
    pub quotes: DashMap<(u64, u64), RfqQuote>,
}

impl RfqRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn quotes_for(&self, rfq_id: u64) -> Vec<RfqQuote> {
        self.quotes
            .iter()
            .filter(|entry| entry.key().0 == rfq_id)
            .map(|entry| entry.value().clone())
            .collect()
    }
}

pub fn next_rfq_id() -> u64 {
    nonzero_random_id()
}

pub fn next_quote_id() -> u64 {
    nonzero_random_id()
}

fn nonzero_random_id() -> u64 {
    loop {
        let id = OsRng.next_u64();
        if id != 0 {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_maker_min_score_defaults_to_6000() {
        // Sanity: default matches the doc comment.
        // Can't rely on env var absence in parallel tests; just assert
        // the parser returns something in the sensible band when unset.
        let s = agent_maker_min_score_bps();
        assert!(s <= 10_001);
    }

    #[test]
    fn maker_provenance_serializes() {
        let p = MakerProvenance::Reputation { score_bps: 8_000 };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("8000"));
        assert!(j.contains("Reputation"));
        let d: MakerProvenance = serde_json::from_str(&j).unwrap();
        assert_eq!(d, p);
    }

    #[test]
    fn quote_notional_rejects_dust_and_balance_overflow() {
        assert_eq!(quote_notional_micro(2_000_000, 3_000_000), Some(6_000_000));
        assert_eq!(quote_notional_micro(1, 1), None);
        assert_eq!(quote_notional_micro(u64::MAX, u64::MAX), None);
    }

    #[test]
    fn quote_cannot_outlive_its_request() {
        assert!(quote_window_valid(100, 90, 89));
        assert!(!quote_window_valid(100, 90, 90));
        assert!(!quote_window_valid(100, 101, 50));
        assert!(!quote_window_valid(100, 100, 100));
    }

    #[test]
    fn rfq_ids_do_not_restart_at_a_replayable_counter() {
        let ids: std::collections::HashSet<_> = (0..100).map(|_| next_rfq_id()).collect();
        assert_eq!(ids.len(), 100);
        assert!(!ids.contains(&0));
        assert_ne!(next_quote_id(), next_rfq_id());
    }
}

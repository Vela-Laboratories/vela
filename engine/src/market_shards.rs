use crate::{
    BatchMetrics, BatchedRequest, EngineMap, FinancialCommitter, FinancialEffectSet,
    MatchingEngine, UserState,
};
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio::time::Duration;
use types::{
    AssetId, Balance, BatchResult, MarketId, OrderId, Request, Response, Timestamp, UserId,
    UserMetadata,
};

pub struct MarketShard {
    pub engine: MatchingEngine,
}

pub struct MarketShards {
    pub shards: HashMap<MarketId, Arc<TokioMutex<MarketShard>>>,
    pub user_state: Arc<RwLock<UserState>>,
}

impl MarketShards {
    pub fn new(user_state: Arc<RwLock<UserState>>) -> Self {
        MarketShards {
            shards: HashMap::new(),
            user_state,
        }
    }

    pub fn add_shard(&mut self, market_id: MarketId, engine: MatchingEngine) {
        self.shards
            .insert(market_id, Arc::new(TokioMutex::new(MarketShard { engine })));
    }

    pub async fn dispatch_sharded_batch(
        shards: Arc<MarketShards>,
        pending: Vec<BatchedRequest>,
        window_open: Instant,
        metrics: Arc<BatchMetrics>,
    ) -> BatchResult {
        Self::dispatch_sharded_batch_with_committer(shards, pending, window_open, metrics, None)
            .await
    }

    pub async fn dispatch_sharded_batch_with_committer(
        shards: Arc<MarketShards>,
        pending: Vec<BatchedRequest>,
        window_open: Instant,
        metrics: Arc<BatchMetrics>,
        committer: Option<Arc<dyn FinancialCommitter>>,
    ) -> BatchResult {
        let Some(committer) = committer else {
            return Self::dispatch_sharded_batch_in_place(shards, pending, window_open, metrics)
                .await;
        };

        // Hold the committed state locks for the complete proposal lifecycle.
        // Readers cannot observe speculative order-book or balance changes, and
        // other writers cannot be lost when the proposal is published.
        let mut committed_user_state = shards.user_state.write().await;
        let mut shard_ids: Vec<MarketId> = shards.shards.keys().cloned().collect();
        shard_ids.sort_by(|left, right| left.0.cmp(&right.0));
        let mut committed_shards = Vec::with_capacity(shard_ids.len());
        for market_id in &shard_ids {
            let shard = shards
                .shards
                .get(market_id)
                .expect("market id came from shard map")
                .lock()
                .await;
            committed_shards.push((market_id.clone(), shard));
        }

        let proposed_user_state = Arc::new(RwLock::new(committed_user_state.clone()));
        let mut proposed_shards = MarketShards::new(Arc::clone(&proposed_user_state));
        for (market_id, shard) in &committed_shards {
            proposed_shards.add_shard(market_id.clone(), shard.engine.clone());
        }
        let proposed_shards = Arc::new(proposed_shards);

        // The in-place executor writes only to the private proposal and sends
        // responses to proxy channels. Original callers hear nothing until the
        // ledger commit and state publication both succeed.
        let mut deliveries = Vec::with_capacity(pending.len());
        let mut proxy_pending = Vec::with_capacity(pending.len());
        for item in pending {
            let (proxy_responder, proxy_receiver) = tokio::sync::oneshot::channel();
            deliveries.push((item.responder, proxy_receiver));
            proxy_pending.push(BatchedRequest {
                request: item.request,
                ts: item.ts,
                responder: proxy_responder,
                decryption_proof: item.decryption_proof,
            });
        }

        let mut batch_result = Self::dispatch_sharded_batch_in_place(
            Arc::clone(&proposed_shards),
            proxy_pending,
            window_open,
            Arc::clone(&metrics),
        )
        .await;

        let mut proposed_responses = Vec::with_capacity(deliveries.len());
        for (_, receiver) in &mut deliveries {
            proposed_responses.push(receiver.await.unwrap_or_else(|_| {
                vec![Response::Error(types::ErrorResponse {
                    code: types::ErrorCode::InternalError,
                    message: "financial proposal failed".to_owned(),
                })]
            }));
        }

        let proposed_user_guard = proposed_user_state.read().await;
        let commit_result = match FinancialEffectSet::between(
            &committed_user_state,
            &proposed_user_guard,
            &batch_result.responses,
            batch_result.batch_size,
        ) {
            Ok(Some(effects)) => committer.commit(&effects).await,
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };

        if let Err(error) = commit_result {
            metrics.record_financial_commit_failure();
            tracing::error!(error = %error, "financial proposal rejected; committed state unchanged");
            proposed_responses = (0..deliveries.len())
                .map(|_| {
                    vec![Response::Error(types::ErrorResponse {
                        code: types::ErrorCode::InternalError,
                        message: "financial commit failed".to_owned(),
                    })]
                })
                .collect();
            batch_result.responses = proposed_responses.iter().flatten().cloned().collect();
        } else {
            *committed_user_state = proposed_user_guard.clone();
            for (market_id, committed_shard) in &mut committed_shards {
                let proposed_shard = proposed_shards
                    .shards
                    .get(market_id)
                    .expect("proposal contains every committed shard")
                    .lock()
                    .await;
                committed_shard.engine = proposed_shard.engine.clone();
            }
        }
        drop(proposed_user_guard);

        for ((responder, _), responses) in deliveries.into_iter().zip(proposed_responses) {
            let _ = responder.send(responses);
        }

        batch_result
    }

    /// Main sharded dispatch: implements 3-phase protocol.
    ///
    /// Phase 1 has two sub-passes:
    ///   1a. Apply deposits and withdrawals directly to UserState.
    ///   1b. Take snapshot (balances/metadata with fresh nonces + deposited funds).
    ///   1c. Validate PostOrders against the snapshot; check in-batch reservations.
    ///       Allocate order_ids. Do NOT modify nonces or balances in UserState.
    ///   1d. Validate cancels (look up routing).
    ///
    /// Phase 2 (concurrent): shard engines process orders using the snapshot as
    /// their base. They accept nonces and lock balances themselves.
    ///
    /// Phase 3: apply shard balance/metadata/fee deltas back to UserState.
    async fn dispatch_sharded_batch_in_place(
        shards: Arc<MarketShards>,
        pending: Vec<BatchedRequest>,
        window_open: Instant,
        _metrics: Arc<BatchMetrics>,
    ) -> BatchResult {
        let batch_size = pending.len();

        type PendingItem = (
            Request,
            Timestamp,
            tokio::sync::oneshot::Sender<Vec<Response>>,
            Option<types::DecryptionProof>,
        );
        let pending_items: Vec<PendingItem> = pending
            .into_iter()
            .map(|b| (b.request, b.ts, b.responder, b.decryption_proof))
            .collect();

        // ── Phase 1 ──────────────────────────────────────────────────────────────
        let mut per_market: HashMap<MarketId, Vec<(OrderId, Request, Timestamp, usize)>> =
            HashMap::new();
        let mut phase1_responses: Vec<(usize, Vec<Response>)> = Vec::new();

        let bal_arc: Arc<EngineMap<(UserId, AssetId), Balance>>;
        let meta_arc: Arc<EngineMap<UserId, UserMetadata>>;
        let snap_fees: EngineMap<String, u64>;

        {
            let mut us = shards.user_state.write().await;

            // Sub-pass 1a: handle deposits and withdrawals (modifies UserState).
            for (idx, (request, ts, _responder, _proof)) in pending_items.iter().enumerate() {
                match request {
                    Request::Deposit(req) => {
                        us.phase1_deposit(req);
                        let bal = us.get_balance(&req.user, &req.asset);
                        phase1_responses.push((
                            idx,
                            vec![Response::BalanceUpdated(types::BalanceUpdatedResponse {
                                user: req.user.clone(),
                                asset: req.asset,
                                available: bal.available,
                                locked: bal.locked,
                            })],
                        ));
                    }
                    Request::Withdrawal(req) => match us.phase1_withdraw(req, *ts) {
                        Ok(()) => {
                            let bal = us.get_balance(&req.user, &req.asset);
                            phase1_responses.push((
                                idx,
                                vec![Response::BalanceUpdated(types::BalanceUpdatedResponse {
                                    user: req.user.clone(),
                                    asset: req.asset,
                                    available: bal.available,
                                    locked: bal.locked,
                                })],
                            ));
                        }
                        Err(e) => {
                            phase1_responses.push((idx, vec![Response::Error(e.into())]));
                        }
                    },
                    _ => {}
                }
            }

            // Sub-pass 1b: snapshot AFTER deposits/withdrawals.
            // Nonces in metadata are still un-accepted — Phase 2 shard engines
            // will accept them when they call try_post_order.
            bal_arc = Arc::new(us.balances.clone());
            meta_arc = Arc::new(us.metadata.clone());
            snap_fees = us.fee_balances.clone();

            // Sub-pass 1c/1d: validate PostOrders and Cancels against snapshot.
            // Track in-batch reservations to prevent cross-market double-spend.
            let mut reserved: EngineMap<(UserId, AssetId), u64> = EngineMap::default();

            for (idx, (request, ts, _responder, _proof)) in pending_items.iter().enumerate() {
                match request {
                    Request::PostOrder(req) => {
                        match us.phase1_check_balance(req, *ts, &reserved, &bal_arc, &meta_arc) {
                            Ok((order_id, spend_asset, order_notional)) => {
                                *reserved.entry((req.user.clone(), spend_asset)).or_insert(0) +=
                                    order_notional;
                                per_market.entry(req.market.clone()).or_default().push((
                                    order_id,
                                    request.clone(),
                                    *ts,
                                    idx,
                                ));
                            }
                            Err(e) => {
                                phase1_responses.push((idx, vec![Response::Error(e.into())]));
                            }
                        }
                    }
                    Request::CancelOrder(req) => match us.phase1_cancel_validate(req) {
                        Ok((order_id, market_id)) => {
                            per_market.entry(market_id).or_default().push((
                                order_id,
                                request.clone(),
                                *ts,
                                idx,
                            ));
                        }
                        Err(e) => {
                            phase1_responses.push((idx, vec![Response::Error(e.into())]));
                        }
                    },
                    _ => {} // Deposits/withdrawals handled above
                }
            }
        } // Release UserState write lock

        // ── Phase 2: per-market shard locks, CONCURRENT ──────────────────────────

        struct ShardResult {
            #[allow(dead_code)]
            market_id: MarketId,
            responses_by_idx: Vec<(usize, Vec<Response>)>,
            final_balances: EngineMap<(UserId, AssetId), Balance>,
            final_metadata: EngineMap<UserId, UserMetadata>,
            final_fees: EngineMap<String, u64>,
        }

        let shard_futures: Vec<_> = per_market
            .into_iter()
            .map(|(market_id, orders)| {
                let shard_arc = shards.shards.get(&market_id).cloned();
                let bal_arc = Arc::clone(&bal_arc);
                let meta_arc = Arc::clone(&meta_arc);
                let market_id_clone = market_id.clone();

                async move {
                    let shard_arc = match shard_arc {
                        Some(s) => s,
                        None => {
                            let responses: Vec<(usize, Vec<Response>)> = orders
                                .into_iter()
                                .map(|(_, req, _, idx)| {
                                    let market_str = match &req {
                                        Request::PostOrder(r) => r.market.0.clone(),
                                        _ => market_id_clone.0.clone(),
                                    };
                                    (
                                        idx,
                                        vec![Response::Error(types::ErrorResponse {
                                            code: types::ErrorCode::MarketNotFound,
                                            message: format!("market {} not found", market_str),
                                        })],
                                    )
                                })
                                .collect();
                            return ShardResult {
                                market_id: market_id_clone,
                                responses_by_idx: responses,
                                final_balances: EngineMap::default(),
                                final_metadata: EngineMap::default(),
                                final_fees: EngineMap::default(),
                            };
                        }
                    };

                    let mut shard = shard_arc.lock().await;

                    // Inject snapshot so the engine uses UserState balances/metadata
                    // as its base for DeltaBuffer lookups.
                    shard
                        .engine
                        .set_snapshot(Some(Arc::clone(&bal_arc)), Some(Arc::clone(&meta_arc)));

                    let mut responses_by_idx: Vec<(usize, Vec<Response>)> = Vec::new();

                    for (pre_id, request, ts, idx) in orders {
                        if let Request::PostOrder(_) = &request {
                            shard.engine.set_next_order_id(pre_id);
                        }
                        let resps = shard.engine.process(request, ts);
                        if shard.engine.next_order_id() <= pre_id {
                            shard.engine.set_next_order_id(pre_id + 1);
                        }
                        responses_by_idx.push((idx, resps));
                    }

                    shard.engine.set_snapshot(None, None);

                    let final_balances = shard.engine.balances.clone();
                    let final_metadata = shard.engine.metadata.clone();
                    let final_fees = shard.engine.fee_balances.clone();

                    ShardResult {
                        market_id: market_id_clone,
                        responses_by_idx,
                        final_balances,
                        final_metadata,
                        final_fees,
                    }
                }
            })
            .collect();

        let shard_results: Vec<ShardResult> = join_all(shard_futures).await;

        // ── Phase 3: UserState write lock — apply deltas ─────────────────────────
        let mut phase2_responses: Vec<(usize, Vec<Response>)> = Vec::new();

        {
            let mut us = shards.user_state.write().await;

            for result in shard_results {
                us.apply_balance_delta(&result.final_balances, &bal_arc);
                us.apply_metadata_delta(&result.final_metadata, &meta_arc);
                us.apply_fee_delta(&result.final_fees, &snap_fees);

                for (idx, resps) in result.responses_by_idx {
                    for resp in &resps {
                        match resp {
                            Response::OrderCanceled(c) => {
                                if let Some((Request::CancelOrder(req), _, _, _)) =
                                    pending_items.get(idx)
                                {
                                    us.remove_order_routing(
                                        c.order_id,
                                        &req.user,
                                        c.client_order_id.as_deref(),
                                    );
                                }
                            }
                            Response::OrderPosted(p) => {
                                if matches!(
                                    p.status,
                                    types::OrderStatus::Filled | types::OrderStatus::Canceled
                                ) {
                                    if let Some((Request::PostOrder(req), _, _, _)) =
                                        pending_items.get(idx)
                                    {
                                        us.remove_order_routing(
                                            p.order_id,
                                            &req.user,
                                            p.client_order_id.as_deref(),
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    phase2_responses.push((idx, resps));
                }
            }
        } // Release UserState write lock

        // ── Distribute responses ──────────────────────────────────────────────────
        let dispatch_latency_ns = window_open.elapsed().as_nanos() as u64;

        let mut response_map: HashMap<usize, Vec<Response>> = HashMap::new();
        for (idx, resps) in phase1_responses {
            response_map.insert(idx, resps);
        }
        for (idx, resps) in phase2_responses {
            response_map.insert(idx, resps);
        }

        let mut flat_responses: Vec<Response> = Vec::new();
        let mut decryption_proofs = Vec::new();

        for (idx, (_request, _ts, responder, proof)) in pending_items.into_iter().enumerate() {
            let resps = response_map.remove(&idx).unwrap_or_default();
            flat_responses.extend_from_slice(&resps);
            if let Some(p) = proof {
                decryption_proofs.push(p);
            }
            let _ = responder.send(resps);
        }

        BatchResult {
            responses: flat_responses,
            batch_size,
            dispatch_latency_ns,
            decryption_proofs,
        }
    }

    /// Infinite run loop (mirrors BatchDispatcher::run but uses sharded dispatch).
    pub async fn run(
        shards: Arc<Self>,
        mut rx: tokio::sync::mpsc::Receiver<BatchedRequest>,
        window_us: u64,
        max_batch_size: usize,
        metrics: Arc<BatchMetrics>,
        result_tx: Option<tokio::sync::mpsc::Sender<BatchResult>>,
        financial_committer: Option<Arc<dyn FinancialCommitter>>,
    ) {
        loop {
            let first = match rx.recv().await {
                Some(item) => item,
                None => break,
            };

            let window_open = Instant::now();
            let deadline = tokio::time::Instant::now() + Duration::from_micros(window_us);

            let mut pending = Vec::with_capacity(max_batch_size);
            pending.push(first);

            while pending.len() < max_batch_size {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(item)) => pending.push(item),
                    _ => break,
                }
            }

            let batch_result = Self::dispatch_sharded_batch_with_committer(
                Arc::clone(&shards),
                pending,
                window_open,
                Arc::clone(&metrics),
                financial_committer.clone(),
            )
            .await;

            metrics.record_batch(batch_result.batch_size, batch_result.dispatch_latency_ns);

            if let Some(ref tx) = result_tx {
                let _ = tx.try_send(batch_result);
            }
        }
    }
}

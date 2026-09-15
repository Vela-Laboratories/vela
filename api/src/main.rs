use api::types::Incident;
use api::wal::{Wal, WalEngineStart, WalEngineStop};
use engine::MatchingEngine;
use std::sync::Arc;
use types::{
    AssetId, DepositRequest, FeeConfig, Market, MarketId, OrderSide, OrderType, PostOrderRequest,
    Request, RuntimeMode, UserId,
};

// mimalloc allocator: measurably faster than the platform default on the
// matching engine's HashMap-heavy allocation pattern. Global; no runtime
// configuration required.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn seed_markets(engine: &mut MatchingEngine) {
    for ticker in &[
        "ETH", "BTC", "SOL", "ARB", "OP", "AVAX", "MATIC", "LINK", "UNI", "AAVE", "DOGE", "PEPE",
        "WIF", "JUP", "PENDLE", "EIGEN",
    ] {
        engine.add_market(Market {
            id: MarketId(format!("{ticker}-USDC")),
            base: AssetId::from_str(ticker),
            quote: AssetId::from_str("USDC"),
            max_orders: 10_000,
            min_order_size: 1,
            price_tick: 1,
            quantity_tick: 1,
            maker_fee_bps: -1,
            taker_fee_bps: 5,
        });
    }
}

fn initialize_fresh_engine(engine: &mut MatchingEngine, mode: RuntimeMode) {
    if mode.is_production() {
        tracing::warn!("production started with empty engine state; no demo markets were seeded");
        return;
    }

    seed_markets(engine);
    if mode.allows_demo_state() {
        seed_order_books(engine);
        tracing::info!("fresh testnet demo state seeded");
    } else {
        tracing::info!("fresh development market catalog initialized without synthetic balances");
    }
}

fn seed_order_books(engine: &mut MatchingEngine) {
    let seed_user = UserId([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    let make_hash = |a: u8, b: u8| -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = 2;
        h[30] = a;
        h[31] = b;
        h
    };

    engine.process(
        Request::Deposit(DepositRequest {
            user: seed_user.clone(),
            asset: AssetId::from_str("USDC"),
            amount: 10_000_000_000_000_000,
            l1_tx_hash: make_hash(0, 0),
            source_chain: Default::default(),
            bridge_id: None,
        }),
        0,
    );

    for (i, asset) in [
        "BTC", "ETH", "SOL", "AVAX", "MATIC", "LINK", "UNI", "ARB", "OP", "AAVE", "DOGE", "PEPE",
        "WIF", "JUP", "PENDLE", "EIGEN",
    ]
    .iter()
    .enumerate()
    {
        engine.process(
            Request::Deposit(DepositRequest {
                user: seed_user.clone(),
                asset: AssetId::from_str(asset),
                amount: 100_000_000_000_000,
                l1_tx_hash: make_hash(1, i as u8),
                source_chain: Default::default(),
                bridge_id: None,
            }),
            0,
        );
    }

    let markets: &[(&str, u64, u64)] = &[
        ("BTC-USDC", 94_000 * 100_000_000, 100_000),
        ("ETH-USDC", 3_200 * 100_000_000, 500_000),
        ("SOL-USDC", 145 * 100_000_000, 10_000_000),
        ("AVAX-USDC", 35 * 100_000_000, 5_000_000),
        ("MATIC-USDC", 850_000, 1_000_000_000),
        ("LINK-USDC", 14 * 100_000_000, 50_000_000),
        ("UNI-USDC", 9 * 100_000_000, 50_000_000),
        ("ARB-USDC", 1_100_000, 500_000_000),
        ("OP-USDC", 2_400_000, 200_000_000),
        ("AAVE-USDC", 280 * 100_000_000, 1_000_000),
        ("DOGE-USDC", 180_000, 10_000_000_000),
        ("PEPE-USDC", 8, 50_000_000_000_000),
        ("WIF-USDC", 1_200_000, 500_000_000),
        ("JUP-USDC", 500_000, 1_000_000_000),
        ("PENDLE-USDC", 3_500_000, 100_000_000),
        ("EIGEN-USDC", 2_000_000, 200_000_000),
    ];

    let mut nonce: u64 = 1000;

    for &(market_name, mid, base_size) in markets {
        let half_spread = mid * 5 / 10_000;
        let best_bid = mid - half_spread;
        let best_ask = mid + half_spread;

        let mut bid_price = best_bid;
        for i in 0u64..20 {
            let quantity = ((i % 5) + 3) * base_size;
            engine.process(
                Request::PostOrder(PostOrderRequest {
                    user: seed_user.clone(),
                    market: MarketId(market_name.to_string()),
                    side: OrderSide::Bid,
                    order_type: OrderType::GoodTillCanceled,
                    price: bid_price,
                    quantity,
                    nonce,
                    client_order_id: None,
                    signature: vec![],
                    stp: Default::default(),
                    min_quantity: None,
                    display_quantity: None,
                }),
                0,
            );
            nonce += 1;
            bid_price = bid_price * 9_995 / 10_000;
        }

        let mut ask_price = best_ask;
        for i in 0u64..20 {
            let quantity = ((i % 5) + 3) * base_size;
            engine.process(
                Request::PostOrder(PostOrderRequest {
                    user: seed_user.clone(),
                    market: MarketId(market_name.to_string()),
                    side: OrderSide::Ask,
                    order_type: OrderType::GoodTillCanceled,
                    price: ask_price,
                    quantity,
                    nonce,
                    client_order_id: None,
                    signature: vec![],
                    stp: Default::default(),
                    min_quantity: None,
                    display_quantity: None,
                }),
                0,
            );
            nonce += 1;
            ask_price = ask_price * 10_005 / 10_000;
        }
    }
}

fn init_observability(mode: RuntimeMode) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Split init so we don't panic if a global subscriber was already
    // installed (e.g. by a test harness inside the same process).
    let init_result = if mode.is_production() {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json().with_target(true))
            .try_init()
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .try_init()
    };

    if init_result.is_ok() {
        // Install a panic hook that emits a structured error before the
        // default handler runs. Only wire it once — matches the
        // subscriber install above.
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info
                .payload()
                .downcast_ref::<&'static str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            tracing::error!(target: "panic", location = %location, payload = %payload, "process panic");
            default_hook(info);
        }));
    }
}

#[tokio::main]
async fn main() {
    let runtime = api::runtime::RuntimeSettings::from_env()
        .unwrap_or_else(|error| panic!("invalid Vela runtime configuration: {error}"));
    init_observability(runtime.mode);
    runtime.log_nonsecret_report();
    runtime.validate().unwrap_or_else(|error| panic!("{error}"));

    let ledger = match std::env::var("DATABASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(database_url) => {
            let ledger = ledger::Ledger::connect_with_options(
                &database_url,
                runtime.database_max_connections,
                std::time::Duration::from_millis(runtime.database_acquire_timeout_ms),
                std::time::Duration::from_millis(runtime.database_statement_timeout_ms),
                runtime.database_serializable_retries,
            )
            .await
            .unwrap_or_else(|_| panic!("ledger database connection failed"));
            ledger
                .ensure_schema_current()
                .await
                .unwrap_or_else(|error| panic!("ledger database is not ready: {error}"));
            tracing::info!(
                schema_version = ledger::Ledger::EXPECTED_SCHEMA_VERSION,
                "ledger database connected"
            );
            Some(Arc::new(ledger))
        }
        None => {
            tracing::warn!("ledger database not configured; financial ledger unavailable");
            None
        }
    };

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3001);

    let wal_dir = std::path::PathBuf::from(
        std::env::var("SNAPSHOT_DIR").unwrap_or_else(|_| "/data".to_string()),
    );

    let recovery = Wal::inspect(&wal_dir)
        .unwrap_or_else(|error| panic!("WAL recovery validation failed: {error}"));
    let wal = Arc::new(
        Wal::new(&wal_dir).unwrap_or_else(|error| panic!("WAL initialization failed: {error}")),
    );

    let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);

    let mut loaded_incidents: Vec<Incident> = Vec::new();
    let mut loaded_decisions: Vec<api::types::Decision> = Vec::new();
    let mut loaded_mms: Vec<api::types::RegisteredMM> = Vec::new();
    let mut loaded_proofs: std::collections::HashMap<u64, zkvm::BatchProof> =
        std::collections::HashMap::new();
    let mut loaded_attestations: std::collections::HashMap<u64, tee::AttestationRecord> =
        std::collections::HashMap::new();
    let mut need_restart_incident = false;
    let snapshot_wal_sequence;

    match api::snapshot::load_recovery_snapshot_default(&recovery).await {
        Ok(Some(snapshot)) => {
            if !snapshot.clean_shutdown || !recovery.clean_shutdown {
                need_restart_incident = true;
            }
            snapshot_wal_sequence = snapshot.wal_sequence;
            if snapshot_wal_sequence > recovery.last_sequence {
                panic!("snapshot references WAL sequence newer than durable WAL evidence");
            }
            loaded_incidents = snapshot.incidents.clone();
            loaded_decisions = snapshot.decisions.clone();
            loaded_mms = snapshot.registered_mms.clone();
            loaded_proofs = api::snapshot::extract_proofs_from_snapshot(&snapshot);
            loaded_attestations = api::snapshot::extract_attestations_from_snapshot(&snapshot);
            api::snapshot::restore_engine_from_snapshot(&mut engine, snapshot)
                .unwrap_or_else(|error| panic!("snapshot restore failed: {error}"));
            tracing::info!("engine restored from snapshot");
        }
        Ok(None) => {
            snapshot_wal_sequence = 0;
            initialize_fresh_engine(&mut engine, runtime.mode);
        }
        Err(e) => {
            panic!("snapshot recovery validation failed: {e}");
        }
    }

    let (recovery_reason, recovered_fills, recovered_orders) =
        if !recovery.clean_shutdown && recovery.has_entries {
            let (fills, orders) =
                api::snapshot::replay_wal_entries(&mut engine, &wal_dir, snapshot_wal_sequence)
                    .unwrap_or_else(|error| panic!("WAL replay failed: {error}"));
            tracing::info!(
                "WAL replay complete: replayed entries, recovered {} orders {} fills",
                orders.len(),
                fills.len()
            );
            ("crash_recovery", fills, orders)
        } else {
            ("clean", vec![], std::collections::HashMap::new())
        };

    runtime
        .validate_engine_state(&engine)
        .unwrap_or_else(|error| panic!("{error}"));

    let previous_sequence = wal.current_sequence();

    let state =
        api::AppState::new_with_runtime_and_ledger(engine, Arc::clone(&wal), runtime.mode, ledger)
            .await
            .unwrap_or_else(|error| {
                if error.starts_with("engine recovery replication") {
                    panic!("engine recovery replication initialization failed")
                }
                if error.starts_with("engine leadership") || error.starts_with("another engine") {
                    panic!("authoritative engine leadership initialization failed")
                }
                panic!("authoritative ledger replay failed")
            });

    {
        let replayed_engine = state.engine.lock().await;
        runtime
            .validate_engine_state(&replayed_engine)
            .unwrap_or_else(|error| panic!("replayed engine state is unsafe: {error}"));
    }

    {
        *state.incidents.lock().await = loaded_incidents;
        *state.decisions.lock().await = loaded_decisions;
        *state.registered_mms.lock().await = loaded_mms;
        *state.proofs.lock().await = loaded_proofs;
        *state.attestations.lock().await = loaded_attestations;
    }

    if !recovered_fills.is_empty() || !recovered_orders.is_empty() {
        *state.fills.lock().await = recovered_fills;
        *state.stored_orders.lock().await = recovered_orders;
    }

    if need_restart_incident {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut incidents = state.incidents.lock().await;
        let next_id = incidents.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        incidents.push(Incident {
            id: next_id,
            incident_type: "RESTART".to_string(),
            started_at: now_ms.saturating_sub(1000),
            resolved_at: Some(now_ms),
            description: "Engine restarted. State restored from snapshot.".to_string(),
            impact: "Brief interruption. All state preserved.".to_string(),
        });
    }

    wal.append(
        api::wal::ENGINE_START,
        &WalEngineStart {
            version: "0.2.0".to_string(),
            reason: recovery_reason.to_string(),
            previous_sequence,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("WAL ENGINE_START write failed: {error}"));

    // A configured replica is a production durability requirement, so prove
    // one complete publication before any listener can accept traffic.
    if state.recovery_replica.is_some() {
        if api::snapshot::save_snapshot(Arc::clone(&state), false)
            .await
            .is_err()
        {
            if let Some(leadership) = &state.engine_leadership {
                if let Err(release_error) = leadership.release().await {
                    tracing::error!(error = %release_error, generation = leadership.token().generation, "failed to release engine leadership after initial recovery replication failure");
                }
            }
            panic!("initial engine recovery replication failed");
        }
        state.last_snapshot_ts.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    if let Some((anchors, count)) = api::anchor::load_anchors().await {
        let last = anchors.last().map(|a| (a.tx_hash.clone(), a.timestamp));
        *state.anchors.lock().await = anchors;
        state
            .anchor_count
            .store(count, std::sync::atomic::Ordering::Relaxed);
        if let Some((tx, ts)) = last {
            *state.last_anchor_tx.lock().await = Some(tx);
            state
                .last_anchor_time
                .store(ts, std::sync::atomic::Ordering::Relaxed);
        }
    }

    let alchemy_url = std::env::var("ALCHEMY_API_URL")
        .unwrap_or_else(|_| "https://eth-sepolia.g.alchemy.com/v2/demo".to_string());
    let operator_key = std::env::var("OPERATOR_PRIVATE_KEY").unwrap_or_default();
    let anchor_contract_address = runtime
        .settlement_address
        .clone()
        .or_else(|| runtime.legacy_contract_address.clone())
        .unwrap_or_else(|| "0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686".to_string());

    if !operator_key.is_empty() {
        let anchor_state = Arc::clone(&state);
        let anchor_chain_id = runtime.chain_id;
        tokio::spawn(async move {
            api::anchor::anchor_task(
                anchor_state,
                alchemy_url,
                operator_key,
                anchor_chain_id,
                anchor_contract_address,
            )
            .await;
        });
    } else {
        tracing::warn!("OPERATOR_PRIVATE_KEY not set — anchor task disabled");
    }

    if runtime.internal_mm_enabled() {
        let mm_order_tx = state.order_tx.clone();
        tokio::spawn(async move {
            api::mm::run_mm_bot(mm_order_tx).await;
        });
    } else {
        tracing::info!("internal market-making bot disabled for this runtime mode");
    }

    let snapshot_state = Arc::clone(&state);
    let snapshot_task = tokio::spawn(async move {
        api::snapshot::run_snapshot_task(snapshot_state).await;
    });

    let pyth_enabled = std::env::var("VELA_PYTH_ENABLED")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(true);
    if pyth_enabled {
        let pyth_state = Arc::clone(&state);
        tokio::spawn(async move {
            api::pyth::pyth_feed_task(pyth_state).await;
        });
    } else {
        tracing::info!("VELA_PYTH_ENABLED=false — Pyth feed task disabled");
    }

    let shutdown_state = Arc::clone(&state);
    let router = api::build_router(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("listening on {addr}");

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        result = axum::serve(listener, router) => {
            if let Err(e) = result {
                eprintln!("Server error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("SIGINT received, shutting down");
        }
        _ = terminate => {
            tracing::info!("SIGTERM received, shutting down");
        }
    }

    // Stop the periodic publisher before creating the one final recovery
    // boundary. Otherwise it could interleave an unclean snapshot between the
    // final checkpoint and ENGINE_STOP.
    snapshot_task.abort();
    let _ = snapshot_task.await;

    // Only label the terminal WAL record clean when the final snapshot and
    // its checkpoint were both published successfully.
    let shutdown_health = Arc::clone(&shutdown_state.storage_health);
    let snapshot_succeeded =
        match api::snapshot::save_snapshot(Arc::clone(&shutdown_state), true).await {
            Ok(()) => true,
            Err(error) => {
                tracing::error!("Final snapshot save failed: {error}");
                false
            }
        };
    let stop_reason = api::durability::shutdown_stop_reason(snapshot_succeeded, &shutdown_health);

    let stop_written = wal
        .append(
            api::wal::ENGINE_STOP,
            &WalEngineStop {
                reason: stop_reason.to_string(),
                final_sequence: wal.current_sequence(),
            },
        )
        .await;
    let stop_succeeded = match stop_written {
        Err(error) => {
            tracing::error!("WAL ENGINE_STOP write failed: {error}");
            false
        }
        Ok(_) => match wal.flush().await {
            Ok(()) => true,
            Err(error) => {
                tracing::error!("WAL ENGINE_STOP flush failed: {error}");
                false
            }
        },
    };

    if snapshot_succeeded && stop_succeeded && shutdown_health.is_healthy() {
        if let Err(error) =
            api::snapshot::replicate_clean_shutdown(Arc::clone(&shutdown_state)).await
        {
            tracing::error!("final engine recovery replication failed: {error}");
        }
    }

    if let Some(leadership) = &shutdown_state.engine_leadership {
        if let Err(error) = leadership.release().await {
            tracing::error!(error = %error, generation = leadership.token().generation, "engine leadership release failed; lease will expire without being reassigned");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_fresh_start_has_no_seeded_state() {
        let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);
        initialize_fresh_engine(&mut engine, RuntimeMode::Production);

        assert!(engine.balances.is_empty());
        assert!(engine.order_books.is_empty());
        assert!(engine.markets.is_empty());
    }

    #[test]
    fn development_has_markets_but_no_synthetic_financial_state() {
        let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);
        initialize_fresh_engine(&mut engine, RuntimeMode::Development);

        assert!(!engine.markets.is_empty());
        assert!(engine.balances.is_empty());
        assert!(engine
            .order_books
            .values()
            .all(|order_book| order_book.total_orders == 0));
    }
}

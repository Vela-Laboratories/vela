use crate::types::{Decision, Incident, OrderFillRecord, RegisteredMM, StoredFill, StoredOrder};
use crate::wal;
use crate::wal::{
    Wal, WalBalanceChange, WalCheckpoint, WalDeposit, WalEngineStart, WalEngineStop,
    WalFillCreated, WalOrderCancel, WalOrderPost, WalOrderProcessed, WalWithdrawalRequest,
};
use anyhow::{bail, Context, Result};
use engine::MatchingEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tee::AttestationRecord;
use types::{
    AssetId, Balance, DepositRequest, Market, Order, Response, UserId, UserMetadata,
    WithdrawalRequest,
};
use zkvm::BatchProof;

const SNAPSHOT_INTERVAL_SECS: u64 = 60;
pub const CURRENT_SNAPSHOT_VERSION: u32 = 2;

fn snapshot_path() -> String {
    let dir = std::env::var("SNAPSHOT_DIR").unwrap_or_else(|_| "/data".to_string());
    format!("{dir}/engine_snapshot.json")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializedBalance {
    pub available: u64,
    pub locked: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub version: u32,
    pub timestamp: u64,
    pub balances: HashMap<String, HashMap<String, SerializedBalance>>,
    pub orders: HashMap<String, Vec<Order>>,
    pub markets: Vec<Market>,
    pub sequence: u64,
    #[serde(default)]
    pub wal_sequence: u64,
    pub metadata: HashMap<String, UserMetadata>,
    #[serde(default)]
    pub fee_balances: HashMap<String, u64>,
    #[serde(default)]
    pub clean_shutdown: bool,
    #[serde(default)]
    pub incidents: Vec<Incident>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub registered_mms: Vec<RegisteredMM>,
    #[serde(default)]
    pub proofs: HashMap<u64, BatchProof>,
    #[serde(default)]
    pub attestations: HashMap<u64, AttestationRecord>,
}

pub async fn save_snapshot(state: Arc<crate::AppState>, clean_shutdown: bool) -> Result<()> {
    let (timestamp, balances, orders, markets, sequence, metadata, fee_balances) = {
        let engine = state.engine.lock().await;

        let mut balances: HashMap<String, HashMap<String, SerializedBalance>> = HashMap::new();
        for ((user, asset), balance) in &engine.balances {
            balances.entry(user.to_hex()).or_default().insert(
                asset.as_str().to_string(),
                SerializedBalance {
                    available: balance.available,
                    locked: balance.locked,
                },
            );
        }

        let mut orders: HashMap<String, Vec<Order>> = HashMap::new();
        for (market_id, book) in &engine.order_books {
            orders.insert(market_id.0.clone(), book.all_orders());
        }

        let markets: Vec<Market> = engine.markets.values().cloned().collect();

        let mut metadata: HashMap<String, UserMetadata> = HashMap::new();
        for (user, meta) in &engine.metadata {
            metadata.insert(user.to_hex(), meta.clone());
        }

        let fee_balances: HashMap<String, u64> = engine
            .fee_balances
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let timestamp = engine.timestamp;
        let sequence = engine.next_order_id();

        (
            timestamp,
            balances,
            orders,
            markets,
            sequence,
            metadata,
            fee_balances,
        )
    };

    let n_orders: usize = orders.values().map(|v| v.len()).sum();
    let n_users = balances.len();
    let n_fills = state.fills.lock().await.len();

    let incidents = state.incidents.lock().await.clone();
    let decisions = state.decisions.lock().await.clone();
    let registered_mms = state.registered_mms.lock().await.clone();
    let proofs = state.proofs.lock().await.clone();
    let attestations = state.attestations.lock().await.clone();

    let snapshot = EngineSnapshot {
        version: CURRENT_SNAPSHOT_VERSION,
        timestamp,
        balances,
        orders,
        markets,
        sequence,
        wal_sequence: state.wal.current_sequence(),
        metadata,
        fee_balances,
        clean_shutdown,
        incidents,
        decisions,
        registered_mms,
        proofs,
        attestations,
    };

    let json = serde_json::to_vec(&snapshot)?;

    let path = snapshot_path();
    let dir = std::path::Path::new(&path)
        .parent()
        .unwrap_or(std::path::Path::new("/data"));
    tokio::fs::create_dir_all(dir).await?;

    let tmp_path = format!("{path}.tmp");
    tokio::fs::write(&tmp_path, &json).await?;
    tokio::fs::rename(&tmp_path, &path).await?;

    tracing::info!("Snapshot saved: {n_orders} orders, {n_users} users");

    let checkpoint = WalCheckpoint {
        snapshot_path: path,
        total_orders: n_orders as u64,
        total_fills: n_fills as u64,
        total_users: n_users as u64,
    };
    state
        .wal
        .append(wal::CHECKPOINT, &checkpoint)
        .await
        .context("append snapshot WAL checkpoint")?;
    state.wal.rotate().await.context("rotate WAL checkpoint")?;

    Ok(())
}

pub async fn load_snapshot() -> Result<Option<EngineSnapshot>> {
    let path = snapshot_path();
    load_snapshot_from(Path::new(&path)).await
}

pub async fn load_recovery_snapshot_default(
    recovery: &crate::wal::WalRecoveryState,
) -> Result<Option<EngineSnapshot>> {
    let path = snapshot_path();
    load_recovery_snapshot(Path::new(&path), recovery).await
}

pub async fn load_snapshot_from(path: &Path) -> Result<Option<EngineSnapshot>> {
    let snapshot = read_snapshot_from(path).await?;
    if let Some(snapshot) = &snapshot {
        validate_snapshot(snapshot)?;
    }
    Ok(snapshot)
}

async fn read_snapshot_from(path: &Path) -> Result<Option<EngineSnapshot>> {
    match tokio::fs::try_exists(path).await {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("inspect snapshot {}", path.display()));
        }
    }
    let data = tokio::fs::read(path)
        .await
        .with_context(|| format!("read snapshot {}", path.display()))?;
    let snapshot: EngineSnapshot = serde_json::from_slice(&data)
        .with_context(|| format!("parse snapshot {}", path.display()))?;
    Ok(Some(snapshot))
}

pub async fn load_recovery_snapshot(
    path: &Path,
    recovery: &crate::wal::WalRecoveryState,
) -> Result<Option<EngineSnapshot>> {
    let mut snapshot = read_snapshot_from(path).await?;
    if let Some(snapshot) = &mut snapshot {
        if snapshot.version == 1 {
            if !recovery.clean_shutdown {
                bail!(
                    "version-1 snapshot migration requires a clean terminal WAL boundary; operator recovery is required"
                );
            }
            snapshot.wal_sequence = recovery
                .checkpoint_sequence
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("version-1 snapshot has no WAL checkpoint"))?;
            snapshot.version = CURRENT_SNAPSHOT_VERSION;
        }
        validate_snapshot(snapshot)?;
    }
    match (&snapshot, recovery.has_prior_evidence) {
        (None, true) => bail!(
            "snapshot is missing while prior WAL recovery evidence exists; operator recovery is required"
        ),
        (Some(_), false) => bail!(
            "snapshot exists without corresponding WAL recovery evidence; operator recovery is required"
        ),
        (Some(snapshot), true) => {
            if !recovery.clean_shutdown || !snapshot.clean_shutdown {
                bail!(
                    "automatic recovery from an unclean snapshot/WAL set is disabled until snapshot publication is atomic; operator recovery is required"
                );
            }
            let expected_checkpoint = snapshot
                .wal_sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("snapshot WAL sequence is exhausted"))?;
            let expected_stop = expected_checkpoint
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("snapshot WAL sequence is exhausted"))?;
            if recovery.checkpoint_sequence != expected_checkpoint
                || recovery.last_sequence != expected_stop
            {
                bail!(
                    "clean snapshot/WAL boundary is inconsistent; operator recovery is required"
                );
            }
            if recovery.checkpoint_snapshot_path.as_deref().map(Path::new) != Some(path) {
                bail!(
                    "clean WAL checkpoint references a different snapshot; operator recovery is required"
                );
            }
        }
        _ => {}
    }
    Ok(snapshot)
}

pub async fn run_snapshot_task(state: Arc<crate::AppState>) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(SNAPSHOT_INTERVAL_SECS)).await;
        if let Err(e) = save_snapshot(Arc::clone(&state), false).await {
            tracing::error!("Snapshot error: {e}");
        } else {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            state
                .last_snapshot_ts
                .store(ts, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

pub fn restore_engine_from_snapshot(
    engine: &mut MatchingEngine,
    snapshot: EngineSnapshot,
) -> Result<()> {
    validate_snapshot(&snapshot)?;
    for market in snapshot.markets {
        engine.add_market(market);
    }

    for (user_hex, asset_balances) in snapshot.balances {
        let user = UserId::from_hex(&user_hex)?;
        for (asset_str, bal) in asset_balances {
            let asset = AssetId::from_str(&asset_str);
            engine.balances.insert(
                (user.clone(), asset),
                Balance {
                    user: user.clone(),
                    asset,
                    available: bal.available,
                    locked: bal.locked,
                },
            );
        }
    }

    for (user_hex, meta) in snapshot.metadata {
        let user = UserId::from_hex(&user_hex)?;
        engine.metadata.insert(user, meta);
    }

    for (market_id_str, orders) in snapshot.orders {
        let market_id = types::MarketId(market_id_str.clone());
        let book = engine.order_books.get_mut(&market_id).ok_or_else(|| {
            anyhow::anyhow!("snapshot orders reference unknown market {market_id_str}")
        })?;
        for order in orders {
            book.insert_resting(order)
                .with_context(|| format!("restore order into market {market_id_str}"))?;
        }
    }

    engine.set_next_order_id(snapshot.sequence);

    engine.fee_balances = snapshot.fee_balances.into_iter().collect();

    tracing::info!("Engine restored from snapshot: {}", snapshot.timestamp);

    Ok(())
}

fn validate_snapshot(snapshot: &EngineSnapshot) -> Result<()> {
    if snapshot.version != CURRENT_SNAPSHOT_VERSION {
        bail!(
            "unsupported snapshot version {}; expected {}",
            snapshot.version,
            CURRENT_SNAPSHOT_VERSION
        );
    }

    let mut market_ids = std::collections::HashSet::new();
    for market in &snapshot.markets {
        if !market_ids.insert(market.id.0.clone()) {
            bail!("duplicate snapshot market {}", market.id.0);
        }
    }

    let mut order_ids = std::collections::HashSet::new();
    for (market_id, orders) in &snapshot.orders {
        if !market_ids.contains(market_id) {
            bail!("snapshot orders reference unknown market {market_id}");
        }
        for order in orders {
            if order.market.0 != *market_id {
                bail!(
                    "snapshot order {} market does not match container {market_id}",
                    order.id
                );
            }
            if !order_ids.insert(order.id) {
                bail!("duplicate snapshot order {}", order.id);
            }
        }
    }
    if order_ids
        .iter()
        .max()
        .is_some_and(|maximum_order_id| snapshot.sequence <= *maximum_order_id)
    {
        bail!("snapshot next order sequence does not exceed every restored order ID");
    }
    Ok(())
}

pub fn extract_proofs_from_snapshot(snapshot: &EngineSnapshot) -> HashMap<u64, BatchProof> {
    snapshot.proofs.clone()
}

pub fn extract_attestations_from_snapshot(
    snapshot: &EngineSnapshot,
) -> HashMap<u64, AttestationRecord> {
    snapshot.attestations.clone()
}

pub fn replay_wal_entries(
    engine: &mut MatchingEngine,
    wal_dir: &Path,
    after_sequence: u64,
) -> Result<(Vec<StoredFill>, HashMap<u64, StoredOrder>)> {
    let mut recovered_fills: Vec<StoredFill> = Vec::new();
    let mut recovered_orders: HashMap<u64, StoredOrder> = HashMap::new();
    let mut fill_ids_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut replayed = 0usize;

    for entry in Wal::read_all_from(wal_dir, after_sequence)? {
        replayed += 1;
        match entry.entry_type {
            wal::ORDER_POST => {
                let post = entry.decode::<WalOrderPost>().with_context(|| {
                    format!("decode ORDER_POST at WAL sequence {}", entry.sequence)
                })?;
                let order_id = post.order_id;
                recovered_orders.entry(order_id).or_insert_with(|| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u64;
                    StoredOrder {
                        id: order_id,
                        market_id: post.market_id,
                        user: post.user,
                        side: post.side,
                        price: post.price,
                        quantity: post.quantity,
                        filled_quantity: 0,
                        status: "open".to_string(),
                        order_type: post.order_type,
                        time_in_force: post.time_in_force,
                        nonce: post.nonce,
                        client_order_id: post.client_order_id,
                        signature: String::new(),
                        created_at: now,
                        updated_at: now,
                        fills: vec![],
                        da_hash: None,
                        reasoning_trace_hash: None,
                        agent_id: None,
                    }
                });
            }
            wal::ORDER_PROCESSED => {
                let processed = entry.decode::<WalOrderProcessed>().with_context(|| {
                    format!("decode ORDER_PROCESSED at WAL sequence {}", entry.sequence)
                })?;
                if let Some(order) = recovered_orders.get_mut(&processed.order_id) {
                    order.filled_quantity = processed.filled_quantity;
                    order.status = processed.result.clone();
                }
            }
            wal::ORDER_CANCEL => {
                let cancel = entry.decode::<WalOrderCancel>().with_context(|| {
                    format!("decode ORDER_CANCEL at WAL sequence {}", entry.sequence)
                })?;
                if let Some(order) = recovered_orders.get_mut(&cancel.order_id) {
                    order.status = "canceled".to_string();
                    order.updated_at = entry.timestamp_ns / 1_000_000;
                }
            }
            wal::FILL_CREATED => {
                let fill = entry.decode::<WalFillCreated>().with_context(|| {
                    format!("decode FILL_CREATED at WAL sequence {}", entry.sequence)
                })?;
                if !fill_ids_seen.contains(&fill.fill_id) {
                    fill_ids_seen.insert(fill.fill_id.clone());
                    let fill_id = fill.fill_id.clone();
                    let sf = StoredFill {
                        id: fill.fill_id,
                        market_id: fill.market_id,
                        price: fill.price,
                        quantity: fill.quantity,
                        maker_order_id: fill.maker_order_id,
                        taker_order_id: fill.taker_order_id,
                        maker_address: fill.maker_address.clone(),
                        taker_address: fill.taker_address.clone(),
                        timestamp: entry.timestamp_ns / 1_000_000,
                        side: String::new(),
                        synthetic: false,
                        toxicity_score: 0.0,
                    };
                    if let Some(order) = recovered_orders.get_mut(&fill.taker_order_id) {
                        order.fills.push(OrderFillRecord {
                            fill_id: fill_id.clone(),
                            counterparty_order_id: fill.maker_order_id,
                            counterparty_address: fill.maker_address,
                            price: fill.price,
                            quantity: fill.quantity,
                            timestamp: entry.timestamp_ns / 1_000_000,
                        });
                    }
                    if let Some(order) = recovered_orders.get_mut(&fill.maker_order_id) {
                        order.fills.push(OrderFillRecord {
                            fill_id,
                            counterparty_order_id: fill.taker_order_id,
                            counterparty_address: fill.taker_address,
                            price: fill.price,
                            quantity: fill.quantity,
                            timestamp: entry.timestamp_ns / 1_000_000,
                        });
                    }
                    recovered_fills.push(sf);
                }
            }
            wal::DEPOSIT => {
                let dep = entry.decode::<WalDeposit>().with_context(|| {
                    format!("decode DEPOSIT at WAL sequence {}", entry.sequence)
                })?;
                let user = types::UserId::from_hex(&dep.user).with_context(|| {
                    format!("decode DEPOSIT user at WAL sequence {}", entry.sequence)
                })?;
                let asset = types::AssetId::from_str(&dep.asset);
                let mut hash = [0u8; 32];
                if let Some(tx) = &dep.tx_hash {
                    let hex_str = tx.strip_prefix("0x").unwrap_or(tx);
                    let bytes = hex::decode(hex_str).with_context(|| {
                        format!(
                            "decode DEPOSIT transaction hash at WAL sequence {}",
                            entry.sequence
                        )
                    })?;
                    if bytes.len() != 32 {
                        bail!(
                            "DEPOSIT transaction hash at WAL sequence {} is not 32 bytes",
                            entry.sequence
                        );
                    }
                    hash.copy_from_slice(&bytes);
                }
                let responses = engine.process(
                    types::Request::Deposit(DepositRequest {
                        user,
                        asset,
                        amount: dep.amount,
                        l1_tx_hash: hash,
                        source_chain: Default::default(),
                        bridge_id: None,
                    }),
                    0,
                );
                reject_replay_errors(&responses, entry.sequence, "DEPOSIT")?;
            }
            wal::WITHDRAWAL_REQUEST => {
                let wr = entry.decode::<WalWithdrawalRequest>().with_context(|| {
                    format!(
                        "decode WITHDRAWAL_REQUEST at WAL sequence {}",
                        entry.sequence
                    )
                })?;
                let user = types::UserId::from_hex(&wr.user).with_context(|| {
                    format!(
                        "decode WITHDRAWAL_REQUEST user at WAL sequence {}",
                        entry.sequence
                    )
                })?;
                let asset = types::AssetId::from_str(&wr.asset);
                let responses = engine.process(
                    types::Request::Withdrawal(WithdrawalRequest {
                        user,
                        asset,
                        amount: wr.amount,
                        nonce: wr.nonce,
                        signature: vec![],
                    }),
                    0,
                );
                reject_replay_errors(&responses, entry.sequence, "WITHDRAWAL_REQUEST")?;
            }
            wal::BALANCE_CHANGE => {
                entry.decode::<WalBalanceChange>().with_context(|| {
                    format!("decode BALANCE_CHANGE at WAL sequence {}", entry.sequence)
                })?;
            }
            wal::CHECKPOINT => {
                entry.decode::<WalCheckpoint>().with_context(|| {
                    format!("decode CHECKPOINT at WAL sequence {}", entry.sequence)
                })?;
            }
            wal::ENGINE_START => {
                entry.decode::<WalEngineStart>().with_context(|| {
                    format!("decode ENGINE_START at WAL sequence {}", entry.sequence)
                })?;
            }
            wal::ENGINE_STOP => {
                entry.decode::<WalEngineStop>().with_context(|| {
                    format!("decode ENGINE_STOP at WAL sequence {}", entry.sequence)
                })?;
            }
            unsupported => bail!(
                "unsupported WAL entry type {unsupported} at sequence {}",
                entry.sequence
            ),
        }
    }

    tracing::info!(
        "WAL replay complete: replayed {replayed} entries, recovered {} orders",
        recovered_orders.len()
    );
    Ok((recovered_fills, recovered_orders))
}

fn reject_replay_errors(responses: &[Response], sequence: u64, entry_type: &str) -> Result<()> {
    if let Some(Response::Error(error)) = responses
        .iter()
        .find(|response| matches!(response, Response::Error(_)))
    {
        bail!(
            "{entry_type} at WAL sequence {sequence} could not be restored: {:?}",
            error.code
        );
    }
    Ok(())
}

use crate::types::{Decision, Incident, OrderFillRecord, RegisteredMM, StoredFill, StoredOrder};
use crate::wal;
use crate::wal::{
    Wal, WalBalanceChange, WalCheckpoint, WalDeposit, WalEngineStart, WalEngineStop,
    WalFillCreated, WalMutationBatch, WalOrderCancel, WalOrderPost, WalOrderProcessed,
    WalWithdrawalRequest,
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
pub const CURRENT_SNAPSHOT_VERSION: u32 = 3;

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
    pub ledger_recovery: Option<ledger::JournalRecoveryWatermark>,
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
    let path = snapshot_path();
    let health = Arc::clone(&state.storage_health);
    let result = save_snapshot_to_path(state, clean_shutdown, Path::new(&path)).await;
    if let Err(error) = &result {
        if health.is_healthy() {
            health.fence("snapshot_save", error);
        }
    }
    result
}

async fn save_snapshot_to_path(
    state: Arc<crate::AppState>,
    clean_shutdown: bool,
    path: &Path,
) -> Result<()> {
    let _mutation_guard = state.mutation_gate.lock().await;
    if !state.storage_health.is_healthy() {
        bail!("durable storage is fenced; refusing to publish a snapshot");
    }

    let (timestamp, balances, orders, markets, sequence, metadata, fee_balances) = {
        let user_state = state.shards.user_state.read().await;

        let mut balances: HashMap<String, HashMap<String, SerializedBalance>> = HashMap::new();
        for ((user, asset), balance) in &user_state.balances {
            balances.entry(user.to_hex()).or_default().insert(
                asset.as_str().to_string(),
                SerializedBalance {
                    available: balance.available,
                    locked: balance.locked,
                },
            );
        }

        let mut shard_ids: Vec<_> = state.shards.shards.keys().cloned().collect();
        shard_ids.sort_by(|left, right| left.0.cmp(&right.0));
        let mut orders: HashMap<String, Vec<Order>> = HashMap::new();
        let mut timestamp = 0;
        for market_id in shard_ids {
            let shard = state
                .shards
                .shards
                .get(&market_id)
                .expect("market id came from shard map")
                .lock()
                .await;
            timestamp = timestamp.max(shard.engine.timestamp);
            let book = shard
                .engine
                .order_books
                .get(&market_id)
                .ok_or_else(|| anyhow::anyhow!("shard missing order book for {market_id}"))?;
            orders.insert(market_id.0, book.all_orders());
        }

        let markets: Vec<Market> = user_state.markets.values().cloned().collect();

        let mut metadata: HashMap<String, UserMetadata> = HashMap::new();
        for (user, meta) in &user_state.metadata {
            metadata.insert(user.to_hex(), meta.clone());
        }

        let fee_balances: HashMap<String, u64> = user_state
            .fee_balances
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let sequence = user_state.next_order_id;

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

    let ledger_recovery = match &state.ledger {
        Some(ledger) => Some(
            ledger
                .journal_recovery_watermark()
                .await
                .context("read authoritative journal recovery watermark")?,
        ),
        None => None,
    };

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
        ledger_recovery,
        incidents,
        decisions,
        registered_mms,
        proofs,
        attestations,
    };

    let json = serde_json::to_vec(&snapshot)?;

    let dir = path.parent().unwrap_or(std::path::Path::new("/data"));
    tokio::fs::create_dir_all(dir).await?;

    let tmp_path = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    let publish_result: Result<()> = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&json).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&tmp_path, path).await?;
        let dir_to_sync = dir.to_path_buf();
        tokio::task::spawn_blocking(move || std::fs::File::open(dir_to_sync)?.sync_all())
            .await
            .map_err(|error| anyhow::anyhow!("snapshot directory sync task failed: {error}"))??;
        Ok(())
    }
    .await;
    if let Err(error) = publish_result {
        state.storage_health.fence("snapshot_publish", &error);
        return Err(error).context("publish durable engine snapshot");
    }

    tracing::info!("Snapshot saved: {n_orders} orders, {n_users} users");

    let checkpoint = WalCheckpoint {
        snapshot_path: path.display().to_string(),
        total_orders: n_orders as u64,
        total_fills: n_fills as u64,
        total_users: n_users as u64,
    };
    if let Err(error) = state.wal.append(wal::CHECKPOINT, &checkpoint).await {
        state.storage_health.fence("snapshot_checkpoint", &error);
        return Err(error).context("append snapshot WAL checkpoint");
    }
    if let Err(error) = state.wal.flush().await {
        state.storage_health.fence("snapshot_checkpoint", &error);
        return Err(error).context("flush snapshot WAL checkpoint");
    }
    if let Err(error) = state.wal.rotate().await {
        state.storage_health.fence("snapshot_rotate", &error);
        return Err(error).context("rotate WAL checkpoint");
    }

    // A clean recovery bundle is published only after ENGINE_STOP has been
    // flushed. Periodic checkpoints can be replicated here while the mutation
    // gate still proves that the snapshot and WAL boundary cannot move.
    if !clean_shutdown {
        publish_recovery_replica(&state, path).await?;
    }

    Ok(())
}

pub async fn replicate_clean_shutdown(state: Arc<crate::AppState>) -> Result<()> {
    let _mutation_guard = state.mutation_gate.lock().await;
    if !state.storage_health.is_healthy() {
        bail!("durable storage is fenced; refusing to replicate clean recovery state");
    }
    let path = snapshot_path();
    publish_recovery_replica(&state, Path::new(&path)).await
}

async fn publish_recovery_replica(state: &crate::AppState, path: &Path) -> Result<()> {
    let Some(replica) = &state.recovery_replica else {
        return Ok(());
    };
    let generation = state
        .engine_leadership
        .as_ref()
        .map(|leadership| leadership.token().generation);
    match replica
        .publish(path, state.wal.directory(), generation)
        .await
    {
        Ok(manifest) => {
            tracing::info!(
                bundle_id = %manifest.bundle_id,
                checkpoint_sequence = manifest.checkpoint_sequence,
                clean_shutdown = manifest.clean_shutdown,
                "engine recovery bundle replicated"
            );
            Ok(())
        }
        Err(error) => {
            state.storage_health.fence("recovery_replication", &error);
            Err(error).context("replicate engine recovery bundle")
        }
    }
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
        } else if snapshot.version == 2 {
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

pub(crate) fn validate_snapshot(snapshot: &EngineSnapshot) -> Result<()> {
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
            wal::MUTATION_BATCH => {
                let batch = entry.decode::<WalMutationBatch>().with_context(|| {
                    format!("decode MUTATION_BATCH at WAL sequence {}", entry.sequence)
                })?;
                if batch.requests.len() != batch.responses.len() {
                    bail!(
                        "MUTATION_BATCH at WAL sequence {} has mismatched request/response counts",
                        entry.sequence
                    );
                }
                for ((request, timestamp), expected) in
                    batch.requests.into_iter().zip(batch.responses)
                {
                    let actual = engine.process(request, timestamp);
                    if rmp_serde::to_vec(&actual)? != rmp_serde::to_vec(&expected)? {
                        bail!(
                            "MUTATION_BATCH at WAL sequence {} replay diverged",
                            entry.sequence
                        );
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use types::{FeeConfig, Market, MarketId, RuntimeMode};

    #[tokio::test]
    async fn snapshot_captures_committed_user_state_instead_of_bootstrap_engine() {
        if std::env::var("ADMIN_TOKEN").is_err() {
            std::env::set_var("ADMIN_TOKEN", "snapshot-test-admin-token");
        }
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("vela_snapshot_state_{suffix}"));
        let wal = Arc::new(Wal::new(&root.join("wal")).unwrap());

        let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);
        engine.add_market(Market {
            id: MarketId::new("ETH", "USDC"),
            base: AssetId::from_str("ETH"),
            quote: AssetId::from_str("USDC"),
            max_orders: 100,
            min_order_size: 1,
            price_tick: 1,
            quantity_tick: 1,
            maker_fee_bps: -1,
            taker_fee_bps: 5,
        });
        let state = crate::AppState::new_with_runtime(engine, wal, RuntimeMode::Development);
        let user = UserId([7; 20]);
        let asset = AssetId::from_str("USDC");
        state.shards.user_state.write().await.balances.insert(
            (user.clone(), asset),
            Balance {
                user: user.clone(),
                asset,
                available: 42,
                locked: 3,
            },
        );
        assert!(state.engine.lock().await.balances.is_empty());

        let path = root.join("engine_snapshot.json");
        save_snapshot_to_path(Arc::clone(&state), false, &path)
            .await
            .unwrap();
        let snapshot = load_snapshot_from(&path).await.unwrap().unwrap();
        let saved = &snapshot.balances[&user.to_hex()]["USDC"];
        assert_eq!(saved.available, 42);
        assert_eq!(saved.locked, 3);
    }
}

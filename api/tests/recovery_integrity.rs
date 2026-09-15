use api::snapshot::{load_recovery_snapshot, replay_wal_entries, restore_engine_from_snapshot};
use api::wal::{Wal, WalCheckpoint, WalOrderPost, CHECKPOINT, ORDER_POST};
use engine::MatchingEngine;
use serde_json::json;
use std::path::{Path, PathBuf};
use types::FeeConfig;

static TEMP_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let n = TEMP_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "vela_recovery_{label}_{}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_empty_snapshot(path: &Path, version: u32, wal_sequence: u64, clean_shutdown: bool) {
    let mut snapshot = json!({
        "version": version,
        "timestamp": 1,
        "balances": {},
        "orders": {},
        "markets": [],
        "sequence": 1,
        "wal_sequence": wal_sequence,
        "metadata": {},
        "clean_shutdown": clean_shutdown
    });
    if version == 1 {
        snapshot.as_object_mut().unwrap().remove("wal_sequence");
    }
    std::fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
}

#[tokio::test]
async fn genuinely_fresh_directory_can_start_without_snapshot() {
    let dir = temp_dir("fresh");
    let recovery = Wal::inspect(&dir).unwrap();
    assert!(!recovery.has_prior_evidence);
    assert!(
        load_recovery_snapshot(&dir.join("engine_snapshot.json"), &recovery)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn missing_snapshot_with_prior_wal_evidence_fails_closed() {
    let dir = temp_dir("missing_snapshot");
    let wal = Wal::new(&dir).unwrap();
    wal.append(
        CHECKPOINT,
        &WalCheckpoint {
            snapshot_path: dir.join("engine_snapshot.json").display().to_string(),
            total_orders: 0,
            total_fills: 0,
            total_users: 0,
        },
    )
    .await
    .unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let recovery = Wal::inspect(&dir).unwrap();
    let error = load_recovery_snapshot(&dir.join("engine_snapshot.json"), &recovery)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("snapshot is missing"));
}

#[tokio::test]
async fn corrupt_and_unsupported_snapshots_fail_closed() {
    let corrupt_dir = temp_dir("corrupt_snapshot");
    let corrupt_path = corrupt_dir.join("engine_snapshot.json");
    std::fs::write(&corrupt_path, b"{not-json").unwrap();
    let error = api::snapshot::load_snapshot_from(&corrupt_path)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("parse snapshot"));

    let unsupported_dir = temp_dir("unsupported_snapshot");
    let unsupported_path = unsupported_dir.join("engine_snapshot.json");
    write_empty_snapshot(&unsupported_path, 999, 0, false);
    let error = api::snapshot::load_snapshot_from(&unsupported_path)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unsupported snapshot version"));
}

#[tokio::test]
async fn snapshot_orders_must_reference_a_restorable_market() {
    let dir = temp_dir("unknown_order_market");
    let path = dir.join("engine_snapshot.json");
    let snapshot = json!({
        "version": api::snapshot::CURRENT_SNAPSHOT_VERSION,
        "timestamp": 1,
        "balances": {},
        "orders": {
            "MISSING-USDC": [{
                "id": 1,
                "user": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                "market": "MISSING-USDC",
                "side": "Bid",
                "order_type": "GoodTillCanceled",
                "price": 1,
                "quantity": 1,
                "filled_quantity": 0,
                "nonce": 1,
                "client_order_id": null,
                "timestamp": 1,
                "status": "Open"
            }]
        },
        "markets": [],
        "sequence": 2,
        "wal_sequence": 0,
        "metadata": {}
    });
    std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let error = api::snapshot::load_snapshot_from(&path).await.unwrap_err();
    assert!(error.to_string().contains("unknown market"));
}

#[tokio::test]
async fn strict_replay_tool_accepts_a_valid_snapshot_and_wal() {
    let dir = temp_dir("valid");
    let snapshot_path = dir.join("engine_snapshot.json");
    write_empty_snapshot(
        &snapshot_path,
        api::snapshot::CURRENT_SNAPSHOT_VERSION,
        0,
        false,
    );

    let wal = Wal::new(&dir).unwrap();
    wal.append(
        CHECKPOINT,
        &WalCheckpoint {
            snapshot_path: snapshot_path.display().to_string(),
            total_orders: 0,
            total_fills: 0,
            total_users: 0,
        },
    )
    .await
    .unwrap();
    wal.append(
        ORDER_POST,
        &WalOrderPost {
            order_id: 42,
            user: "0x0000000000000000000000000000000000000001".to_string(),
            market_id: "ETH-USDC".to_string(),
            side: "bid".to_string(),
            price: 3_000,
            quantity: 1,
            order_type: "limit".to_string(),
            time_in_force: "gtc".to_string(),
            nonce: 7,
            client_order_id: Some("recovered-42".to_string()),
        },
    )
    .await
    .unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let snapshot = api::snapshot::load_snapshot_from(&snapshot_path)
        .await
        .unwrap()
        .unwrap();
    let after_sequence = snapshot.wal_sequence;
    let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    restore_engine_from_snapshot(&mut engine, snapshot).unwrap();
    let (fills, orders) = replay_wal_entries(&mut engine, &dir, after_sequence).unwrap();
    assert!(fills.is_empty());
    assert_eq!(
        orders.get(&42).unwrap().client_order_id.as_deref(),
        Some("recovered-42")
    );
}

#[tokio::test]
async fn clean_version_one_snapshot_is_migrated_at_a_verified_boundary() {
    let dir = temp_dir("clean_set");
    let snapshot_path = dir.join("engine_snapshot.json");
    write_empty_snapshot(&snapshot_path, 1, 0, true);
    let wal = Wal::new(&dir).unwrap();
    wal.append(
        CHECKPOINT,
        &WalCheckpoint {
            snapshot_path: snapshot_path.display().to_string(),
            total_orders: 0,
            total_fills: 0,
            total_users: 0,
        },
    )
    .await
    .unwrap();
    wal.append(
        api::wal::ENGINE_STOP,
        &api::wal::WalEngineStop {
            reason: "clean".to_owned(),
            final_sequence: 1,
        },
    )
    .await
    .unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let recovery = Wal::inspect(&dir).unwrap();
    let migrated = load_recovery_snapshot(&snapshot_path, &recovery)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(migrated.version, api::snapshot::CURRENT_SNAPSHOT_VERSION);
    assert_eq!(migrated.wal_sequence, 0);
}

#[tokio::test]
async fn clean_checkpoint_for_a_different_snapshot_is_rejected() {
    let dir = temp_dir("mismatched_checkpoint");
    let snapshot_path = dir.join("engine_snapshot.json");
    write_empty_snapshot(
        &snapshot_path,
        api::snapshot::CURRENT_SNAPSHOT_VERSION,
        0,
        true,
    );
    let wal = Wal::new(&dir).unwrap();
    wal.append(
        CHECKPOINT,
        &WalCheckpoint {
            snapshot_path: dir.join("other_snapshot.json").display().to_string(),
            total_orders: 0,
            total_fills: 0,
            total_users: 0,
        },
    )
    .await
    .unwrap();
    wal.append(
        api::wal::ENGINE_STOP,
        &api::wal::WalEngineStop {
            reason: "clean".to_owned(),
            final_sequence: 1,
        },
    )
    .await
    .unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let recovery = Wal::inspect(&dir).unwrap();
    let error = load_recovery_snapshot(&snapshot_path, &recovery)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("different snapshot"));
}

#[tokio::test]
async fn unclean_snapshot_and_wal_set_requires_operator_recovery() {
    let dir = temp_dir("unclean_set");
    let snapshot_path = dir.join("engine_snapshot.json");
    write_empty_snapshot(
        &snapshot_path,
        api::snapshot::CURRENT_SNAPSHOT_VERSION,
        0,
        false,
    );
    let wal = Wal::new(&dir).unwrap();
    wal.append(
        CHECKPOINT,
        &WalCheckpoint {
            snapshot_path: snapshot_path.display().to_string(),
            total_orders: 0,
            total_fills: 0,
            total_users: 0,
        },
    )
    .await
    .unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let recovery = Wal::inspect(&dir).unwrap();
    let error = load_recovery_snapshot(&snapshot_path, &recovery)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("automatic recovery from an unclean"));
}

#[tokio::test]
async fn undecodable_recovery_entry_fails_closed() {
    let dir = temp_dir("undecodable");
    let wal = Wal::new(&dir).unwrap();
    wal.append(ORDER_POST, &"not an order").await.unwrap();
    wal.flush().await.unwrap();
    drop(wal);

    let mut engine = MatchingEngine::new(FeeConfig::default(), 5.0);
    let error = replay_wal_entries(&mut engine, &dir, 0).unwrap_err();
    assert!(error.to_string().contains("decode ORDER_POST"));
}

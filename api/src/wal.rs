use anyhow::{bail, Context, Result};
use crc::{Crc, CRC_64_ECMA_182};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

const CRC64: Crc<u64> = Crc::<u64>::new(&CRC_64_ECMA_182);

pub const ORDER_POST: u32 = 1;
pub const ORDER_PROCESSED: u32 = 2;
pub const ORDER_CANCEL: u32 = 3;
pub const FILL_CREATED: u32 = 4;
pub const DEPOSIT: u32 = 5;
pub const WITHDRAWAL_REQUEST: u32 = 6;
pub const BALANCE_CHANGE: u32 = 7;
pub const CHECKPOINT: u32 = 8;
pub const ENGINE_START: u32 = 9;
pub const ENGINE_STOP: u32 = 10;

const DEFAULT_MAX_SEGMENT_SIZE: u64 = 50 * 1024 * 1024;
const MAX_PAYLOAD_SIZE: u32 = 10 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
pub struct WalOrderPost {
    pub order_id: u64,
    pub user: String,
    pub market_id: String,
    pub side: String,
    pub price: u64,
    pub quantity: u64,
    pub order_type: String,
    pub time_in_force: String,
    pub nonce: u64,
    pub client_order_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WalOrderProcessed {
    pub order_id: u64,
    pub result: String,
    pub fill_ids: Vec<String>,
    pub filled_quantity: u64,
    pub rejection_reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WalOrderCancel {
    pub order_id: u64,
    pub client_order_id: Option<String>,
    pub user: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
pub struct WalFillCreated {
    pub fill_id: String,
    pub market_id: String,
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub maker_address: String,
    pub taker_address: String,
    pub price: u64,
    pub quantity: u64,
    pub maker_fee: i64,
    pub taker_fee: u64,
}

#[derive(Serialize, Deserialize)]
pub struct WalDeposit {
    pub user: String,
    pub asset: String,
    pub amount: u64,
    pub tx_hash: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WalWithdrawalRequest {
    pub user: String,
    pub asset: String,
    pub amount: u64,
    pub nonce: u64,
}

#[derive(Serialize, Deserialize)]
pub struct WalBalanceChange {
    pub user: String,
    pub asset: String,
    pub old_balance: u64,
    pub new_balance: u64,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WalCheckpoint {
    pub snapshot_path: String,
    pub total_orders: u64,
    pub total_fills: u64,
    pub total_users: u64,
}

#[derive(Serialize, Deserialize)]
pub struct WalEngineStart {
    pub version: String,
    pub reason: String,
    pub previous_sequence: u64,
}

#[derive(Serialize, Deserialize)]
pub struct WalEngineStop {
    pub reason: String,
    pub final_sequence: u64,
}

#[derive(Debug)]
pub struct WalEntry {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub entry_type: u32,
    pub payload_bytes: Vec<u8>,
}

impl WalEntry {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        rmp_serde::from_slice(&self.payload_bytes)
            .map_err(|e| anyhow::anyhow!("WAL decode error: {e}"))
    }
}

pub struct WalStats {
    pub current_sequence: u64,
    pub current_segment: String,
    pub segment_size_bytes: u64,
    pub last_checkpoint_sequence: u64,
    pub last_checkpoint_time: u64,
    pub entries_since_checkpoint: u64,
    pub last_engine_start_reason: String,
}

struct WalInner {
    file: tokio::fs::File,
    segment_number: u32,
    segment_size: u64,
    last_checkpoint_sequence: u64,
    last_checkpoint_time_ms: u64,
    last_engine_start_reason: String,
}

pub struct Wal {
    dir: PathBuf,
    inner: tokio::sync::Mutex<WalInner>,
    sequence: Arc<AtomicU64>,
    #[allow(dead_code)]
    max_segment_size: u64,
}

fn segment_path(dir: &Path, num: u32) -> PathBuf {
    dir.join(format!("engine_wal_{:04}.log", num))
}

fn segment_name(num: u32) -> String {
    format!("engine_wal_{:04}.log", num)
}

fn parse_segment_number(name: &str) -> Option<u32> {
    name.strip_prefix("engine_wal_")
        .and_then(|s| s.strip_suffix(".log"))
        .and_then(|s| s.parse().ok())
}

fn list_segments_sync(dir: &Path) -> Result<Vec<(u32, PathBuf)>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("read WAL directory"),
    };

    let mut segments = Vec::new();
    for entry in entries {
        let entry = entry.context("read WAL directory entry")?;
        let name = entry.file_name();
        if let Some(number) = parse_segment_number(&name.to_string_lossy()) {
            segments.push((number, entry.path()));
        }
    }
    segments.sort_unstable_by_key(|(number, _)| *number);
    for pair in segments.windows(2) {
        let expected = pair[0]
            .0
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("WAL segment number space is exhausted"))?;
        if pair[1].0 != expected {
            bail!(
                "WAL segment numbering gap between {} and {}",
                pair[0].0,
                pair[1].0
            );
        }
    }
    Ok(segments)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecoveryState {
    pub has_prior_evidence: bool,
    pub has_entries: bool,
    pub last_sequence: u64,
    pub checkpoint_sequence: u64,
    pub checkpoint_snapshot_path: Option<String>,
    pub clean_shutdown: bool,
}

impl Wal {
    pub fn new(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;

        let recovery = Self::inspect(dir)?;
        let segments = list_segments_sync(dir)?;
        let segment_number = segments.last().map(|(number, _)| *number).unwrap_or(0);
        let path = segment_path(dir, segment_number);

        let existing_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let std_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let file = tokio::fs::File::from_std(std_file);

        Ok(Self {
            dir: dir.to_path_buf(),
            inner: tokio::sync::Mutex::new(WalInner {
                file,
                segment_number,
                segment_size: existing_size,
                last_checkpoint_sequence: 0,
                last_checkpoint_time_ms: 0,
                last_engine_start_reason: "clean".to_string(),
            }),
            sequence: Arc::new(AtomicU64::new(recovery.last_sequence.saturating_add(1))),
            max_segment_size: DEFAULT_MAX_SEGMENT_SIZE,
        })
    }

    pub async fn open(dir: &Path) -> Result<Self> {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || Self::new(&dir))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking: {e}"))?
    }

    pub async fn append<T: Serialize>(&self, entry_type: u32, payload: &T) -> Result<u64> {
        if !(ORDER_POST..=ENGINE_STOP).contains(&entry_type) {
            bail!("unsupported WAL entry type {entry_type}");
        }
        let payload_bytes = match rmp_serde::to_vec(payload) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("WAL serialize error: {e}");
                return Err(anyhow::anyhow!("WAL serialize error: {e}"));
            }
        };

        if payload_bytes.len() > MAX_PAYLOAD_SIZE as usize {
            bail!("WAL payload exceeds maximum size");
        }

        let mut inner = self.inner.lock().await;
        let seq = self.sequence.load(Ordering::SeqCst);
        let next_sequence = seq
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("WAL sequence space is exhausted"))?;

        let ts_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let payload_len = payload_bytes.len() as u32;

        let mut header = [0u8; 24];
        header[0..8].copy_from_slice(&seq.to_be_bytes());
        header[8..16].copy_from_slice(&ts_ns.to_be_bytes());
        header[16..20].copy_from_slice(&entry_type.to_be_bytes());
        header[20..24].copy_from_slice(&payload_len.to_be_bytes());

        let mut data: Vec<u8> = Vec::with_capacity(24 + payload_bytes.len() + 8);
        data.extend_from_slice(&header);
        data.extend_from_slice(&payload_bytes);

        let checksum = CRC64.checksum(&data);
        data.extend_from_slice(&checksum.to_be_bytes());

        let entry_len = data.len() as u64;

        if let Err(e) = inner.file.write_all(&data).await {
            tracing::error!("WAL write error: {e}");
            return Err(e.into());
        }

        inner.segment_size += entry_len;
        self.sequence.store(next_sequence, Ordering::SeqCst);

        if entry_type == CHECKPOINT {
            inner.last_checkpoint_sequence = seq;
            inner.last_checkpoint_time_ms = ts_ns / 1_000_000;
        }
        if entry_type == ENGINE_START {
            if let Ok(start) = rmp_serde::from_slice::<WalEngineStart>(&payload_bytes) {
                inner.last_engine_start_reason = start.reason;
            }
        }

        Ok(seq)
    }

    /// Flush all buffered writes to disk (one `sync_all` per call).
    /// Call once after all entries for a dispatch batch have been appended
    /// to amortise fsync cost across the entire batch.
    pub async fn flush(&self) -> Result<()> {
        #[allow(unused_mut)]
        let mut inner = self.inner.lock().await;
        if let Err(e) = inner.file.sync_all().await {
            tracing::error!("WAL flush error: {e}");
            return Err(e.into());
        }
        Ok(())
    }

    pub async fn rotate(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;

        inner
            .file
            .sync_all()
            .await
            .context("sync WAL before rotation")?;
        let new_num = inner
            .segment_number
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("WAL segment number space is exhausted"))?;
        let new_path = segment_path(&self.dir, new_num);

        let std_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&new_path)
            .map_err(|e| anyhow::anyhow!("WAL rotate open error: {e}"))?;

        inner.file = tokio::fs::File::from_std(std_file);
        inner.segment_number = new_num;
        inner.segment_size = 0;

        let oldest_to_keep = new_num.saturating_sub(2);
        let dir = self.dir.clone();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(num) = parse_segment_number(&name.to_string_lossy()) {
                    if num < oldest_to_keep {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn read_from(path: &Path, after_sequence: u64) -> Result<Vec<WalEntry>> {
        let bytes =
            std::fs::read(path).with_context(|| format!("read WAL segment {}", path.display()))?;

        let mut entries = Vec::new();
        let mut cursor = 0usize;
        let mut previous_sequence: Option<u64> = None;
        while cursor < bytes.len() {
            let remaining = bytes.len() - cursor;
            if remaining < 24 {
                bail!("truncated WAL header at byte {cursor}");
            }
            let header: [u8; 24] = bytes[cursor..cursor + 24]
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid WAL header at byte {cursor}"))?;
            cursor += 24;

            let seq = u64::from_be_bytes(
                header[0..8]
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid WAL sequence"))?,
            );
            let ts_ns = u64::from_be_bytes(
                header[8..16]
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid WAL timestamp"))?,
            );
            let entry_type = u32::from_be_bytes(
                header[16..20]
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid WAL entry type"))?,
            );
            if !(ORDER_POST..=ENGINE_STOP).contains(&entry_type) {
                bail!("unsupported WAL entry type {entry_type} at sequence {seq}");
            }
            let payload_len = u32::from_be_bytes(
                header[20..24]
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid WAL payload length"))?,
            );

            if payload_len > MAX_PAYLOAD_SIZE {
                bail!("WAL payload length {payload_len} exceeds maximum at sequence {seq}");
            }
            let record_tail = payload_len as usize + 8;
            if bytes.len() - cursor < record_tail {
                bail!("truncated WAL record at sequence {seq}");
            }
            let payload = bytes[cursor..cursor + payload_len as usize].to_vec();
            cursor += payload_len as usize;
            let stored_crc = u64::from_be_bytes(
                bytes[cursor..cursor + 8]
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid WAL CRC at sequence {seq}"))?,
            );
            cursor += 8;

            let mut to_check: Vec<u8> = Vec::with_capacity(24 + payload_len as usize);
            to_check.extend_from_slice(&header);
            to_check.extend_from_slice(&payload);
            let computed_crc = CRC64.checksum(&to_check);

            if stored_crc != computed_crc {
                bail!("WAL CRC mismatch at sequence {seq}");
            }
            if let Some(previous) = previous_sequence {
                let expected = previous
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("WAL sequence overflow"))?;
                if seq != expected {
                    bail!("WAL sequence discontinuity: expected {expected}, found {seq}");
                }
            }
            previous_sequence = Some(seq);

            if seq > after_sequence {
                entries.push(WalEntry {
                    sequence: seq,
                    timestamp_ns: ts_ns,
                    entry_type,
                    payload_bytes: payload,
                });
            }
        }

        Ok(entries)
    }

    pub fn inspect(dir: &Path) -> Result<WalRecoveryState> {
        let segments = list_segments_sync(dir)?;
        let mut all_entries = Vec::new();
        let mut previous_sequence: Option<u64> = None;
        for (_, path) in &segments {
            for entry in Self::read_from(path, 0)? {
                if let Some(previous) = previous_sequence {
                    let expected = previous
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("WAL sequence overflow"))?;
                    if entry.sequence != expected {
                        bail!(
                            "WAL sequence discontinuity across segments: expected {expected}, found {}",
                            entry.sequence
                        );
                    }
                }
                previous_sequence = Some(entry.sequence);
                all_entries.push(entry);
            }
        }

        let mut checkpoint_sequence = 0;
        let mut checkpoint_snapshot_path = None;
        for entry in &all_entries {
            if entry.entry_type == CHECKPOINT {
                let checkpoint = entry
                    .decode::<WalCheckpoint>()
                    .context("decode WAL checkpoint")?;
                checkpoint_sequence = entry.sequence;
                checkpoint_snapshot_path = Some(checkpoint.snapshot_path);
            }
        }
        let clean_shutdown = match all_entries.last() {
            Some(entry) if entry.entry_type == ENGINE_STOP => {
                let stop = entry
                    .decode::<WalEngineStop>()
                    .context("decode terminal WAL stop")?;
                stop.reason == "clean" && stop.final_sequence.saturating_add(1) == entry.sequence
            }
            _ => false,
        };
        let last_sequence = previous_sequence.unwrap_or(0);
        if last_sequence == u64::MAX {
            bail!("WAL sequence space is exhausted");
        }
        let has_entries = previous_sequence.is_some();
        let has_prior_evidence = has_entries
            || segments.len() > 1
            || segments.first().is_some_and(|(number, _)| *number != 0);
        if has_prior_evidence && !has_entries {
            bail!("WAL segment history exists but contains no recoverable entries");
        }
        Ok(WalRecoveryState {
            has_prior_evidence,
            has_entries,
            last_sequence,
            checkpoint_sequence,
            checkpoint_snapshot_path,
            clean_shutdown,
        })
    }

    pub fn read_all_from(dir: &Path, after_sequence: u64) -> Result<Vec<WalEntry>> {
        let segments = list_segments_sync(dir)?;
        let mut entries = Vec::new();
        let mut previous_sequence: Option<u64> = None;
        for (_, path) in segments {
            for entry in Self::read_from(&path, 0)? {
                if let Some(previous) = previous_sequence {
                    let expected = previous
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("WAL sequence overflow"))?;
                    if entry.sequence != expected {
                        bail!(
                            "WAL sequence discontinuity across segments: expected {expected}, found {}",
                            entry.sequence
                        );
                    }
                }
                previous_sequence = Some(entry.sequence);
                if entry.sequence > after_sequence {
                    if entries.is_empty() {
                        let expected = after_sequence
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("WAL replay sequence overflow"))?;
                        if entry.sequence != expected {
                            bail!(
                                "WAL replay history gap: expected sequence {expected}, found {}",
                                entry.sequence
                            );
                        }
                    }
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }

    pub async fn find_last_checkpoint(dir: &Path) -> Result<Option<(u64, WalCheckpoint)>> {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Wal::inspect(&dir)?;
            let mut last_checkpoint: Option<(u64, WalCheckpoint)> = None;
            for (_, path) in list_segments_sync(&dir)? {
                for entry in Wal::read_from(&path, 0)? {
                    if entry.entry_type == CHECKPOINT {
                        let cp = entry.decode::<WalCheckpoint>()?;
                        last_checkpoint = Some((entry.sequence, cp));
                    }
                }
            }
            Ok(last_checkpoint)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking: {e}"))?
    }

    pub fn was_clean_shutdown_sync(dir: &Path) -> Result<bool> {
        Ok(Self::inspect(dir)?.clean_shutdown)
    }

    pub fn find_checkpoint_sequence_sync(dir: &Path) -> Result<u64> {
        Ok(Self::inspect(dir)?.checkpoint_sequence)
    }

    pub fn wal_files_exist(dir: &Path) -> Result<bool> {
        Ok(!list_segments_sync(dir)?.is_empty())
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst).saturating_sub(1)
    }

    pub async fn stats(&self) -> WalStats {
        let inner = self.inner.lock().await;
        let current_seq = self.sequence.load(Ordering::SeqCst).saturating_sub(1);
        let entries_since = current_seq.saturating_sub(inner.last_checkpoint_sequence);
        WalStats {
            current_sequence: current_seq,
            current_segment: segment_name(inner.segment_number),
            segment_size_bytes: inner.segment_size,
            last_checkpoint_sequence: inner.last_checkpoint_sequence,
            last_checkpoint_time: inner.last_checkpoint_time_ms,
            entries_since_checkpoint: entries_since,
            last_engine_start_reason: inner.last_engine_start_reason.clone(),
        }
    }
}

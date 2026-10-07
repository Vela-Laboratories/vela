use crate::snapshot::{self, EngineSnapshot};
use crate::wal::Wal;
use age::secrecy::ExposeSecret;
use anyhow::{bail, Context, Result};
use k256::ecdsa::{
    signature::hazmat::{PrehashSigner, PrehashVerifier},
    Signature, SigningKey, VerifyingKey,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroize;

pub const RECOVERY_MANIFEST_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "recovery_manifest.json";
const SIGNATURE_FILE: &str = "recovery_signature.json";
const LATEST_FILE: &str = "LATEST.json";
const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const SIGNATURE_VERSION: u32 = 1;
const SIGNATURE_ALGORITHM: &str = "secp256k1-ecdsa-sha256";
const OFFSITE_ARCHIVE_MAGIC: &[u8] = b"VELA-RECOVERY-AGE-V1\n";
const MAX_ARCHIVE_ENTRIES: u32 = 1024;
const MAX_ARCHIVE_FILE_BYTES: u64 = 1 << 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryArtifact {
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryManifest {
    pub version: u32,
    pub bundle_id: String,
    pub created_at_ms: u64,
    pub build_id: String,
    pub leadership_generation: Option<i64>,
    pub original_snapshot_path: String,
    pub snapshot_version: u32,
    pub snapshot_wal_sequence: u64,
    pub checkpoint_sequence: u64,
    pub last_sequence: u64,
    pub clean_shutdown: bool,
    #[serde(default)]
    pub ledger_recovery: Option<ledger::JournalRecoveryWatermark>,
    pub artifacts: Vec<RecoveryArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestRecoveryBundle {
    pub version: u32,
    pub bundle_id: String,
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryManifestSignature {
    pub version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    pub manifest_sha256: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryReplicationSnapshot {
    pub configured: bool,
    pub healthy: bool,
    pub last_success_at_ms: u64,
    pub last_checkpoint_sequence: u64,
    pub failure_count: u64,
    pub failure_code: Option<String>,
    pub signed: bool,
    pub offsite_configured: bool,
}

#[derive(Clone)]
struct RecoverySigner {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    public_key_hex: String,
    key_id: String,
}

#[derive(Clone)]
struct OffsiteTarget {
    destination: PathBuf,
    recipient: age::x25519::Recipient,
}

impl RecoverySigner {
    fn from_env() -> Result<Self, String> {
        let mut signing_key = nonempty_env("VELA_RECOVERY_SIGNING_KEY_HEX").ok_or_else(|| {
            "VELA_RECOVERY_SIGNING_KEY_HEX is required when recovery replication is configured"
                .to_owned()
        })?;
        let verifying_key = nonempty_env("VELA_RECOVERY_VERIFYING_KEY_HEX").ok_or_else(|| {
            "VELA_RECOVERY_VERIFYING_KEY_HEX is required when recovery replication is configured"
                .to_owned()
        })?;
        let result =
            Self::from_hex(&signing_key, &verifying_key).map_err(|error| error.to_string());
        signing_key.zeroize();
        result
    }

    fn from_hex(signing_key_hex: &str, verifying_key_hex: &str) -> Result<Self> {
        let mut signing_bytes = decode_hex(signing_key_hex, "recovery signing key")?;
        if signing_bytes.len() != 32 {
            signing_bytes.zeroize();
            bail!("recovery signing key must be exactly 32 bytes");
        }
        let signing_key_result = SigningKey::from_slice(&signing_bytes)
            .map_err(|_| anyhow::anyhow!("recovery signing key is invalid"));
        signing_bytes.zeroize();
        let signing_key = signing_key_result?;
        let configured_verifying_bytes = decode_hex(verifying_key_hex, "recovery verifying key")?;
        let configured_verifying_key =
            VerifyingKey::from_sec1_bytes(&configured_verifying_bytes)
                .map_err(|_| anyhow::anyhow!("recovery verifying key is invalid"))?;
        let derived_verifying_key = VerifyingKey::from(&signing_key);
        if configured_verifying_key.to_encoded_point(true)
            != derived_verifying_key.to_encoded_point(true)
        {
            bail!("recovery signing and verifying keys do not match");
        }
        let public_key = derived_verifying_key.to_encoded_point(true);
        let public_key_hex = hex::encode(public_key.as_bytes());
        let key_id = key_id(public_key.as_bytes());
        Ok(Self {
            signing_key,
            verifying_key: derived_verifying_key,
            public_key_hex,
            key_id,
        })
    }

    fn generate() -> Self {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = VerifyingKey::from(&signing_key);
        let public_key = verifying_key.to_encoded_point(true);
        Self {
            signing_key,
            verifying_key,
            public_key_hex: hex::encode(public_key.as_bytes()),
            key_id: key_id(public_key.as_bytes()),
        }
    }

    fn sign_manifest(&self, manifest_bytes: &[u8]) -> Result<RecoveryManifestSignature> {
        let digest = Sha256::digest(manifest_bytes);
        let signature: Signature = self
            .signing_key
            .sign_prehash(&digest)
            .map_err(|_| anyhow::anyhow!("sign recovery manifest"))?;
        Ok(RecoveryManifestSignature {
            version: SIGNATURE_VERSION,
            algorithm: SIGNATURE_ALGORITHM.to_owned(),
            key_id: self.key_id.clone(),
            public_key: self.public_key_hex.clone(),
            manifest_sha256: hex::encode(digest),
            signature: hex::encode(signature.to_bytes()),
        })
    }
}

impl OffsiteTarget {
    fn from_env(snapshot_dir: &Path, replica_dir: &Path) -> Result<Option<Self>, String> {
        let destination = nonempty_env("VELA_ENGINE_OFFSITE_DIR");
        let recipient = nonempty_env("VELA_RECOVERY_AGE_RECIPIENT");
        match (destination, recipient) {
            (None, None) => Ok(None),
            (Some(_), None) => Err(
                "VELA_RECOVERY_AGE_RECIPIENT is required when VELA_ENGINE_OFFSITE_DIR is configured"
                    .to_owned(),
            ),
            (None, Some(_)) => Err(
                "VELA_ENGINE_OFFSITE_DIR is required when VELA_RECOVERY_AGE_RECIPIENT is configured"
                    .to_owned(),
            ),
            (Some(destination), Some(recipient)) => {
                let destination = PathBuf::from(destination);
                validate_distinct_directories(snapshot_dir, &destination)?;
                validate_distinct_directories(replica_dir, &destination)?;
                fs::create_dir_all(&destination).map_err(|error| {
                    format!(
                        "create engine offsite directory {}: {error}",
                        destination.display()
                    )
                })?;
                validate_distinct_directories(snapshot_dir, &destination)?;
                validate_distinct_directories(replica_dir, &destination)?;
                set_private_directory_permissions(&destination)
                    .map_err(|error| error.to_string())?;
                sync_directory(&destination).map_err(|error| error.to_string())?;
                let recipient = age::x25519::Recipient::from_str(&recipient)
                    .map_err(|_| "VELA_RECOVERY_AGE_RECIPIENT is invalid".to_owned())?;
                Ok(Some(Self {
                    destination,
                    recipient,
                }))
            }
        }
    }
}

pub struct RecoveryReplica {
    destination: PathBuf,
    build_id: String,
    timeout: Duration,
    signer: RecoverySigner,
    offsite: Option<OffsiteTarget>,
    copy_lock: Arc<Mutex<()>>,
    healthy: AtomicBool,
    last_success_at_ms: AtomicU64,
    last_checkpoint_sequence: AtomicU64,
    failure_count: AtomicU64,
    failure_code: Mutex<Option<String>>,
}

impl RecoveryReplica {
    pub fn from_env(snapshot_dir: &Path) -> Result<Option<Arc<Self>>, String> {
        let Some(destination) = nonempty_env("VELA_ENGINE_REPLICA_DIR") else {
            return Ok(None);
        };
        let build_id = nonempty_env("VELA_BUILD_ID").ok_or_else(|| {
            "VELA_BUILD_ID is required when VELA_ENGINE_REPLICA_DIR is configured".to_owned()
        })?;
        if build_id.len() > 128
            || !build_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || matches!(
                build_id.to_ascii_lowercase().as_str(),
                "unknown" | "latest" | "dev"
            )
        {
            return Err(
                "VELA_BUILD_ID must be a specific 1-128 character build identity using ASCII letters, digits, '.', '_', or '-'".to_owned(),
            );
        }
        let destination = PathBuf::from(destination);
        validate_distinct_directories(snapshot_dir, &destination)?;
        let timeout_ms = nonempty_env("VELA_ENGINE_REPLICA_TIMEOUT_MS")
            .map(|raw| {
                raw.parse::<u64>()
                    .map_err(|_| "VELA_ENGINE_REPLICA_TIMEOUT_MS must be an integer".to_owned())
            })
            .transpose()?
            .unwrap_or(30_000);
        if timeout_ms < 1_000 {
            return Err("VELA_ENGINE_REPLICA_TIMEOUT_MS must be at least 1000ms".to_owned());
        }
        let signer = RecoverySigner::from_env()?;
        let offsite = OffsiteTarget::from_env(snapshot_dir, &destination)?;
        fs::create_dir_all(&destination).map_err(|error| {
            format!(
                "create engine replica directory {}: {error}",
                destination.display()
            )
        })?;
        validate_distinct_directories(snapshot_dir, &destination)?;
        if let Some(offsite) = &offsite {
            validate_distinct_directories(snapshot_dir, &offsite.destination)?;
            validate_distinct_directories(&destination, &offsite.destination)?;
        }
        set_private_directory_permissions(&destination).map_err(|error| error.to_string())?;
        sync_directory(&destination).map_err(|error| error.to_string())?;
        Ok(Some(Arc::new(Self::new(
            destination,
            build_id,
            Duration::from_millis(timeout_ms),
            signer,
            offsite,
        ))))
    }

    fn new(
        destination: PathBuf,
        build_id: String,
        timeout: Duration,
        signer: RecoverySigner,
        offsite: Option<OffsiteTarget>,
    ) -> Self {
        Self {
            destination,
            build_id,
            timeout,
            signer,
            offsite,
            copy_lock: Arc::new(Mutex::new(())),
            healthy: AtomicBool::new(true),
            last_success_at_ms: AtomicU64::new(0),
            last_checkpoint_sequence: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
            failure_code: Mutex::new(None),
        }
    }

    pub async fn publish(
        &self,
        snapshot_path: &Path,
        wal_dir: &Path,
        leadership_generation: Option<i64>,
    ) -> Result<RecoveryManifest> {
        if !self.healthy.load(Ordering::Acquire) {
            bail!("engine recovery replication is fenced");
        }
        let destination = self.destination.clone();
        let build_id = self.build_id.clone();
        let signer = self.signer.clone();
        let offsite = self.offsite.clone();
        let snapshot_path = snapshot_path.to_path_buf();
        let wal_dir = wal_dir.to_path_buf();
        let copy_lock = Arc::clone(&self.copy_lock);
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancelled));
        let task_cancelled = Arc::clone(&cancelled);
        let task = tokio::task::spawn_blocking(move || {
            let _guard = copy_lock
                .lock()
                .map_err(|_| anyhow::anyhow!("engine replica copy lock is poisoned"))?;
            if task_cancelled.load(Ordering::Acquire) {
                bail!("engine recovery replication was cancelled");
            }
            publish_bundle_sync(
                &snapshot_path,
                &wal_dir,
                &destination,
                &build_id,
                leadership_generation,
                &signer,
                offsite.as_ref(),
                &task_cancelled,
            )
        });
        let result = match tokio::time::timeout(self.timeout, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(anyhow::anyhow!("engine replica task failed: {error}")),
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(anyhow::anyhow!("engine recovery replication timed out"))
            }
        };
        match result {
            Ok(manifest) => {
                self.last_success_at_ms
                    .store(manifest.created_at_ms, Ordering::Release);
                self.last_checkpoint_sequence
                    .store(manifest.checkpoint_sequence, Ordering::Release);
                Ok(manifest)
            }
            Err(error) => {
                self.record_failure("replication_failed");
                Err(error)
            }
        }
    }

    pub fn snapshot(&self) -> RecoveryReplicationSnapshot {
        RecoveryReplicationSnapshot {
            configured: true,
            healthy: self.healthy.load(Ordering::Acquire),
            last_success_at_ms: self.last_success_at_ms.load(Ordering::Acquire),
            last_checkpoint_sequence: self.last_checkpoint_sequence.load(Ordering::Acquire),
            failure_count: self.failure_count.load(Ordering::Acquire),
            failure_code: self
                .failure_code
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            signed: true,
            offsite_configured: self.offsite.is_some(),
        }
    }

    fn record_failure(&self, code: &str) {
        if self
            .healthy
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            *self
                .failure_code
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(code.to_owned());
        }
        self.failure_count.fetch_add(1, Ordering::Relaxed);
    }
}

fn publish_bundle_sync(
    snapshot_path: &Path,
    wal_dir: &Path,
    destination: &Path,
    build_id: &str,
    leadership_generation: Option<i64>,
    signer: &RecoverySigner,
    offsite: Option<&OffsiteTarget>,
    cancelled: &AtomicBool,
) -> Result<RecoveryManifest> {
    let snapshot = read_and_validate_snapshot(snapshot_path)?;
    let recovery = Wal::inspect(wal_dir).context("inspect source WAL recovery set")?;
    validate_recovery_boundary(snapshot_path, &snapshot, &recovery)?;

    let temporary = destination.join(format!(".bundle-{}.tmp", Uuid::new_v4()));
    fs::create_dir(&temporary)
        .with_context(|| format!("create temporary recovery bundle {}", temporary.display()))?;
    set_private_directory_permissions(&temporary)?;

    let result: Result<RecoveryManifest> = (|| {
        let mut artifacts = Vec::new();
        artifacts.push(copy_artifact(
            snapshot_path,
            &temporary,
            "engine_snapshot.json",
            cancelled,
        )?);
        for wal_path in Wal::recovery_segment_paths(wal_dir)? {
            let file_name = wal_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow::anyhow!("WAL file name is not valid UTF-8"))?;
            artifacts.push(copy_artifact(&wal_path, &temporary, file_name, cancelled)?);
        }
        artifacts.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        if cancelled.load(Ordering::Acquire) {
            bail!("engine recovery replication was cancelled");
        }

        let snapshot_hash = artifacts
            .iter()
            .find(|artifact| artifact.file_name == "engine_snapshot.json")
            .map(|artifact| artifact.sha256.as_str())
            .ok_or_else(|| anyhow::anyhow!("snapshot artifact is missing"))?;
        let bundle_id = format!(
            "checkpoint-{:020}-{}",
            recovery.checkpoint_sequence,
            &snapshot_hash[..16]
        );
        let manifest = RecoveryManifest {
            version: RECOVERY_MANIFEST_VERSION,
            bundle_id: bundle_id.clone(),
            created_at_ms: now_ms(),
            build_id: build_id.to_owned(),
            leadership_generation,
            original_snapshot_path: snapshot_path.display().to_string(),
            snapshot_version: snapshot.version,
            snapshot_wal_sequence: snapshot.wal_sequence,
            checkpoint_sequence: recovery.checkpoint_sequence,
            last_sequence: recovery.last_sequence,
            clean_shutdown: recovery.clean_shutdown && snapshot.clean_shutdown,
            ledger_recovery: snapshot.ledger_recovery.clone(),
            artifacts,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        write_synced_file(&temporary.join(MANIFEST_FILE), &manifest_bytes)?;
        let signature = signer.sign_manifest(&manifest_bytes)?;
        write_synced_file(
            &temporary.join(SIGNATURE_FILE),
            &serde_json::to_vec_pretty(&signature)?,
        )?;
        sync_directory(&temporary)?;
        if cancelled.load(Ordering::Acquire) {
            bail!("engine recovery replication was cancelled");
        }

        let final_path = destination.join(&bundle_id);
        let published_manifest = if final_path.exists() {
            let existing = verify_bundle_with_key(&final_path, Some(&signer.verifying_key))?;
            if !same_bundle_contents(&existing, &manifest) {
                bail!("recovery bundle identity collision");
            }
            fs::remove_dir_all(&temporary).with_context(|| {
                format!("remove duplicate temporary bundle {}", temporary.display())
            })?;
            existing
        } else {
            fs::rename(&temporary, &final_path).with_context(|| {
                format!(
                    "publish recovery bundle {} to {}",
                    temporary.display(),
                    final_path.display()
                )
            })?;
            sync_directory(destination)?;
            manifest
        };

        let published_manifest_bytes = fs::read(final_path.join(MANIFEST_FILE))?;
        let latest = LatestRecoveryBundle {
            version: RECOVERY_MANIFEST_VERSION,
            bundle_id,
            manifest_sha256: sha256_bytes(&published_manifest_bytes),
        };
        publish_latest(destination, &latest)?;
        if let Some(offsite) = offsite {
            export_bundle_to_offsite(
                &final_path,
                &offsite.destination,
                &offsite.recipient,
                Some(&signer.verifying_key),
                cancelled,
            )?;
        }
        Ok(published_manifest)
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn same_bundle_contents(left: &RecoveryManifest, right: &RecoveryManifest) -> bool {
    left.version == right.version
        && left.bundle_id == right.bundle_id
        && left.build_id == right.build_id
        && left.leadership_generation == right.leadership_generation
        && left.original_snapshot_path == right.original_snapshot_path
        && left.snapshot_version == right.snapshot_version
        && left.snapshot_wal_sequence == right.snapshot_wal_sequence
        && left.checkpoint_sequence == right.checkpoint_sequence
        && left.last_sequence == right.last_sequence
        && left.clean_shutdown == right.clean_shutdown
        && left.ledger_recovery == right.ledger_recovery
        && left.artifacts == right.artifacts
}

fn verify_manifest_signature(
    bundle_dir: &Path,
    manifest_bytes: &[u8],
    trusted_verifying_key: Option<&VerifyingKey>,
) -> Result<RecoveryManifestSignature> {
    let signature_path = bundle_dir.join(SIGNATURE_FILE);
    require_regular_file(&signature_path)?;
    let metadata = fs::metadata(&signature_path)?;
    if metadata.len() > 64 * 1024 {
        bail!("recovery signature exceeds 64 KiB");
    }
    let signature: RecoveryManifestSignature = serde_json::from_reader(BufReader::new(
        File::open(&signature_path)
            .with_context(|| format!("open recovery signature {}", signature_path.display()))?,
    ))
    .with_context(|| format!("parse recovery signature {}", signature_path.display()))?;
    if signature.version != SIGNATURE_VERSION || signature.algorithm != SIGNATURE_ALGORITHM {
        bail!("unsupported recovery signature format");
    }
    let verifying_key = parse_verifying_key(&signature.public_key)?;
    let public_point = verifying_key.to_encoded_point(true);
    if signature.key_id != key_id(public_point.as_bytes()) {
        bail!("recovery signature key identity does not match its public key");
    }
    if let Some(trusted) = trusted_verifying_key {
        if trusted.to_encoded_point(true) != public_point {
            bail!("recovery signature was not produced by the trusted verification key");
        }
    }
    let digest = Sha256::digest(manifest_bytes);
    if signature.manifest_sha256 != hex::encode(digest) {
        bail!("recovery signature manifest hash does not match");
    }
    let signature_bytes = decode_hex(&signature.signature, "recovery signature")?;
    let ecdsa_signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| anyhow::anyhow!("recovery signature is invalid"))?;
    verifying_key
        .verify_prehash(&digest, &ecdsa_signature)
        .map_err(|_| anyhow::anyhow!("recovery manifest signature verification failed"))?;
    Ok(signature)
}

fn parse_verifying_key(value: &str) -> Result<VerifyingKey> {
    let bytes = decode_hex(value, "recovery verifying key")?;
    VerifyingKey::from_sec1_bytes(&bytes)
        .map_err(|_| anyhow::anyhow!("recovery verifying key is invalid"))
}

fn decode_hex(value: &str, label: &str) -> Result<Vec<u8>> {
    hex::decode(value.strip_prefix("0x").unwrap_or(value))
        .map_err(|_| anyhow::anyhow!("{label} is not valid hexadecimal"))
}

fn key_id(public_key: &[u8]) -> String {
    hex::encode(&Sha256::digest(public_key)[..16])
}

#[cfg(test)]
fn verify_bundle(bundle_dir: &Path) -> Result<RecoveryManifest> {
    verify_bundle_with_key(bundle_dir, None)
}

pub fn verify_bundle_with_trusted_key(
    bundle_dir: &Path,
    trusted_verifying_key_hex: &str,
) -> Result<RecoveryManifest> {
    let trusted_key = parse_verifying_key(trusted_verifying_key_hex)?;
    verify_bundle_with_key(bundle_dir, Some(&trusted_key))
}

fn verify_bundle_with_key(
    bundle_dir: &Path,
    trusted_verifying_key: Option<&VerifyingKey>,
) -> Result<RecoveryManifest> {
    verify_bundle_with_key_options(bundle_dir, trusted_verifying_key, true)
}

fn verify_bundle_with_key_options(
    bundle_dir: &Path,
    trusted_verifying_key: Option<&VerifyingKey>,
    require_directory_identity: bool,
) -> Result<RecoveryManifest> {
    let manifest_path = bundle_dir.join(MANIFEST_FILE);
    require_regular_file(&manifest_path)?;
    let metadata = fs::metadata(&manifest_path)?;
    if metadata.len() > 1024 * 1024 {
        bail!("recovery manifest exceeds 1 MiB");
    }
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read recovery manifest {}", manifest_path.display()))?;
    verify_manifest_signature(bundle_dir, &manifest_bytes, trusted_verifying_key)?;
    let manifest: RecoveryManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse recovery manifest {}", manifest_path.display()))?;
    if manifest.version != RECOVERY_MANIFEST_VERSION {
        bail!("unsupported recovery manifest version {}", manifest.version);
    }
    if manifest.build_id.is_empty() || manifest.artifacts.len() < 2 {
        bail!("recovery manifest is incomplete");
    }
    let directory_name = bundle_dir.file_name().and_then(|name| name.to_str());
    if require_directory_identity && directory_name != Some(manifest.bundle_id.as_str()) {
        bail!("recovery bundle directory does not match its manifest identity");
    }

    let mut snapshot_count = 0usize;
    let mut wal_count = 0usize;
    let mut seen = std::collections::HashSet::new();
    for artifact in &manifest.artifacts {
        validate_artifact_name(&artifact.file_name)?;
        if !seen.insert(&artifact.file_name) {
            bail!("duplicate recovery artifact {}", artifact.file_name);
        }
        if artifact.file_name == "engine_snapshot.json" {
            snapshot_count += 1;
        } else if is_wal_file_name(&artifact.file_name) {
            wal_count += 1;
        } else {
            bail!("unsupported recovery artifact {}", artifact.file_name);
        }
        let path = bundle_dir.join(&artifact.file_name);
        require_regular_file(&path)?;
        let (size_bytes, sha256) = hash_file(&path)?;
        if size_bytes != artifact.size_bytes || sha256 != artifact.sha256 {
            bail!(
                "recovery artifact integrity mismatch for {}",
                artifact.file_name
            );
        }
    }
    if snapshot_count != 1 || wal_count == 0 {
        bail!("recovery bundle must contain one snapshot and at least one WAL segment");
    }
    let expected_files: std::collections::HashSet<&str> = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.file_name.as_str())
        .chain([MANIFEST_FILE, SIGNATURE_FILE])
        .collect();
    for entry in fs::read_dir(bundle_dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("recovery bundle contains a non-UTF-8 file name"))?;
        if !expected_files.contains(name.as_str()) {
            bail!("recovery bundle contains unexpected artifact {name}");
        }
    }

    let snapshot = read_and_validate_snapshot(&bundle_dir.join("engine_snapshot.json"))?;
    let recovery = Wal::inspect(bundle_dir).context("inspect bundled WAL recovery set")?;
    validate_manifest_boundary(&manifest, &snapshot, &recovery)?;
    Ok(manifest)
}

#[cfg(test)]
fn verify_latest(replica_dir: &Path) -> Result<(PathBuf, RecoveryManifest)> {
    verify_latest_with_key(replica_dir, None)
}

pub fn verify_latest_with_trusted_key(
    replica_dir: &Path,
    trusted_verifying_key_hex: &str,
) -> Result<(PathBuf, RecoveryManifest)> {
    let trusted_key = parse_verifying_key(trusted_verifying_key_hex)?;
    verify_latest_with_key(replica_dir, Some(&trusted_key))
}

fn verify_latest_with_key(
    replica_dir: &Path,
    trusted_verifying_key: Option<&VerifyingKey>,
) -> Result<(PathBuf, RecoveryManifest)> {
    let latest_path = replica_dir.join(LATEST_FILE);
    require_regular_file(&latest_path)?;
    let bytes = fs::read(&latest_path)?;
    if bytes.len() > 64 * 1024 {
        bail!("latest recovery pointer exceeds 64 KiB");
    }
    let latest: LatestRecoveryBundle = serde_json::from_slice(&bytes)?;
    if latest.version != RECOVERY_MANIFEST_VERSION {
        bail!(
            "unsupported latest recovery pointer version {}",
            latest.version
        );
    }
    validate_artifact_name(&latest.bundle_id)?;
    let bundle_dir = replica_dir.join(&latest.bundle_id);
    let manifest_path = bundle_dir.join(MANIFEST_FILE);
    require_regular_file(&manifest_path)?;
    let manifest_bytes = fs::read(&manifest_path)?;
    if sha256_bytes(&manifest_bytes) != latest.manifest_sha256 {
        bail!("latest recovery pointer manifest hash does not match");
    }
    let manifest = verify_bundle_with_key(&bundle_dir, trusted_verifying_key)?;
    if manifest.bundle_id != latest.bundle_id {
        bail!("latest recovery pointer bundle identity does not match");
    }
    Ok((bundle_dir, manifest))
}

#[cfg(test)]
fn restore_bundle(bundle_dir: &Path, target_dir: &Path) -> Result<RecoveryManifest> {
    restore_bundle_with_key(bundle_dir, target_dir, None)
}

pub fn restore_bundle_with_trusted_key(
    bundle_dir: &Path,
    target_dir: &Path,
    trusted_verifying_key_hex: &str,
) -> Result<RecoveryManifest> {
    let trusted_key = parse_verifying_key(trusted_verifying_key_hex)?;
    restore_bundle_with_key(bundle_dir, target_dir, Some(&trusted_key))
}

fn restore_bundle_with_key(
    bundle_dir: &Path,
    target_dir: &Path,
    trusted_verifying_key: Option<&VerifyingKey>,
) -> Result<RecoveryManifest> {
    let manifest = verify_bundle_with_key(bundle_dir, trusted_verifying_key)?;
    if !manifest.clean_shutdown {
        bail!("refusing to restore an unclean recovery bundle");
    }
    let expected_snapshot_path = target_dir.join("engine_snapshot.json");
    if expected_snapshot_path.display().to_string() != manifest.original_snapshot_path {
        bail!(
            "restore target must preserve snapshot path {}; requested {}",
            manifest.original_snapshot_path,
            expected_snapshot_path.display()
        );
    }
    if target_dir.exists() {
        let mut entries = fs::read_dir(target_dir)
            .with_context(|| format!("inspect restore target {}", target_dir.display()))?;
        if entries.next().transpose()?.is_some() {
            bail!("restore target must be absent or empty");
        }
    }
    let parent = target_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("restore target has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".vela-restore-{}.tmp", Uuid::new_v4()));
    fs::create_dir(&staging)?;
    set_private_directory_permissions(&staging)?;
    let result: Result<()> = (|| {
        for artifact in &manifest.artifacts {
            let copied = copy_artifact(
                &bundle_dir.join(&artifact.file_name),
                &staging,
                &artifact.file_name,
                &AtomicBool::new(false),
            )?;
            if copied != *artifact {
                bail!(
                    "recovery artifact changed while restoring: {}",
                    artifact.file_name
                );
            }
        }
        sync_directory(&staging)?;
        let staged_snapshot = read_and_validate_snapshot(&staging.join("engine_snapshot.json"))?;
        let staged_recovery = Wal::inspect(&staging)?;
        validate_recovery_boundary(&expected_snapshot_path, &staged_snapshot, &staged_recovery)?;
        if target_dir.exists() {
            fs::remove_dir(target_dir)
                .with_context(|| format!("remove empty restore target {}", target_dir.display()))?;
        }
        fs::rename(&staging, target_dir).with_context(|| {
            format!("publish restored recovery set to {}", target_dir.display())
        })?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(manifest)
}

pub fn export_encrypted_bundle(
    bundle_dir: &Path,
    offsite_dir: &Path,
    recipient: &str,
    trusted_verifying_key_hex: &str,
) -> Result<PathBuf> {
    let recipient = age::x25519::Recipient::from_str(recipient)
        .map_err(|_| anyhow::anyhow!("age recipient is invalid"))?;
    let trusted_key = parse_verifying_key(trusted_verifying_key_hex)?;
    fs::create_dir_all(offsite_dir)?;
    set_private_directory_permissions(offsite_dir)?;
    export_bundle_to_offsite(
        bundle_dir,
        offsite_dir,
        &recipient,
        Some(&trusted_key),
        &AtomicBool::new(false),
    )
}

fn export_bundle_to_offsite(
    bundle_dir: &Path,
    offsite_dir: &Path,
    recipient: &age::x25519::Recipient,
    trusted_verifying_key: Option<&VerifyingKey>,
    cancelled: &AtomicBool,
) -> Result<PathBuf> {
    let manifest = verify_bundle_with_key(bundle_dir, trusted_verifying_key)?;
    let final_path = offsite_dir.join(format!("{}.age", manifest.bundle_id));
    if final_path.exists() {
        bail!("refusing to replace an existing offsite recovery archive");
    }
    fs::create_dir_all(offsite_dir)?;
    set_private_directory_permissions(offsite_dir)?;
    let temporary = offsite_dir.join(format!(
        ".{}.age-{}.tmp",
        manifest.bundle_id,
        Uuid::new_v4()
    ));
    let result: Result<PathBuf> = (|| {
        let output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        set_private_file_permissions(&temporary)?;
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))?;
        let mut encrypted = encryptor.wrap_output(output)?;
        encrypted.write_all(OFFSITE_ARCHIVE_MAGIC)?;
        let mut names = Vec::with_capacity(manifest.artifacts.len() + 2);
        names.push(MANIFEST_FILE.to_owned());
        names.push(SIGNATURE_FILE.to_owned());
        names.extend(
            manifest
                .artifacts
                .iter()
                .map(|artifact| artifact.file_name.clone()),
        );
        names.sort();
        let count = u32::try_from(names.len())
            .map_err(|_| anyhow::anyhow!("too many recovery archive entries"))?;
        if count > MAX_ARCHIVE_ENTRIES {
            bail!("too many recovery archive entries");
        }
        encrypted.write_all(&count.to_be_bytes())?;
        for name in names {
            if cancelled.load(Ordering::Acquire) {
                bail!("encrypted offsite export was cancelled");
            }
            validate_artifact_name(&name)?;
            let source = bundle_dir.join(&name);
            require_regular_file(&source)?;
            let size = fs::metadata(&source)?.len();
            if size > MAX_ARCHIVE_FILE_BYTES {
                bail!("recovery archive entry exceeds the size limit");
            }
            let name_bytes = name.as_bytes();
            let name_len = u16::try_from(name_bytes.len())
                .map_err(|_| anyhow::anyhow!("recovery archive file name is too long"))?;
            encrypted.write_all(&name_len.to_be_bytes())?;
            encrypted.write_all(name_bytes)?;
            encrypted.write_all(&size.to_be_bytes())?;
            copy_exact_file(&source, &mut encrypted, size, cancelled)?;
        }
        let output = encrypted.finish()?;
        output.sync_all()?;
        if cancelled.load(Ordering::Acquire) {
            bail!("encrypted offsite export was cancelled");
        }
        if final_path.exists() {
            bail!("refusing to replace an existing offsite recovery archive");
        }
        fs::rename(&temporary, &final_path)
            .context("publish encrypted offsite recovery archive")?;
        sync_directory(offsite_dir)?;
        Ok(final_path.clone())
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn import_encrypted_bundle(
    archive_path: &Path,
    replica_dir: &Path,
    identity: &str,
    trusted_verifying_key_hex: &str,
) -> Result<(PathBuf, RecoveryManifest)> {
    require_regular_file(archive_path)?;
    let identity = age::x25519::Identity::from_str(identity.trim())
        .map_err(|_| anyhow::anyhow!("age identity is invalid"))?;
    let trusted_key = parse_verifying_key(trusted_verifying_key_hex)?;
    fs::create_dir_all(replica_dir)?;
    set_private_directory_permissions(replica_dir)?;
    let staging = replica_dir.join(format!(".offsite-import-{}.tmp", Uuid::new_v4()));
    fs::create_dir(&staging)?;
    set_private_directory_permissions(&staging)?;
    let result: Result<(PathBuf, RecoveryManifest)> = (|| {
        let decryptor = age::Decryptor::new(BufReader::new(File::open(archive_path)?))?;
        let mut decrypted = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
        let mut magic = vec![0u8; OFFSITE_ARCHIVE_MAGIC.len()];
        decrypted.read_exact(&mut magic)?;
        if magic != OFFSITE_ARCHIVE_MAGIC {
            bail!("encrypted recovery archive has an unsupported format");
        }
        let count = read_u32(&mut decrypted)?;
        if !(3..=MAX_ARCHIVE_ENTRIES).contains(&count) {
            bail!("encrypted recovery archive has an invalid entry count");
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            let name_len = usize::from(read_u16(&mut decrypted)?);
            if name_len == 0 || name_len > 255 {
                bail!("encrypted recovery archive has an invalid file name length");
            }
            let mut name_bytes = vec![0u8; name_len];
            decrypted.read_exact(&mut name_bytes)?;
            let name = String::from_utf8(name_bytes).map_err(|_| {
                anyhow::anyhow!("encrypted recovery archive file name is not UTF-8")
            })?;
            validate_artifact_name(&name)?;
            if !seen.insert(name.clone()) {
                bail!("encrypted recovery archive contains a duplicate file");
            }
            let size = read_u64(&mut decrypted)?;
            if size > MAX_ARCHIVE_FILE_BYTES {
                bail!("encrypted recovery archive entry exceeds the size limit");
            }
            let output_path = staging.join(name);
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&output_path)?;
            set_private_file_permissions(&output_path)?;
            copy_exact_reader(&mut decrypted, &mut output, size)?;
            output.sync_all()?;
        }
        let mut trailing = [0u8; 1];
        if decrypted.read(&mut trailing)? != 0 {
            bail!("encrypted recovery archive contains trailing data");
        }
        sync_directory(&staging)?;
        let manifest = verify_bundle_with_key_options(&staging, Some(&trusted_key), false)?;
        let archive_bundle_id = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".age"))
            .ok_or_else(|| anyhow::anyhow!("offsite archive must end in .age"))?;
        if archive_bundle_id != manifest.bundle_id {
            bail!("offsite archive name does not match the signed bundle identity");
        }
        let final_path = replica_dir.join(&manifest.bundle_id);
        if final_path.exists() {
            let existing = verify_bundle_with_key(&final_path, Some(&trusted_key))?;
            if !same_bundle_contents(&existing, &manifest) {
                bail!("imported recovery bundle conflicts with an existing bundle");
            }
            fs::remove_dir_all(&staging)?;
        } else {
            fs::rename(&staging, &final_path)?;
            sync_directory(replica_dir)?;
        }
        Ok((final_path, manifest))
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[derive(Debug, Clone)]
pub struct GeneratedRecoveryKeys {
    pub key_id: String,
    pub age_recipient: String,
}

pub fn generate_recovery_key_files(target_dir: &Path) -> Result<GeneratedRecoveryKeys> {
    if target_dir.exists() {
        bail!("key output directory already exists");
    }
    fs::create_dir(target_dir)?;
    let result: Result<GeneratedRecoveryKeys> = (|| {
        set_private_directory_permissions(target_dir)?;
        let signer = RecoverySigner::generate();
        let age_identity = age::x25519::Identity::generate();
        let age_recipient = age_identity.to_public().to_string();
        let mut signing_key_hex = hex::encode(signer.signing_key.to_bytes());
        let signing_write = write_synced_file(
            &target_dir.join("recovery-signing-key.hex"),
            signing_key_hex.as_bytes(),
        );
        signing_key_hex.zeroize();
        signing_write?;
        write_synced_file(
            &target_dir.join("recovery-verifying-key.hex"),
            signer.public_key_hex.as_bytes(),
        )?;
        write_synced_file(
            &target_dir.join("recovery-age-identity.txt"),
            age_identity.to_string().expose_secret().as_bytes(),
        )?;
        write_synced_file(
            &target_dir.join("recovery-age-recipient.txt"),
            age_recipient.as_bytes(),
        )?;
        sync_directory(target_dir)?;
        if let Some(parent) = target_dir
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            sync_directory(parent)?;
        }
        Ok(GeneratedRecoveryKeys {
            key_id: signer.key_id,
            age_recipient,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(target_dir);
    }
    result
}

fn copy_exact_file(
    source: &Path,
    output: &mut impl Write,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut input = BufReader::new(File::open(source)?);
    let mut remaining = expected_size;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        if cancelled.load(Ordering::Acquire) {
            bail!("encrypted offsite export was cancelled");
        }
        let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = input.read(&mut buffer[..limit])?;
        if read == 0 {
            bail!("recovery artifact was truncated during offsite export");
        }
        output.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    if input.read(&mut [0u8; 1])? != 0 {
        bail!("recovery artifact grew during offsite export");
    }
    Ok(())
}

fn copy_exact_reader(
    input: &mut impl Read,
    output: &mut impl Write,
    expected_size: u64,
) -> Result<()> {
    let mut remaining = expected_size;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = input.read(&mut buffer[..limit])?;
        if read == 0 {
            bail!("encrypted recovery archive is truncated");
        }
        output.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    Ok(())
}

fn read_u16(reader: &mut impl Read) -> Result<u16> {
    let mut bytes = [0u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_be_bytes(bytes))
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn validate_recovery_boundary(
    snapshot_path: &Path,
    snapshot: &EngineSnapshot,
    recovery: &crate::wal::WalRecoveryState,
) -> Result<()> {
    let expected_checkpoint = snapshot
        .wal_sequence
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("snapshot WAL sequence is exhausted"))?;
    if recovery.checkpoint_sequence != expected_checkpoint {
        bail!("snapshot and WAL checkpoint sequences do not form one recovery boundary");
    }
    if recovery.checkpoint_snapshot_path.as_deref() != Some(&snapshot_path.display().to_string()) {
        bail!("WAL checkpoint references a different snapshot path");
    }
    if snapshot.clean_shutdown {
        let expected_stop = expected_checkpoint
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("snapshot WAL sequence is exhausted"))?;
        if !recovery.clean_shutdown || recovery.last_sequence != expected_stop {
            bail!("clean snapshot is missing its exact terminal WAL stop boundary");
        }
    } else if recovery.clean_shutdown || recovery.last_sequence != expected_checkpoint {
        bail!("periodic snapshot is not at its exact WAL checkpoint boundary");
    }
    Ok(())
}

fn validate_manifest_boundary(
    manifest: &RecoveryManifest,
    snapshot: &EngineSnapshot,
    recovery: &crate::wal::WalRecoveryState,
) -> Result<()> {
    if manifest.snapshot_version != snapshot.version
        || manifest.snapshot_wal_sequence != snapshot.wal_sequence
        || manifest.checkpoint_sequence != recovery.checkpoint_sequence
        || manifest.last_sequence != recovery.last_sequence
        || manifest.clean_shutdown != (snapshot.clean_shutdown && recovery.clean_shutdown)
        || manifest.ledger_recovery != snapshot.ledger_recovery
    {
        bail!("recovery manifest does not match its snapshot/WAL boundary");
    }
    if recovery.checkpoint_snapshot_path.as_deref()
        != Some(manifest.original_snapshot_path.as_str())
    {
        bail!("recovery manifest snapshot path does not match its WAL checkpoint");
    }
    let expected_checkpoint = snapshot
        .wal_sequence
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("snapshot WAL sequence is exhausted"))?;
    if recovery.checkpoint_sequence != expected_checkpoint {
        bail!("recovery manifest checkpoint is not adjacent to its snapshot");
    }
    if manifest.clean_shutdown {
        if recovery.last_sequence != expected_checkpoint.saturating_add(1) {
            bail!("clean recovery bundle has no exact terminal stop record");
        }
    } else if recovery.last_sequence != expected_checkpoint {
        bail!("periodic recovery bundle extends beyond its checkpoint");
    }
    Ok(())
}

fn copy_artifact(
    source: &Path,
    destination_dir: &Path,
    file_name: &str,
    cancelled: &AtomicBool,
) -> Result<RecoveryArtifact> {
    validate_artifact_name(file_name)?;
    require_regular_file(source)?;
    let destination = destination_dir.join(file_name);
    let mut input = BufReader::new(
        File::open(source)
            .with_context(|| format!("open recovery artifact {}", source.display()))?,
    );
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)
        .with_context(|| format!("create recovery artifact {}", destination.display()))?;
    set_private_file_permissions(&destination)?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0u64;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        if cancelled.load(Ordering::Acquire) {
            bail!("engine recovery replication was cancelled");
        }
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("recovery artifact size overflow"))?;
    }
    output.sync_all()?;
    Ok(RecoveryArtifact {
        file_name: file_name.to_owned(),
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn read_and_validate_snapshot(path: &Path) -> Result<EngineSnapshot> {
    require_regular_file(path)?;
    let snapshot: EngineSnapshot = serde_json::from_reader(BufReader::new(File::open(path)?))
        .with_context(|| format!("parse engine snapshot {}", path.display()))?;
    snapshot::validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut size_bytes = 0u64;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("recovery artifact size overflow"))?;
    }
    Ok((size_bytes, hex::encode(hasher.finalize())))
}

fn publish_latest(destination: &Path, latest: &LatestRecoveryBundle) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(latest)?;
    let temporary = destination.join(format!(".{LATEST_FILE}-{}.tmp", Uuid::new_v4()));
    write_synced_file(&temporary, &bytes)?;
    fs::rename(&temporary, destination.join(LATEST_FILE))?;
    sync_directory(destination)
}

fn write_synced_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    set_private_file_permissions(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn require_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect recovery artifact {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "recovery artifact is not a regular file: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_artifact_name(file_name: &str) -> Result<()> {
    let mut components = Path::new(file_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        bail!("recovery artifact name is not a plain file name");
    }
    Ok(())
}

fn is_wal_file_name(file_name: &str) -> bool {
    file_name
        .strip_prefix("engine_wal_")
        .and_then(|rest| rest.strip_suffix(".log"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn validate_distinct_directories(snapshot_dir: &Path, replica_dir: &Path) -> Result<(), String> {
    let source = normalized_absolute(snapshot_dir)?;
    let destination = normalized_absolute(replica_dir)?;
    if source == destination || source.starts_with(&destination) || destination.starts_with(&source)
    {
        return Err(
            "VELA_ENGINE_REPLICA_DIR must be a separate, non-nested filesystem path".to_owned(),
        );
    }
    Ok(())
}

fn normalized_absolute(path: &Path) -> Result<PathBuf, String> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("resolve current directory: {error}"))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::{
        WalCheckpoint, WalEngineStart, WalEngineStop, CHECKPOINT, ENGINE_START, ENGINE_STOP,
    };

    #[test]
    fn replica_path_must_not_overlap_source() {
        assert!(validate_distinct_directories(Path::new("/data"), Path::new("/backup")).is_ok());
        assert!(validate_distinct_directories(Path::new("/data"), Path::new("/data")).is_err());
        assert!(
            validate_distinct_directories(Path::new("/data"), Path::new("/data/replica")).is_err()
        );
    }

    #[test]
    fn artifact_names_cannot_escape_bundle() {
        assert!(validate_artifact_name("engine_snapshot.json").is_ok());
        assert!(validate_artifact_name("../engine_snapshot.json").is_err());
        assert!(validate_artifact_name("nested/file").is_err());
    }

    #[tokio::test]
    async fn periodic_bundle_is_immutable_and_detects_artifact_corruption() {
        let fixture = fixture(false).await;
        let manifest = fixture
            .replica
            .publish(&fixture.snapshot_path, &fixture.source, Some(7))
            .await
            .unwrap();
        assert!(!manifest.clean_shutdown);
        assert_eq!(manifest.leadership_generation, Some(7));
        let bundle = fixture.replica_dir.join(&manifest.bundle_id);
        assert_eq!(verify_bundle(&bundle).unwrap(), manifest);
        assert_eq!(
            verify_bundle_with_trusted_key(&bundle, &fixture.trusted_key_hex).unwrap(),
            manifest
        );
        let untrusted = RecoverySigner::generate();
        assert!(
            verify_bundle_with_trusted_key(&bundle, &untrusted.public_key_hex)
                .unwrap_err()
                .to_string()
                .contains("trusted verification key")
        );
        assert_eq!(verify_latest(&fixture.replica_dir).unwrap().1, manifest);
        assert!(restore_bundle(&bundle, &fixture.base.join("unused"))
            .unwrap_err()
            .to_string()
            .contains("unclean"));

        let mut snapshot = OpenOptions::new()
            .append(true)
            .open(bundle.join("engine_snapshot.json"))
            .unwrap();
        snapshot.write_all(b"corruption").unwrap();
        snapshot.sync_all().unwrap();
        assert!(verify_bundle(&bundle)
            .unwrap_err()
            .to_string()
            .contains("integrity mismatch"));
        fs::remove_dir_all(fixture.base).unwrap();
    }

    #[tokio::test]
    async fn signed_manifest_rejects_post_signature_changes() {
        let fixture = fixture(false).await;
        let manifest = fixture
            .replica
            .publish(&fixture.snapshot_path, &fixture.source, Some(8))
            .await
            .unwrap();
        let bundle = fixture.replica_dir.join(&manifest.bundle_id);
        let manifest_path = bundle.join(MANIFEST_FILE);
        let mut manifest_file = OpenOptions::new()
            .append(true)
            .open(&manifest_path)
            .unwrap();
        manifest_file.write_all(b"\n").unwrap();
        manifest_file.sync_all().unwrap();
        assert!(
            verify_bundle_with_trusted_key(&bundle, &fixture.trusted_key_hex)
                .unwrap_err()
                .to_string()
                .contains("manifest hash does not match")
        );
        fs::remove_dir_all(fixture.base).unwrap();
    }

    #[tokio::test]
    async fn clean_bundle_restores_a_lost_engine_directory_atomically() {
        let fixture = fixture(true).await;
        let manifest = fixture
            .replica
            .publish(&fixture.snapshot_path, &fixture.source, Some(9))
            .await
            .unwrap();
        assert!(manifest.clean_shutdown);
        let bundle = fixture.replica_dir.join(&manifest.bundle_id);

        let lost = fixture.base.join("lost-source-evidence");
        fs::rename(&fixture.source, &lost).unwrap();
        let restored =
            restore_bundle_with_trusted_key(&bundle, &fixture.source, &fixture.trusted_key_hex)
                .unwrap();
        assert_eq!(restored, manifest);
        let recovery = Wal::inspect(&fixture.source).unwrap();
        assert!(recovery.clean_shutdown);
        assert_eq!(recovery.last_sequence, manifest.last_sequence);
        fs::remove_dir_all(fixture.base).unwrap();
    }

    #[tokio::test]
    async fn encrypted_offsite_archive_round_trips_and_requires_trusted_signature() {
        let fixture = fixture(true).await;
        let manifest = fixture
            .replica
            .publish(&fixture.snapshot_path, &fixture.source, Some(11))
            .await
            .unwrap();
        let bundle = fixture.replica_dir.join(&manifest.bundle_id);
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let offsite = fixture.base.join("offsite");
        let archive =
            export_encrypted_bundle(&bundle, &offsite, &recipient, &fixture.trusted_key_hex)
                .unwrap();
        assert!(archive
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".age"));

        let imported_dir = fixture.base.join("imported");
        let (imported_bundle, imported_manifest) = import_encrypted_bundle(
            &archive,
            &imported_dir,
            identity.to_string().expose_secret(),
            &fixture.trusted_key_hex,
        )
        .unwrap();
        assert_eq!(imported_manifest, manifest);
        assert_eq!(
            verify_bundle_with_trusted_key(&imported_bundle, &fixture.trusted_key_hex).unwrap(),
            manifest
        );

        let mut ciphertext = fs::read(&archive).unwrap();
        let last = ciphertext.last_mut().unwrap();
        *last ^= 0x01;
        fs::write(&archive, ciphertext).unwrap();
        assert!(import_encrypted_bundle(
            &archive,
            &fixture.base.join("corrupt-import"),
            identity.to_string().expose_secret(),
            &fixture.trusted_key_hex,
        )
        .is_err());
        fs::remove_dir_all(fixture.base).unwrap();
    }

    #[test]
    fn key_generation_refuses_to_overwrite_existing_material() {
        let base = std::env::temp_dir().join(format!("vela-recovery-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let key_dir = base.join("keys");
        let generated = generate_recovery_key_files(&key_dir).unwrap();
        assert_eq!(generated.key_id.len(), 32);
        for name in [
            "recovery-signing-key.hex",
            "recovery-verifying-key.hex",
            "recovery-age-identity.txt",
            "recovery-age-recipient.txt",
        ] {
            assert!(key_dir.join(name).is_file());
        }
        assert!(generate_recovery_key_files(&key_dir).is_err());
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn failed_publication_latches_replica_health() {
        let base = std::env::temp_dir().join(format!("vela-recovery-{}", Uuid::new_v4()));
        let destination = base.join("replica");
        fs::create_dir_all(&destination).unwrap();
        let replica = RecoveryReplica::new(
            destination,
            "test-build".to_owned(),
            Duration::from_secs(5),
            RecoverySigner::generate(),
            None,
        );
        assert!(replica
            .publish(&base.join("missing.json"), &base.join("missing-wal"), None)
            .await
            .is_err());
        let status = replica.snapshot();
        assert!(!status.healthy);
        assert_eq!(status.failure_count, 1);
        assert_eq!(status.failure_code.as_deref(), Some("replication_failed"));
        assert!(replica
            .publish(&base.join("missing.json"), &base.join("missing-wal"), None)
            .await
            .unwrap_err()
            .to_string()
            .contains("fenced"));
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn offsite_export_failure_latches_replica_health() {
        let fixture = fixture(false).await;
        let blocked_destination = fixture.base.join("offsite-is-a-file");
        fs::write(&blocked_destination, b"not a directory").unwrap();
        let second_replica = fixture.base.join("second-replica");
        fs::create_dir(&second_replica).unwrap();
        let identity = age::x25519::Identity::generate();
        let replica = RecoveryReplica::new(
            second_replica,
            "test-build".to_owned(),
            Duration::from_secs(5),
            RecoverySigner::generate(),
            Some(OffsiteTarget {
                destination: blocked_destination,
                recipient: identity.to_public(),
            }),
        );
        assert!(replica
            .publish(&fixture.snapshot_path, &fixture.source, None)
            .await
            .is_err());
        let status = replica.snapshot();
        assert!(!status.healthy);
        assert!(status.offsite_configured);
        assert_eq!(status.failure_code.as_deref(), Some("replication_failed"));
        fs::remove_dir_all(fixture.base).unwrap();
    }

    struct Fixture {
        base: PathBuf,
        source: PathBuf,
        snapshot_path: PathBuf,
        replica_dir: PathBuf,
        trusted_key_hex: String,
        replica: RecoveryReplica,
    }

    async fn fixture(clean: bool) -> Fixture {
        let base = std::env::temp_dir().join(format!("vela-recovery-{}", Uuid::new_v4()));
        let source = base.join("engine");
        let replica_dir = base.join("replica");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&replica_dir).unwrap();
        let wal = Wal::new(&source).unwrap();
        wal.append(
            ENGINE_START,
            &WalEngineStart {
                version: "test".to_owned(),
                reason: "clean".to_owned(),
                previous_sequence: 0,
            },
        )
        .await
        .unwrap();
        wal.flush().await.unwrap();
        let snapshot_path = source.join("engine_snapshot.json");
        let snapshot: EngineSnapshot = serde_json::from_value(serde_json::json!({
            "version": crate::snapshot::CURRENT_SNAPSHOT_VERSION,
            "timestamp": 1,
            "balances": {},
            "orders": {},
            "markets": [],
            "sequence": 1,
            "wal_sequence": wal.current_sequence(),
            "metadata": {},
            "fee_balances": {},
            "clean_shutdown": clean,
            "incidents": [],
            "decisions": [],
            "registered_mms": [],
            "proofs": {},
            "attestations": {}
        }))
        .unwrap();
        fs::write(&snapshot_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
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
        wal.rotate().await.unwrap();
        if clean {
            wal.append(
                ENGINE_STOP,
                &WalEngineStop {
                    reason: "clean".to_owned(),
                    final_sequence: wal.current_sequence(),
                },
            )
            .await
            .unwrap();
            wal.flush().await.unwrap();
        }
        drop(wal);
        let signer = RecoverySigner::generate();
        Fixture {
            base,
            source,
            snapshot_path,
            replica_dir: replica_dir.clone(),
            trusted_key_hex: signer.public_key_hex.clone(),
            replica: RecoveryReplica::new(
                replica_dir,
                "test-build".to_owned(),
                Duration::from_secs(5),
                signer,
                None,
            ),
        }
    }
}

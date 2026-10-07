use async_trait::async_trait;
use bigdecimal::{BigDecimal, Zero};
use chrono::Utc;
use ledger::{
    AccountLookup, AccountProvisionRequest, ActivateAssetHalt, AssetHaltReason, DepositChainEvent,
    DepositContradictionOutcome, DepositState, FinalizeDeposit, Ledger, LedgerError,
    ObserveDeposit, ReverseCreditedDeposit, RpcCanonicalContradiction, RpcFinalityObservation,
};
use reqwest::Url;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use std::{
    cmp::min,
    collections::HashMap,
    env,
    future::Future,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use thiserror::Error;
use tokio::time::sleep;
use tracing::{error, info, warn};
use uuid::Uuid;

pub const DEPOSIT_EVENT_SIGNATURE: &str = "Deposited(bytes32,address,bytes32,address,uint256)";
pub const DEPOSIT_EVENT_TOPIC: &str =
    "0x1670a29e5e0e2dd0f73d6c06d59212610f64f0d1e118644003f43d01ded42825";
pub const FINALITY_POLICY: &str = "rpc_finalized_two_provider";
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub deployment_id: Uuid,
    pub primary_name: String,
    pub primary_url: Url,
    pub secondary_name: String,
    pub secondary_url: Url,
    pub request_timeout: Duration,
    pub poll_interval: Duration,
    pub retry_max: Duration,
    pub max_head_age: Duration,
    pub max_provider_block_lag: u64,
    pub batch_size: u64,
    pub audit_batch_size: u64,
    pub lease_duration: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, IndexerError> {
        let primary_url = required_url("VELA_BASE_RPC_PRIMARY_URL")?;
        let secondary_url = required_url("VELA_BASE_RPC_SECONDARY_URL")?;
        if provider_origin(&primary_url) == provider_origin(&secondary_url) {
            return Err(IndexerError::Config(
                "primary and secondary RPC URLs must use different origins".to_owned(),
            ));
        }
        let primary_name = required("VELA_BASE_RPC_PRIMARY_NAME")?;
        let secondary_name = required("VELA_BASE_RPC_SECONDARY_NAME")?;
        if primary_name.eq_ignore_ascii_case(&secondary_name) {
            return Err(IndexerError::Config(
                "primary and secondary RPC provider names must differ".to_owned(),
            ));
        }
        let config = Self {
            database_url: required("DATABASE_URL")?,
            deployment_id: required("VELA_INDEXER_DEPLOYMENT_ID")?
                .parse()
                .map_err(|_| {
                    IndexerError::Config("VELA_INDEXER_DEPLOYMENT_ID must be a UUID".to_owned())
                })?,
            primary_name,
            primary_url,
            secondary_name,
            secondary_url,
            request_timeout: millis("VELA_INDEXER_RPC_TIMEOUT_MS", 5_000)?,
            poll_interval: millis("VELA_INDEXER_POLL_MS", 2_000)?,
            retry_max: millis("VELA_INDEXER_RETRY_MAX_MS", 30_000)?,
            max_head_age: seconds("VELA_INDEXER_MAX_HEAD_AGE_SECONDS", 120)?,
            max_provider_block_lag: positive_u64("VELA_INDEXER_MAX_PROVIDER_BLOCK_LAG", 3)?,
            batch_size: positive_u64("VELA_INDEXER_BATCH_SIZE", 500)?,
            audit_batch_size: positive_u64("VELA_INDEXER_AUDIT_BATCH_SIZE", 500)?,
            lease_duration: seconds("VELA_INDEXER_LEASE_SECONDS", 30)?,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), IndexerError> {
        if self.deployment_id.is_nil()
            || self.primary_name.trim().is_empty()
            || self.secondary_name.trim().is_empty()
            || self.primary_name.eq_ignore_ascii_case(&self.secondary_name)
            || provider_origin(&self.primary_url) == provider_origin(&self.secondary_url)
            || self.request_timeout.is_zero()
            || self.poll_interval.is_zero()
            || self.retry_max.is_zero()
            || self.max_head_age.is_zero()
            || self.max_provider_block_lag == 0
            || self.batch_size == 0
            || self.audit_batch_size == 0
            || self.lease_duration.is_zero()
        {
            return Err(IndexerError::Config(
                "deployment, distinct providers, bounded timing, and batch settings are required"
                    .to_owned(),
            ));
        }
        if self.retry_max < self.poll_interval {
            return Err(IndexerError::Config(
                "retry maximum must not be shorter than the normal poll interval".to_owned(),
            ));
        }
        let minimum_lease = self.request_timeout.checked_mul(6).ok_or_else(|| {
            IndexerError::Config("RPC timeout is too large for lease safety".to_owned())
        })?;
        if self.lease_duration < minimum_lease {
            return Err(IndexerError::Config(
                "lease must cover six bounded sequential RPC phases".to_owned(),
            ));
        }
        Ok(())
    }
}

fn required(name: &str) -> Result<String, IndexerError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| IndexerError::Config(format!("{name} must be set")))
}

fn required_url(name: &str) -> Result<Url, IndexerError> {
    let url = Url::parse(&required(name)?)
        .map_err(|_| IndexerError::Config(format!("{name} must be a valid URL")))?;
    let host = url.host_str().unwrap_or_default();
    let local_http = url.scheme() == "http"
        && (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1");
    if (url.scheme() != "https" && !local_http) || host.is_empty() {
        return Err(IndexerError::Config(format!(
            "{name} must use HTTPS unless it targets loopback"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(IndexerError::Config(format!(
            "{name} must not place credentials in URL userinfo"
        )));
    }
    Ok(url)
}

fn provider_origin(url: &Url) -> (String, String, Option<u16>) {
    (
        url.scheme().to_owned(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default(),
    )
}

fn positive_u64(name: &str, default: u64) -> Result<u64, IndexerError> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| IndexerError::Config(format!("{name} must be a positive integer")))?,
        Err(_) => default,
    };
    if value == 0 {
        return Err(IndexerError::Config(format!(
            "{name} must be a positive integer"
        )));
    }
    Ok(value)
}

fn millis(name: &str, default: u64) -> Result<Duration, IndexerError> {
    Ok(Duration::from_millis(positive_u64(name, default)?))
}

fn seconds(name: &str, default: u64) -> Result<Duration, IndexerError> {
    Ok(Duration::from_secs(positive_u64(name, default)?))
}

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("invalid indexer configuration: {0}")]
    Config(String),
    #[error("RPC provider {provider} failed: {message}")]
    Rpc { provider: String, message: String },
    #[error("indexer safety halt ({reason:?}): {message}")]
    Safety {
        reason: HaltReason,
        message: String,
        evidence: Value,
    },
    #[error("indexer persistence failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("ledger command failed: {0}")]
    Ledger(#[from] LedgerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltReason {
    ProviderChainMismatch,
    ProviderStale,
    ProviderDisagreement,
    FinalizedRegression,
    VaultCodeMismatch,
    MalformedLog,
    UnregisteredAsset,
    LedgerRejection,
    LeaseLost,
    CreditedDepositContradiction,
    HistoricalDepositAppeared,
}

impl HaltReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderChainMismatch => "provider_chain_mismatch",
            Self::ProviderStale => "provider_stale",
            Self::ProviderDisagreement => "provider_disagreement",
            Self::FinalizedRegression => "finalized_regression",
            Self::VaultCodeMismatch => "vault_code_mismatch",
            Self::MalformedLog => "malformed_log",
            Self::UnregisteredAsset => "unregistered_asset",
            Self::LedgerRejection => "ledger_rejection",
            Self::LeaseLost => "lease_lost",
            Self::CreditedDepositContradiction => "credited_deposit_contradiction",
            Self::HistoricalDepositAppeared => "historical_deposit_appeared",
        }
    }
}

fn safety(reason: HaltReason, message: impl Into<String>, evidence: Value) -> IndexerError {
    IndexerError::Safety {
        reason,
        message: message.into(),
        evidence,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Deployment {
    pub id: Uuid,
    pub chain_id: i64,
    pub network: String,
    pub vault_address: String,
    pub deployment_version: String,
    pub runtime_code_hash: String,
    pub deposit_event_signature: String,
    pub deposit_event_topic: String,
    pub finality_policy: String,
    pub start_block: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct NewDeployment {
    pub id: Uuid,
    pub chain_id: i64,
    pub network: String,
    pub vault_address: String,
    pub deployment_version: String,
    pub runtime_code_hash: String,
    pub start_block: i64,
}

/// Reviewed, nonsecret deployment evidence required before registry admission.
/// On-chain code and RPC agreement are checked separately by the indexer.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentManifest {
    pub schema_version: u32,
    pub id: Uuid,
    pub chain_id: i64,
    pub network: String,
    pub vault_address: String,
    pub deployment_version: String,
    pub runtime_code_hash: String,
    pub start_block: i64,
    pub source_commit: String,
    pub creation_bytecode_hash: String,
    pub runtime_template_hash: String,
    pub deposit_event_signature: String,
    pub deposit_event_topic: String,
}

impl DeploymentManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, IndexerError> {
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|_| IndexerError::Config("invalid V3 deployment manifest JSON".to_owned()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), IndexerError> {
        validate_new_deployment(&self.registration())?;
        if self.schema_version != 1
            || self.start_block == 0
            || !is_nonzero_hex_bytes(&self.source_commit, 32)
            || !is_nonzero_hex_bytes(&self.creation_bytecode_hash, 32)
            || !is_nonzero_hex_bytes(&self.runtime_template_hash, 32)
            || self.deposit_event_signature != DEPOSIT_EVENT_SIGNATURE
            || !self
                .deposit_event_topic
                .eq_ignore_ascii_case(DEPOSIT_EVENT_TOPIC)
        {
            return Err(IndexerError::Config(
                "V3 deployment manifest identity, build hashes, or event ABI is invalid".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn registration(&self) -> NewDeployment {
        NewDeployment {
            id: self.id,
            chain_id: self.chain_id,
            network: self.network.clone(),
            vault_address: self.vault_address.clone(),
            deployment_version: self.deployment_version.clone(),
            runtime_code_hash: self.runtime_code_hash.clone(),
            start_block: self.start_block,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkpoint {
    next_block: u64,
    last_finalized_block_number: Option<u64>,
    last_finalized_block_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditCheckpoint {
    next_block: u64,
    cycle: u64,
}

#[derive(Debug, Clone, FromRow)]
struct AuditableDeposit {
    deposit_id: Uuid,
    customer_reference: String,
    asset_id: Uuid,
    asset_code: String,
    state: String,
    chain_id: i64,
    block_number: i64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: i32,
    log_index: i32,
    vault_address: String,
    vault_deposit_id: String,
    sender_address: String,
    asset_address: Option<String>,
    actual_amount: BigDecimal,
}

#[derive(Clone)]
pub struct IndexerStore {
    pool: PgPool,
}

impl IndexerStore {
    pub async fn connect(database_url: &str) -> Result<Self, IndexerError> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn ledger(&self) -> Ledger {
        Ledger::from_pool(self.pool.clone())
    }

    pub async fn load_deployment(&self, id: Uuid) -> Result<Deployment, IndexerError> {
        let deployment = sqlx::query_as::<_, Deployment>(
            r#"
            SELECT id, chain_id, network, vault_address, deployment_version,
                   runtime_code_hash, deposit_event_signature, deposit_event_topic, finality_policy,
                   start_block, enabled
            FROM ledger_chain_deployments
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            IndexerError::Config(format!("deployment registry entry {id} was not found"))
        })?;
        validate_deployment(&deployment)?;
        Ok(deployment)
    }

    pub async fn register_deployment(
        &self,
        input: &NewDeployment,
    ) -> Result<Deployment, IndexerError> {
        validate_new_deployment(input)?;
        sqlx::query(
            r#"
            INSERT INTO ledger_chain_deployments (
                id, chain_id, network, vault_address, deployment_version,
                runtime_code_hash, deposit_event_signature, deposit_event_topic,
                finality_policy, start_block
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (chain_id, vault_address, deployment_version) DO NOTHING
            "#,
        )
        .bind(input.id)
        .bind(input.chain_id)
        .bind(input.network.trim())
        .bind(input.vault_address.to_ascii_lowercase())
        .bind(input.deployment_version.trim())
        .bind(input.runtime_code_hash.to_ascii_lowercase())
        .bind(DEPOSIT_EVENT_SIGNATURE)
        .bind(DEPOSIT_EVENT_TOPIC)
        .bind(FINALITY_POLICY)
        .bind(input.start_block)
        .execute(&self.pool)
        .await?;

        let deployment = sqlx::query_as::<_, Deployment>(
            r#"
            SELECT id, chain_id, network, vault_address, deployment_version,
                   runtime_code_hash, deposit_event_signature, deposit_event_topic, finality_policy,
                   start_block, enabled
            FROM ledger_chain_deployments
            WHERE chain_id = $1 AND vault_address = $2 AND deployment_version = $3
            "#,
        )
        .bind(input.chain_id)
        .bind(input.vault_address.to_ascii_lowercase())
        .bind(input.deployment_version.trim())
        .fetch_one(&self.pool)
        .await?;
        let expected = Deployment {
            id: input.id,
            chain_id: input.chain_id,
            network: input.network.trim().to_owned(),
            vault_address: input.vault_address.to_ascii_lowercase(),
            deployment_version: input.deployment_version.trim().to_owned(),
            runtime_code_hash: input.runtime_code_hash.to_ascii_lowercase(),
            deposit_event_signature: DEPOSIT_EVENT_SIGNATURE.to_owned(),
            deposit_event_topic: DEPOSIT_EVENT_TOPIC.to_owned(),
            finality_policy: FINALITY_POLICY.to_owned(),
            start_block: input.start_block,
            enabled: true,
        };
        if deployment != expected {
            return Err(IndexerError::Config(
                "existing deployment registry entry conflicts with requested immutable values"
                    .to_owned(),
            ));
        }
        Ok(deployment)
    }

    async fn acquire_lease(
        &self,
        deployment: &Deployment,
        owner: Uuid,
        lease_duration: Duration,
    ) -> Result<Checkpoint, IndexerError> {
        let seconds = i64::try_from(lease_duration.as_secs())
            .map_err(|_| IndexerError::Config("lease duration is too large".to_owned()))?;
        let row = sqlx::query_as::<_, (i64, Option<i64>, Option<String>)>(
            r#"
            INSERT INTO ledger_indexer_checkpoints (
                deployment_id, next_block, lease_owner, lease_expires_at
            ) VALUES ($1, $2, $3, transaction_timestamp() + ($4 * interval '1 second'))
            ON CONFLICT (deployment_id) DO UPDATE
            SET lease_owner = EXCLUDED.lease_owner,
                lease_expires_at = EXCLUDED.lease_expires_at,
                updated_at = transaction_timestamp()
            WHERE ledger_indexer_checkpoints.lease_owner = EXCLUDED.lease_owner
               OR ledger_indexer_checkpoints.lease_expires_at <= transaction_timestamp()
            RETURNING next_block, last_finalized_block_number, last_finalized_block_hash
            "#,
        )
        .bind(deployment.id)
        .bind(deployment.start_block)
        .bind(owner)
        .bind(seconds)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            safety(
                HaltReason::LeaseLost,
                "another indexer owns the deployment lease",
                json!({"deployment_id": deployment.id}),
            )
        })?;
        Ok(Checkpoint {
            next_block: u64::try_from(row.0)
                .map_err(|_| IndexerError::Config("negative checkpoint".to_owned()))?,
            last_finalized_block_number: row
                .1
                .map(u64::try_from)
                .transpose()
                .map_err(|_| IndexerError::Config("negative finalized checkpoint".to_owned()))?,
            last_finalized_block_hash: row.2,
        })
    }

    async fn load_audit_checkpoint(
        &self,
        deployment: &Deployment,
        owner: Uuid,
    ) -> Result<AuditCheckpoint, IndexerError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            r#"
            INSERT INTO ledger_indexer_audit_checkpoints (deployment_id, next_block)
            SELECT $1, $2
            WHERE EXISTS (
                SELECT 1 FROM ledger_indexer_checkpoints
                WHERE deployment_id = $1 AND lease_owner = $3
                  AND lease_expires_at > transaction_timestamp()
            )
            ON CONFLICT (deployment_id) DO UPDATE
            SET updated_at = ledger_indexer_audit_checkpoints.updated_at
            WHERE EXISTS (
                SELECT 1 FROM ledger_indexer_checkpoints
                WHERE deployment_id = $1 AND lease_owner = $3
                  AND lease_expires_at > transaction_timestamp()
            )
            RETURNING next_block, cycle
            "#,
        )
        .bind(deployment.id)
        .bind(deployment.start_block)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            safety(
                HaltReason::LeaseLost,
                "indexer lease expired before history audit",
                json!({"deployment_id": deployment.id}),
            )
        })?;
        Ok(AuditCheckpoint {
            next_block: u64::try_from(row.0)
                .map_err(|_| IndexerError::Config("negative audit checkpoint".to_owned()))?,
            cycle: u64::try_from(row.1)
                .map_err(|_| IndexerError::Config("negative audit cycle".to_owned()))?,
        })
    }

    async fn advance_audit_checkpoint(
        &self,
        deployment: &Deployment,
        owner: Uuid,
        next_block: u64,
        cycle: u64,
        last_scanned: Option<&RpcBlock>,
    ) -> Result<(), IndexerError> {
        let next_block = i64::try_from(next_block).map_err(|_| {
            IndexerError::Config("audit checkpoint exceeds PostgreSQL bigint".to_owned())
        })?;
        let cycle = i64::try_from(cycle).map_err(|_| {
            IndexerError::Config("audit cycle exceeds PostgreSQL bigint".to_owned())
        })?;
        let last_number = last_scanned
            .map(|block| i64::try_from(block.number))
            .transpose()
            .map_err(|_| {
                IndexerError::Config("audited block exceeds PostgreSQL bigint".to_owned())
            })?;
        let last_hash = last_scanned.map(|block| block.hash.as_str());
        let updated = sqlx::query(
            r#"
            UPDATE ledger_indexer_audit_checkpoints audit
            SET next_block = $3, cycle = $4,
                last_scanned_block_number = $5,
                last_scanned_block_hash = $6,
                updated_at = transaction_timestamp()
            WHERE audit.deployment_id = $1
              AND EXISTS (
                  SELECT 1 FROM ledger_indexer_checkpoints live
                  WHERE live.deployment_id = $1 AND live.lease_owner = $2
                    AND live.lease_expires_at > transaction_timestamp()
              )
            "#,
        )
        .bind(deployment.id)
        .bind(owner)
        .bind(next_block)
        .bind(cycle)
        .bind(last_number)
        .bind(last_hash)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(safety(
                HaltReason::LeaseLost,
                "indexer lease expired before history audit checkpoint",
                json!({"deployment_id": deployment.id}),
            ));
        }
        Ok(())
    }

    async fn auditable_deposits(
        &self,
        deployment: &Deployment,
        from: u64,
        to: u64,
    ) -> Result<Vec<AuditableDeposit>, IndexerError> {
        let from = i64::try_from(from).map_err(|_| {
            IndexerError::Config("audit range exceeds PostgreSQL bigint".to_owned())
        })?;
        let to = i64::try_from(to).map_err(|_| {
            IndexerError::Config("audit range exceeds PostgreSQL bigint".to_owned())
        })?;
        Ok(sqlx::query_as::<_, AuditableDeposit>(
            r#"
            SELECT d.id AS deposit_id, d.customer_reference, d.asset_id,
                   asset.code AS asset_code, d.state, event.chain_id,
                   event.block_number, event.block_hash, event.transaction_hash,
                   event.transaction_index, event.log_index, event.vault_address,
                   event.vault_deposit_id, event.sender_address,
                   event.asset_address, event.actual_amount
            FROM ledger_deposits d
            JOIN ledger_deposit_chain_events event ON event.deposit_id = d.id
            JOIN ledger_assets asset ON asset.id = d.asset_id
            WHERE event.chain_id = $1 AND lower(event.vault_address) = lower($2)
              AND event.block_number BETWEEN $3 AND $4
            ORDER BY event.block_number, event.log_index, lower(event.transaction_hash)
            "#,
        )
        .bind(deployment.chain_id)
        .bind(&deployment.vault_address)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn advance_checkpoint(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        next_block: u64,
        finalized: &RpcBlock,
        lease_duration: Duration,
    ) -> Result<(), IndexerError> {
        let next_block = i64::try_from(next_block)
            .map_err(|_| IndexerError::Config("checkpoint exceeds PostgreSQL bigint".to_owned()))?;
        let finalized_number = i64::try_from(finalized.number).map_err(|_| {
            IndexerError::Config("finalized height exceeds PostgreSQL bigint".to_owned())
        })?;
        let seconds = i64::try_from(lease_duration.as_secs())
            .map_err(|_| IndexerError::Config("lease duration is too large".to_owned()))?;
        let updated = sqlx::query(
            r#"
            UPDATE ledger_indexer_checkpoints
            SET next_block = $3,
                last_finalized_block_number = $4,
                last_finalized_block_hash = $5,
                lease_expires_at = transaction_timestamp() + ($6 * interval '1 second'),
                updated_at = transaction_timestamp()
            WHERE deployment_id = $1
              AND lease_owner = $2
              AND lease_expires_at > transaction_timestamp()
            "#,
        )
        .bind(deployment_id)
        .bind(owner)
        .bind(next_block)
        .bind(finalized_number)
        .bind(&finalized.hash)
        .bind(seconds)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(safety(
                HaltReason::LeaseLost,
                "indexer lease expired before checkpoint commit",
                json!({"deployment_id": deployment_id}),
            ));
        }
        Ok(())
    }

    pub async fn record_halt(
        &self,
        deployment_id: Uuid,
        reason: HaltReason,
        evidence: &Value,
    ) -> Result<(), IndexerError> {
        sqlx::query(
            "INSERT INTO ledger_indexer_halts (deployment_id, reason, evidence) VALUES ($1, $2, $3)",
        )
        .bind(deployment_id)
        .bind(reason.as_str())
        .bind(evidence)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn release_lease(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
    ) -> Result<(), IndexerError> {
        sqlx::query(
            r#"
            UPDATE ledger_indexer_checkpoints
            SET lease_owner = NULL, lease_expires_at = NULL, updated_at = transaction_timestamp()
            WHERE deployment_id = $1 AND lease_owner = $2
            "#,
        )
        .bind(deployment_id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn validate_new_deployment(input: &NewDeployment) -> Result<(), IndexerError> {
    let network_matches_chain = matches!(
        (input.chain_id, input.network.trim()),
        (8453, "base") | (84532, "base-sepolia")
    );
    if !network_matches_chain
        || input.id.is_nil()
        || input.network.trim().is_empty()
        || input.network.len() > 64
        || input.deployment_version.trim() != "v3"
        || input.start_block < 0
        || !is_hex_bytes(&input.vault_address, 20)
        || !is_hex_bytes(&input.runtime_code_hash, 32)
    {
        return Err(IndexerError::Config(
            "deployment registry values are malformed or not a supported network".to_owned(),
        ));
    }
    Ok(())
}

fn validate_deployment(deployment: &Deployment) -> Result<(), IndexerError> {
    if !deployment.enabled {
        return Err(IndexerError::Config(format!(
            "deployment {} is disabled",
            deployment.id
        )));
    }
    if deployment.deposit_event_signature != DEPOSIT_EVENT_SIGNATURE
        || !deployment
            .deposit_event_topic
            .eq_ignore_ascii_case(DEPOSIT_EVENT_TOPIC)
        || deployment.finality_policy != FINALITY_POLICY
    {
        return Err(IndexerError::Config(
            "deployment uses an unsupported deposit event ABI".to_owned(),
        ));
    }
    validate_new_deployment(&NewDeployment {
        id: deployment.id,
        chain_id: deployment.chain_id,
        network: deployment.network.clone(),
        vault_address: deployment.vault_address.clone(),
        deployment_version: deployment.deployment_version.clone(),
        runtime_code_hash: deployment.runtime_code_hash.clone(),
        start_block: deployment.start_block,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcBlock {
    pub number: u64,
    pub hash: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub block_number: String,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: String,
    pub log_index: String,
    #[serde(default)]
    pub removed: bool,
}

impl RpcLog {
    fn normalize(mut self) -> Self {
        self.address.make_ascii_lowercase();
        self.data.make_ascii_lowercase();
        self.block_hash.make_ascii_lowercase();
        self.transaction_hash.make_ascii_lowercase();
        for topic in &mut self.topics {
            topic.make_ascii_lowercase();
        }
        self
    }

    fn key(&self) -> Result<(u64, u64, String), IndexerError> {
        Ok((
            parse_quantity(&self.block_number)?,
            parse_quantity(&self.log_index)?,
            self.transaction_hash.clone(),
        ))
    }
}

// `async_trait` adds `must_use` to boxed futures. Clippy 1.99 also treats
// their Result output as must-use, so the generated methods trip this lint.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Rpc: Send + Sync {
    fn name(&self) -> &str;
    async fn chain_id(&self) -> Result<u64, IndexerError>;
    async fn block(&self, tag: &str) -> Result<RpcBlock, IndexerError>;
    async fn logs(
        &self,
        address: &str,
        topic: &str,
        from: u64,
        to: u64,
    ) -> Result<Vec<RpcLog>, IndexerError>;
    async fn code(&self, address: &str, block: u64) -> Result<String, IndexerError>;
}

pub struct HttpRpc {
    name: String,
    url: Url,
    client: reqwest::Client,
    request_id: AtomicU64,
}

impl HttpRpc {
    pub fn new(name: String, url: Url, timeout: Duration) -> Result<Self, IndexerError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| IndexerError::Config("could not construct RPC HTTP client".to_owned()))?;
        Ok(Self {
            name,
            url,
            client,
            request_id: AtomicU64::new(1),
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, IndexerError> {
        #[derive(Deserialize)]
        struct Response<T> {
            result: Option<T>,
            error: Option<RpcError>,
        }
        #[derive(Deserialize)]
        struct RpcError {
            code: i64,
            message: String,
        }

        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .send()
            .await
            .map_err(|_| IndexerError::Rpc {
                provider: self.name.clone(),
                message: "request failed or timed out".to_owned(),
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(IndexerError::Rpc {
                provider: self.name.clone(),
                message: format!("HTTP status {status}"),
            });
        }
        let body: Response<T> = response.json().await.map_err(|_| IndexerError::Rpc {
            provider: self.name.clone(),
            message: "response was not valid JSON-RPC".to_owned(),
        })?;
        if let Some(error) = body.error {
            return Err(IndexerError::Rpc {
                provider: self.name.clone(),
                message: format!("JSON-RPC error {}: {}", error.code, error.message),
            });
        }
        body.result.ok_or_else(|| IndexerError::Rpc {
            provider: self.name.clone(),
            message: "JSON-RPC response omitted result".to_owned(),
        })
    }
}

#[derive(Deserialize)]
struct WireBlock {
    number: Option<String>,
    hash: Option<String>,
    timestamp: String,
}

#[async_trait]
impl Rpc for HttpRpc {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chain_id(&self) -> Result<u64, IndexerError> {
        let value: String = self.call("eth_chainId", json!([])).await?;
        parse_quantity(&value)
    }

    async fn block(&self, tag: &str) -> Result<RpcBlock, IndexerError> {
        let wire: WireBlock = self
            .call("eth_getBlockByNumber", json!([tag, false]))
            .await?;
        let number = wire.number.ok_or_else(|| IndexerError::Rpc {
            provider: self.name.clone(),
            message: "block response omitted number".to_owned(),
        })?;
        let hash = wire.hash.ok_or_else(|| IndexerError::Rpc {
            provider: self.name.clone(),
            message: "block response omitted hash".to_owned(),
        })?;
        if !is_hex_bytes(&hash, 32) {
            return Err(IndexerError::Rpc {
                provider: self.name.clone(),
                message: "block response contained malformed hash".to_owned(),
            });
        }
        Ok(RpcBlock {
            number: parse_quantity(&number)?,
            hash: hash.to_ascii_lowercase(),
            timestamp: parse_quantity(&wire.timestamp)?,
        })
    }

    async fn logs(
        &self,
        address: &str,
        topic: &str,
        from: u64,
        to: u64,
    ) -> Result<Vec<RpcLog>, IndexerError> {
        self.call(
            "eth_getLogs",
            json!([{
                "address": address,
                "topics": [topic],
                "fromBlock": quantity(from),
                "toBlock": quantity(to),
            }]),
        )
        .await
    }

    async fn code(&self, address: &str, block: u64) -> Result<String, IndexerError> {
        self.call("eth_getCode", json!([address, quantity(block)]))
            .await
    }
}

fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

fn parse_quantity(value: &str) -> Result<u64, IndexerError> {
    let hex = value
        .strip_prefix("0x")
        .ok_or_else(|| IndexerError::Config("RPC quantity was not 0x-prefixed".to_owned()))?;
    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IndexerError::Config(
            "RPC quantity was malformed".to_owned(),
        ));
    }
    u64::from_str_radix(hex, 16)
        .map_err(|_| IndexerError::Config("RPC quantity exceeded u64".to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedDeposit {
    pub vault_deposit_id: String,
    pub depositor: String,
    /// Opaque bytes32 routing identifier. This is the only customer-facing
    /// value committed by the omnibus vault; it is not an on-chain balance.
    pub credited_account_id: String,
    pub asset_address: Option<String>,
    pub amount: BigDecimal,
    pub block_number: u64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub transaction_index: u64,
    pub log_index: u64,
}

pub fn decode_deposit(
    log: RpcLog,
    deployment: &Deployment,
) -> Result<DecodedDeposit, IndexerError> {
    let log = log.normalize();
    let valid_shape = !log.removed
        && log.address.eq_ignore_ascii_case(&deployment.vault_address)
        && log.topics.len() == 4
        && log.topics[0].eq_ignore_ascii_case(&deployment.deposit_event_topic)
        && is_hex_bytes(&log.topics[1], 32)
        && is_padded_address(&log.topics[2])
        && is_hex_bytes(&log.topics[3], 32)
        && is_hex_bytes(&log.data, 64)
        && is_hex_bytes(&log.block_hash, 32)
        && is_hex_bytes(&log.transaction_hash, 32);
    if !valid_shape {
        return Err(safety(
            HaltReason::MalformedLog,
            "deposit log did not match the pinned ABI",
            json!({
                "transaction_hash": safe_hash(&log.transaction_hash),
                "log_index": log.log_index,
            }),
        ));
    }
    let amount = BigDecimal::from_str(&u256_hex_to_decimal(&data_word(&log.data, 1)).map_err(
        |message| {
            safety(
                HaltReason::MalformedLog,
                message,
                json!({"transaction_hash": log.transaction_hash, "log_index": log.log_index}),
            )
        },
    )?)
    .map_err(|_| {
        safety(
            HaltReason::MalformedLog,
            "deposit amount could not be represented exactly",
            json!({"transaction_hash": log.transaction_hash, "log_index": log.log_index}),
        )
    })?;
    if amount <= BigDecimal::zero() {
        return Err(safety(
            HaltReason::MalformedLog,
            "deposit amount must be positive",
            json!({"transaction_hash": log.transaction_hash, "log_index": log.log_index}),
        ));
    }
    let asset = topic_address(&data_word(&log.data, 0));
    Ok(DecodedDeposit {
        vault_deposit_id: log.topics[1].clone(),
        depositor: topic_address(&log.topics[2]),
        credited_account_id: log.topics[3].to_ascii_lowercase(),
        asset_address: (asset != ZERO_ADDRESS).then_some(asset),
        amount,
        block_number: parse_quantity(&log.block_number)?,
        block_hash: log.block_hash,
        transaction_hash: log.transaction_hash,
        transaction_index: parse_quantity(&log.transaction_index)?,
        log_index: parse_quantity(&log.log_index)?,
    })
}

fn data_word(value: &str, index: usize) -> String {
    let start = 2 + index * 64;
    format!("0x{}", &value[start..start + 64])
}

fn is_padded_address(value: &str) -> bool {
    is_hex_bytes(value, 32) && value[2..26].bytes().all(|byte| byte == b'0')
}

fn topic_address(value: &str) -> String {
    format!("0x{}", &value[value.len() - 40..]).to_ascii_lowercase()
}

fn u256_hex_to_decimal(value: &str) -> Result<String, &'static str> {
    let bytes = hex::decode(
        value
            .strip_prefix("0x")
            .ok_or("amount was not 0x-prefixed")?,
    )
    .map_err(|_| "amount was not hex")?;
    if bytes.len() != 32 {
        return Err("amount was not a uint256");
    }
    let mut digits = vec![0_u8];
    for byte in bytes {
        let mut carry = u16::from(byte);
        for digit in &mut digits {
            let value = u16::from(*digit) * 256 + carry;
            *digit = (value % 10) as u8;
            carry = value / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    Ok(digits
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + *digit))
        .collect())
}

fn is_hex_bytes(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn is_nonzero_hex_bytes(value: &str, bytes: usize) -> bool {
    is_hex_bytes(value, bytes) && value.as_bytes()[2..].iter().any(|byte| *byte != b'0')
}

fn safe_hash(value: &str) -> Option<String> {
    is_hex_bytes(value, 32).then(|| value.to_ascii_lowercase())
}

pub struct DepositIndexer<P, S> {
    config: Config,
    deployment: Deployment,
    store: IndexerStore,
    ledger: Ledger,
    primary: P,
    secondary: S,
    owner: Uuid,
}

impl<P: Rpc, S: Rpc> DepositIndexer<P, S> {
    pub async fn new(
        config: Config,
        store: IndexerStore,
        primary: P,
        secondary: S,
    ) -> Result<Self, IndexerError> {
        config.validate()?;
        let deployment = store.load_deployment(config.deployment_id).await?;
        let ledger = store.ledger();
        ledger.ensure_schema_current().await?;
        Ok(Self {
            config,
            deployment,
            store,
            ledger,
            primary,
            secondary,
            owner: Uuid::new_v4(),
        })
    }

    pub async fn run(self) -> Result<(), IndexerError> {
        self.run_until_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .map_err(|_| IndexerError::Config("could not install shutdown signal".to_owned()))
        })
        .await
    }

    pub async fn run_until_shutdown<F>(mut self, shutdown: F) -> Result<(), IndexerError>
    where
        F: Future<Output = Result<(), IndexerError>> + Send,
    {
        tokio::pin!(shutdown);
        let mut backoff = self.config.poll_interval;
        loop {
            let delay = tokio::select! {
                result = &mut shutdown => {
                    self.store.release_lease(self.deployment.id, self.owner).await?;
                    result?;
                    info!(deployment_id = %self.deployment.id, "deposit indexer shut down cleanly");
                    return Ok(());
                }
                result = self.tick() => {
                    match result {
                        Ok(processed) => {
                            backoff = self.config.poll_interval;
                            if processed > 0 {
                                info!(processed, deployment_id = %self.deployment.id, "credited finalized deposits");
                            }
                            self.config.poll_interval
                        }
                        Err(error) => {
                            let halt = match &error {
                                IndexerError::Safety { reason, evidence, .. } => {
                                    Some((*reason, evidence.clone()))
                                }
                                IndexerError::Rpc { provider, message } => Some((
                                    HaltReason::ProviderStale,
                                    json!({"provider": provider, "error": message}),
                                )),
                                _ => None,
                            };
                            if let Some((reason, evidence)) = halt {
                                if let Err(record_error) = self.store.record_halt(self.deployment.id, reason, &evidence).await {
                                    error!(error = %record_error, "could not persist indexer halt evidence");
                                }
                            }
                            warn!(error = %error, retry_ms = backoff.as_millis(), "deposit credits halted");
                            let delay = backoff;
                            backoff = min(backoff.saturating_mul(2), self.config.retry_max);
                            delay
                        }
                    }
                }
            };
            tokio::select! {
                result = &mut shutdown => {
                    self.store.release_lease(self.deployment.id, self.owner).await?;
                    result?;
                    info!(deployment_id = %self.deployment.id, "deposit indexer shut down cleanly");
                    return Ok(());
                }
                _ = sleep(delay) => {}
            }
        }
    }

    pub async fn tick(&mut self) -> Result<usize, IndexerError> {
        let checkpoint = self
            .store
            .acquire_lease(&self.deployment, self.owner, self.config.lease_duration)
            .await?;
        self.verify_chain_ids().await?;
        let (primary_latest, secondary_latest) =
            tokio::try_join!(self.primary.block("latest"), self.secondary.block("latest"))?;
        self.verify_live_heads(&primary_latest, &secondary_latest)?;
        let finalized = self.agreed_finalized_head(&checkpoint).await?;
        self.verify_vault_code(finalized.number).await?;
        self.audit_finalized_history(&checkpoint, &finalized)
            .await?;

        if checkpoint.next_block > finalized.number {
            self.store
                .advance_checkpoint(
                    self.deployment.id,
                    self.owner,
                    checkpoint.next_block,
                    &finalized,
                    self.config.lease_duration,
                )
                .await?;
            return Ok(0);
        }
        let to = min(
            finalized.number,
            checkpoint
                .next_block
                .saturating_add(self.config.batch_size - 1),
        );
        let (first_logs, second_logs) = tokio::try_join!(
            self.primary.logs(
                &self.deployment.vault_address,
                &self.deployment.deposit_event_topic,
                checkpoint.next_block,
                to,
            ),
            self.secondary.logs(
                &self.deployment.vault_address,
                &self.deployment.deposit_event_topic,
                checkpoint.next_block,
                to,
            ),
        )?;
        let first_logs = normalize_logs(first_logs)?;
        let second_logs = normalize_logs(second_logs)?;
        if first_logs != second_logs {
            return Err(safety(
                HaltReason::ProviderDisagreement,
                "RPC providers returned different deposit logs",
                json!({
                    "from_block": checkpoint.next_block,
                    "to_block": to,
                    "primary_count": first_logs.len(),
                    "secondary_count": second_logs.len(),
                }),
            ));
        }

        let mut processed = 0;
        for raw in first_logs {
            let deposit = decode_deposit(raw, &self.deployment)?;
            if deposit.block_number < checkpoint.next_block || deposit.block_number > to {
                return Err(safety(
                    HaltReason::MalformedLog,
                    "RPC returned a deposit outside the requested finalized range",
                    json!({
                        "transaction_hash": deposit.transaction_hash,
                        "log_index": deposit.log_index,
                        "block_number": deposit.block_number,
                        "from_block": checkpoint.next_block,
                        "to_block": to,
                    }),
                ));
            }
            self.process_deposit(&deposit, &finalized).await?;
            processed += 1;
        }
        self.store
            .advance_checkpoint(
                self.deployment.id,
                self.owner,
                to.saturating_add(1),
                &finalized,
                self.config.lease_duration,
            )
            .await?;
        Ok(processed)
    }

    async fn verify_chain_ids(&self) -> Result<(), IndexerError> {
        let (first, second) = tokio::try_join!(self.primary.chain_id(), self.secondary.chain_id())?;
        let expected = u64::try_from(self.deployment.chain_id)
            .map_err(|_| IndexerError::Config("deployment chain ID is invalid".to_owned()))?;
        if first != expected || second != expected {
            return Err(safety(
                HaltReason::ProviderChainMismatch,
                "an RPC provider reported the wrong chain ID",
                json!({"expected": expected, "primary": first, "secondary": second}),
            ));
        }
        Ok(())
    }

    fn verify_live_heads(&self, first: &RpcBlock, second: &RpcBlock) -> Result<(), IndexerError> {
        let now = Utc::now().timestamp();
        let max_age = i64::try_from(self.config.max_head_age.as_secs()).unwrap_or(i64::MAX);
        let stale = [first, second].iter().any(|block| {
            let timestamp = i64::try_from(block.timestamp).unwrap_or(i64::MAX);
            timestamp > now.saturating_add(30) || now.saturating_sub(timestamp) > max_age
        });
        let lag = first.number.abs_diff(second.number);
        if stale || lag > self.config.max_provider_block_lag {
            return Err(safety(
                HaltReason::ProviderStale,
                "RPC latest heads are stale or too far apart",
                json!({
                    "primary_number": first.number,
                    "primary_timestamp": first.timestamp,
                    "secondary_number": second.number,
                    "secondary_timestamp": second.timestamp,
                    "max_block_lag": self.config.max_provider_block_lag,
                    "max_head_age_seconds": self.config.max_head_age.as_secs(),
                }),
            ));
        }
        Ok(())
    }

    async fn agreed_finalized_head(
        &self,
        checkpoint: &Checkpoint,
    ) -> Result<RpcBlock, IndexerError> {
        let (first_tip, second_tip) = tokio::try_join!(
            self.primary.block("finalized"),
            self.secondary.block("finalized")
        )?;
        let common_number = min(first_tip.number, second_tip.number);
        let tag = quantity(common_number);
        let (first, second) =
            tokio::try_join!(self.primary.block(&tag), self.secondary.block(&tag))?;
        if first.number != common_number
            || second.number != common_number
            || !first.hash.eq_ignore_ascii_case(&second.hash)
        {
            return Err(safety(
                HaltReason::ProviderDisagreement,
                "RPC providers disagree on the common finalized block",
                json!({
                    "block_number": common_number,
                    "primary_hash": first.hash,
                    "secondary_hash": second.hash,
                }),
            ));
        }
        if checkpoint
            .last_finalized_block_number
            .is_some_and(|number| common_number < number)
            || (checkpoint.last_finalized_block_number == Some(common_number)
                && checkpoint
                    .last_finalized_block_hash
                    .as_ref()
                    .is_some_and(|hash| !hash.eq_ignore_ascii_case(&first.hash)))
        {
            return Err(safety(
                HaltReason::FinalizedRegression,
                "agreed finalized chain regressed or changed hash",
                json!({
                    "previous_number": checkpoint.last_finalized_block_number,
                    "previous_hash": checkpoint.last_finalized_block_hash,
                    "current_number": common_number,
                    "current_hash": first.hash,
                }),
            ));
        }
        Ok(first)
    }

    async fn verify_vault_code(&self, block: u64) -> Result<(), IndexerError> {
        let (first, second) = tokio::try_join!(
            self.primary.code(&self.deployment.vault_address, block),
            self.secondary.code(&self.deployment.vault_address, block)
        )?;
        let first_hash = code_hash(&first)?;
        let second_hash = code_hash(&second)?;
        if first_hash != second_hash
            || !first_hash.eq_ignore_ascii_case(&self.deployment.runtime_code_hash)
        {
            return Err(safety(
                HaltReason::VaultCodeMismatch,
                "vault runtime bytecode does not match the pinned deployment",
                json!({
                    "block_number": block,
                    "expected_hash": self.deployment.runtime_code_hash,
                    "primary_hash": first_hash,
                    "secondary_hash": second_hash,
                }),
            ));
        }
        Ok(())
    }

    async fn audit_finalized_history(
        &self,
        checkpoint: &Checkpoint,
        finalized: &RpcBlock,
    ) -> Result<(), IndexerError> {
        let audit = self
            .store
            .load_audit_checkpoint(&self.deployment, self.owner)
            .await?;
        let processed_end = checkpoint
            .next_block
            .saturating_sub(1)
            .min(finalized.number);
        if audit.next_block > processed_end {
            if audit.next_block
                > u64::try_from(self.deployment.start_block)
                    .map_err(|_| IndexerError::Config("negative deployment start".to_owned()))?
            {
                self.store
                    .advance_audit_checkpoint(
                        &self.deployment,
                        self.owner,
                        u64::try_from(self.deployment.start_block).map_err(|_| {
                            IndexerError::Config("negative deployment start".to_owned())
                        })?,
                        audit.cycle.saturating_add(1),
                        None,
                    )
                    .await?;
            }
            return Ok(());
        }
        let to = processed_end.min(
            audit
                .next_block
                .saturating_add(self.config.audit_batch_size - 1),
        );
        let (first_logs, second_logs) = tokio::try_join!(
            self.primary.logs(
                &self.deployment.vault_address,
                &self.deployment.deposit_event_topic,
                audit.next_block,
                to,
            ),
            self.secondary.logs(
                &self.deployment.vault_address,
                &self.deployment.deposit_event_topic,
                audit.next_block,
                to,
            ),
        )?;
        let first_logs = normalize_logs(first_logs)?;
        let second_logs = normalize_logs(second_logs)?;
        if first_logs != second_logs {
            return Err(safety(
                HaltReason::ProviderDisagreement,
                "RPC providers disagree while re-auditing finalized deposit history",
                json!({
                    "from_block": audit.next_block,
                    "to_block": to,
                    "audit_cycle": audit.cycle,
                    "primary_count": first_logs.len(),
                    "secondary_count": second_logs.len(),
                }),
            ));
        }

        let mut canonical = HashMap::with_capacity(first_logs.len());
        for raw in first_logs {
            let decoded = decode_deposit(raw, &self.deployment)?;
            if decoded.block_number < audit.next_block || decoded.block_number > to {
                return Err(safety(
                    HaltReason::MalformedLog,
                    "RPC returned a historical deposit outside the audit range",
                    event_evidence(&decoded),
                ));
            }
            canonical.insert(
                (decoded.transaction_hash.clone(), decoded.log_index),
                decoded,
            );
        }
        let stored = self
            .store
            .auditable_deposits(&self.deployment, audit.next_block, to)
            .await?;
        let stored_by_key: HashMap<_, _> = stored
            .iter()
            .map(|deposit| {
                (
                    (
                        deposit.transaction_hash.to_ascii_lowercase(),
                        u64::try_from(deposit.log_index).unwrap_or(u64::MAX),
                    ),
                    deposit,
                )
            })
            .collect();

        // Contain invalidated credits before reporting any newly appeared event in
        // the same rewritten range. Otherwise an added replacement log could halt
        // the scan before the old customer credit is isolated.
        for stored in &stored {
            if DepositState::from_str(&stored.state)? != DepositState::Credited {
                continue;
            }
            let canonical_log = canonical.get(&(
                stored.transaction_hash.to_ascii_lowercase(),
                u64::try_from(stored.log_index).map_err(|_| {
                    IndexerError::Config("stored log index was negative".to_owned())
                })?,
            ));
            let canonical_log_matches = canonical_log.is_some_and(|event| {
                stored_event_matches(stored, event, &self.deployment.vault_address)
            });
            if canonical_log_matches {
                continue;
            }
            let canonical_block = self
                .agreed_historical_block(
                    u64::try_from(stored.block_number).map_err(|_| {
                        IndexerError::Config("stored block number was negative".to_owned())
                    })?,
                    audit.cycle,
                )
                .await?;
            self.reverse_credited_contradiction(
                stored,
                &canonical_block,
                finalized,
                canonical_log_matches,
            )
            .await?;
        }

        for decoded in canonical.values() {
            if !stored_by_key.contains_key(&(
                decoded.transaction_hash.to_ascii_lowercase(),
                decoded.log_index,
            )) {
                let asset = self
                    .ledger
                    .resolve_chain_asset(self.deployment.chain_id, decoded.asset_address.as_deref())
                    .await
                    .map_err(|error| ledger_halt(decoded, error))?;
                let evidence = json!({
                    "audit_cycle": audit.cycle,
                    "from_block": audit.next_block,
                    "to_block": to,
                    "finalized_block_number": finalized.number,
                    "finalized_block_hash": finalized.hash,
                    "event": event_evidence(decoded),
                    "primary_provider": self.primary.name(),
                    "secondary_provider": self.secondary.name(),
                });
                self.ledger
                    .activate_asset_halt(&ActivateAssetHalt {
                        halt_id: deterministic_audit_uuid("halt-historical", decoded),
                        asset_id: asset.id,
                        deployment_id: self.deployment.id,
                        source_deposit_id: None,
                        reason: AssetHaltReason::HistoricalDepositAppeared,
                        evidence: evidence.clone(),
                    })
                    .await
                    .map_err(|error| ledger_halt(decoded, error))?;
                return Err(safety(
                    HaltReason::HistoricalDepositAppeared,
                    "a deposit appeared behind the durable forward checkpoint",
                    evidence,
                ));
            }
        }

        let range_tip = self.agreed_historical_block(to, audit.cycle).await?;
        self.store
            .advance_audit_checkpoint(
                &self.deployment,
                self.owner,
                to.saturating_add(1),
                audit.cycle,
                Some(&range_tip),
            )
            .await
    }

    async fn agreed_historical_block(
        &self,
        block_number: u64,
        audit_cycle: u64,
    ) -> Result<RpcBlock, IndexerError> {
        let tag = quantity(block_number);
        let (first, second) =
            tokio::try_join!(self.primary.block(&tag), self.secondary.block(&tag))?;
        if first.number != block_number
            || second.number != block_number
            || !first.hash.eq_ignore_ascii_case(&second.hash)
        {
            return Err(safety(
                HaltReason::ProviderDisagreement,
                "RPC providers disagree on a previously finalized block",
                json!({
                    "block_number": block_number,
                    "audit_cycle": audit_cycle,
                    "primary_hash": first.hash,
                    "secondary_hash": second.hash,
                }),
            ));
        }
        Ok(first)
    }

    async fn reverse_credited_contradiction(
        &self,
        stored: &AuditableDeposit,
        canonical_block: &RpcBlock,
        finalized: &RpcBlock,
        canonical_log_matches: bool,
    ) -> Result<(), IndexerError> {
        let pending_lookup = AccountLookup {
            asset_code: stored.asset_code.clone(),
            owner_type: "customer".to_owned(),
            owner_reference: stored.customer_reference.clone(),
            account_type: "customer_pending".to_owned(),
        };
        let pending = self
            .ledger
            .ensure_accounts(&[AccountProvisionRequest {
                asset_code: stored.asset_code.clone(),
                owner_type: "customer".to_owned(),
                owner_reference: stored.customer_reference.clone(),
                account_type: "customer_pending".to_owned(),
                allow_negative: false,
            }])
            .await
            .map_err(|error| stored_ledger_halt(stored, error))?
            .remove(&pending_lookup)
            .ok_or_else(|| {
                safety(
                    HaltReason::LedgerRejection,
                    "pending account provisioning returned no account",
                    stored_event_evidence(stored),
                )
            })?;
        let observation = RpcCanonicalContradiction {
            provider: self.primary.name().to_owned(),
            chain_id: stored.chain_id,
            finalized_block_number: checked_i64(finalized.number, "finalized block number")?,
            finalized_block_hash: finalized.hash.clone(),
            deposit_block_number: stored.block_number,
            stored_block_hash: stored.block_hash.clone(),
            canonical_block_hash: canonical_block.hash.clone(),
            transaction_hash: stored.transaction_hash.clone(),
            log_index: stored.log_index,
            canonical_log_matches,
        };
        let command = ReverseCreditedDeposit {
            deposit_id: stored.deposit_id,
            deployment_id: self.deployment.id,
            pending_account_id: pending.id,
            reversal_transaction_id: deterministic_stored_uuid("reversal", stored),
            halt_id: deterministic_stored_uuid("halt", stored),
            first: observation.clone(),
            second: RpcCanonicalContradiction {
                provider: self.secondary.name().to_owned(),
                ..observation
            },
        };
        self.ledger
            .activate_asset_halt(&ActivateAssetHalt {
                halt_id: command.halt_id,
                asset_id: stored.asset_id,
                deployment_id: self.deployment.id,
                source_deposit_id: Some(stored.deposit_id),
                reason: AssetHaltReason::CreditedDepositContradiction,
                evidence: json!({
                    "deposit_id": stored.deposit_id,
                    "transaction_hash": stored.transaction_hash,
                    "log_index": stored.log_index,
                    "stored_block_hash": stored.block_hash,
                    "canonical_block_hash": canonical_block.hash,
                    "canonical_log_matches": canonical_log_matches,
                    "primary_provider": self.primary.name(),
                    "secondary_provider": self.secondary.name(),
                }),
            })
            .await
            .map_err(|error| stored_ledger_halt(stored, error))?;
        let outcome = self
            .ledger
            .reverse_credited_deposit(&command)
            .await
            .map_err(|error| stored_ledger_halt(stored, error))?;
        let outcome_name = match outcome {
            DepositContradictionOutcome::Reversed { .. } => "reversed_to_pending",
            DepositContradictionOutcome::Quarantined { .. } => "quarantined_shortfall",
        };
        let evidence = json!({
            "deposit_id": stored.deposit_id,
            "transaction_hash": stored.transaction_hash,
            "log_index": stored.log_index,
            "stored_block_hash": stored.block_hash,
            "canonical_block_hash": canonical_block.hash,
            "canonical_log_matches": canonical_log_matches,
            "outcome": outcome_name,
        });
        Err(safety(
            HaltReason::CreditedDepositContradiction,
            "a credited deposit contradicted canonical finalized history",
            evidence,
        ))
    }

    async fn process_deposit(
        &self,
        deposit: &DecodedDeposit,
        finalized: &RpcBlock,
    ) -> Result<(), IndexerError> {
        let asset = self
            .ledger
            .resolve_chain_asset(self.deployment.chain_id, deposit.asset_address.as_deref())
            .await
            .map_err(|error| {
                safety(
                    HaltReason::UnregisteredAsset,
                    "deposit referenced an unregistered asset",
                    json!({
                        "transaction_hash": deposit.transaction_hash,
                        "log_index": deposit.log_index,
                        "asset_address": deposit.asset_address,
                        "ledger_error": error.to_string(),
                    }),
                )
            })?;
        let customer_lookup = AccountLookup {
            asset_code: asset.code.clone(),
            owner_type: "customer".to_owned(),
            owner_reference: deposit.credited_account_id.clone(),
            account_type: "customer_available".to_owned(),
        };
        let customer = self
            .ledger
            .ensure_accounts(&[AccountProvisionRequest {
                asset_code: asset.code.clone(),
                owner_type: "customer".to_owned(),
                owner_reference: deposit.credited_account_id.clone(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            }])
            .await
            .map_err(|error| ledger_halt(deposit, error))?
            .remove(&customer_lookup)
            .ok_or_else(|| {
                safety(
                    HaltReason::LedgerRejection,
                    "customer account provisioning returned no account",
                    event_evidence(deposit),
                )
            })?;
        let custody_lookup = AccountLookup {
            asset_code: asset.code.clone(),
            owner_type: "custody".to_owned(),
            owner_reference: self.deployment.vault_address.clone(),
            account_type: "custody_vault".to_owned(),
        };
        let custody = self
            .ledger
            .resolve_accounts(std::slice::from_ref(&custody_lookup))
            .await
            .map_err(|error| ledger_halt(deposit, error))?
            .remove(&custody_lookup)
            .ok_or_else(|| {
                safety(
                    HaltReason::LedgerRejection,
                    "custody account lookup returned no account",
                    event_evidence(deposit),
                )
            })?;
        let deposit_id = deterministic_uuid("deposit", deposit);
        let observed = self
            .ledger
            .observe_deposit(&ObserveDeposit {
                deposit_id,
                customer_reference: deposit.credited_account_id.clone(),
                destination_account_id: customer.id,
                asset_id: asset.id,
                amount: deposit.amount.clone(),
                event: DepositChainEvent {
                    chain_id: self.deployment.chain_id,
                    block_number: checked_i64(deposit.block_number, "block number")?,
                    block_hash: deposit.block_hash.clone(),
                    transaction_hash: deposit.transaction_hash.clone(),
                    transaction_index: checked_i32(deposit.transaction_index, "transaction index")?,
                    log_index: checked_i32(deposit.log_index, "log index")?,
                    vault_address: self.deployment.vault_address.clone(),
                    vault_deposit_id: deposit.vault_deposit_id.clone(),
                    sender_address: deposit.depositor.clone(),
                    asset_address: deposit.asset_address.clone(),
                    actual_amount: deposit.amount.clone(),
                },
            })
            .await
            .map_err(|error| ledger_halt(deposit, error))?;

        let mut state = observed.state;
        if state == DepositState::Observed {
            state = self
                .ledger
                .mark_deposit_confirming(
                    deposit_id,
                    finalized
                        .number
                        .saturating_sub(deposit.block_number)
                        .saturating_add(1),
                    checked_i64(finalized.number, "finalized block number")?,
                    &finalized.hash,
                )
                .await
                .map_err(|error| ledger_halt(deposit, error))?
                .state;
        }
        if state == DepositState::Confirming {
            let evidence = RpcFinalityObservation {
                provider: self.primary.name().to_owned(),
                chain_id: self.deployment.chain_id,
                finalized_block_number: checked_i64(finalized.number, "finalized block number")?,
                finalized_block_hash: finalized.hash.clone(),
                deposit_block_hash: deposit.block_hash.clone(),
                transaction_hash: deposit.transaction_hash.clone(),
                log_index: checked_i32(deposit.log_index, "log index")?,
                vault_address: self.deployment.vault_address.clone(),
                vault_deposit_id: deposit.vault_deposit_id.clone(),
                sender_address: deposit.depositor.clone(),
                credited_account_reference: deposit.credited_account_id.clone(),
                asset_address: deposit.asset_address.clone(),
                actual_amount: deposit.amount.clone(),
            };
            state = self
                .ledger
                .finalize_deposit(&FinalizeDeposit {
                    deposit_id,
                    first: evidence.clone(),
                    second: RpcFinalityObservation {
                        provider: self.secondary.name().to_owned(),
                        ..evidence
                    },
                })
                .await
                .map_err(|error| ledger_halt(deposit, error))?
                .state;
        }
        if state == DepositState::Finalized {
            self.ledger
                .credit_finalized_deposit(
                    deposit_id,
                    custody.id,
                    deterministic_uuid("credit", deposit),
                )
                .await
                .map_err(|error| ledger_halt(deposit, error))?;
        } else if state != DepositState::Credited {
            return Err(safety(
                HaltReason::LedgerRejection,
                format!("deposit is in non-creditable state {}", state.as_str()),
                event_evidence(deposit),
            ));
        }
        Ok(())
    }
}

fn normalize_logs(logs: Vec<RpcLog>) -> Result<Vec<RpcLog>, IndexerError> {
    let mut logs: Vec<_> = logs.into_iter().map(RpcLog::normalize).collect();
    logs.sort_by_key(|log| log.key().unwrap_or((u64::MAX, u64::MAX, String::new())));
    for log in &logs {
        log.key()?;
    }
    for duplicate in logs.windows(2) {
        if duplicate[0].key()? == duplicate[1].key()? {
            return Err(safety(
                HaltReason::MalformedLog,
                "RPC returned the same canonical log more than once",
                json!({
                    "transaction_hash": duplicate[0].transaction_hash,
                    "log_index": duplicate[0].log_index,
                }),
            ));
        }
    }
    Ok(logs)
}

fn code_hash(code: &str) -> Result<String, IndexerError> {
    let raw = code.strip_prefix("0x").ok_or_else(|| {
        safety(
            HaltReason::VaultCodeMismatch,
            "vault bytecode was not 0x-prefixed",
            json!({}),
        )
    })?;
    let bytes = hex::decode(raw).map_err(|_| {
        safety(
            HaltReason::VaultCodeMismatch,
            "vault bytecode was malformed",
            json!({}),
        )
    })?;
    if bytes.is_empty() {
        return Err(safety(
            HaltReason::VaultCodeMismatch,
            "vault has no runtime bytecode at the finalized block",
            json!({}),
        ));
    }
    Ok(format!("0x{}", hex::encode(Keccak256::digest(bytes))))
}

fn checked_i64(value: u64, field: &str) -> Result<i64, IndexerError> {
    i64::try_from(value).map_err(|_| {
        safety(
            HaltReason::MalformedLog,
            format!("{field} exceeds signed 64-bit storage"),
            json!({"value": value}),
        )
    })
}

fn checked_i32(value: u64, field: &str) -> Result<i32, IndexerError> {
    i32::try_from(value).map_err(|_| {
        safety(
            HaltReason::MalformedLog,
            format!("{field} exceeds signed 32-bit storage"),
            json!({"value": value}),
        )
    })
}

fn deterministic_uuid(namespace: &str, deposit: &DecodedDeposit) -> Uuid {
    let identity = format!(
        "vela:{namespace}:{}:{}:{}",
        deposit.transaction_hash, deposit.log_index, deposit.vault_deposit_id
    );
    Uuid::new_v5(&Uuid::NAMESPACE_URL, identity.as_bytes())
}

fn deterministic_audit_uuid(namespace: &str, deposit: &DecodedDeposit) -> Uuid {
    deterministic_uuid(namespace, deposit)
}

fn deterministic_stored_uuid(namespace: &str, deposit: &AuditableDeposit) -> Uuid {
    let identity = format!(
        "vela:{namespace}:{}:{}:{}",
        deposit.transaction_hash.to_ascii_lowercase(),
        deposit.log_index,
        deposit.vault_deposit_id
    );
    Uuid::new_v5(&Uuid::NAMESPACE_URL, identity.as_bytes())
}

fn stored_event_matches(
    stored: &AuditableDeposit,
    canonical: &DecodedDeposit,
    expected_vault: &str,
) -> bool {
    stored.chain_id > 0
        && u64::try_from(stored.block_number) == Ok(canonical.block_number)
        && stored
            .block_hash
            .eq_ignore_ascii_case(&canonical.block_hash)
        && stored
            .transaction_hash
            .eq_ignore_ascii_case(&canonical.transaction_hash)
        && u64::try_from(stored.transaction_index) == Ok(canonical.transaction_index)
        && u64::try_from(stored.log_index) == Ok(canonical.log_index)
        && stored.vault_address.eq_ignore_ascii_case(expected_vault)
        && stored.vault_deposit_id == canonical.vault_deposit_id
        && stored
            .sender_address
            .eq_ignore_ascii_case(&canonical.depositor)
        && optional_address_matches(&stored.asset_address, &canonical.asset_address)
        && stored.actual_amount == canonical.amount
}

fn optional_address_matches(left: &Option<String>, right: &Option<String>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn event_evidence(deposit: &DecodedDeposit) -> Value {
    json!({
        "transaction_hash": deposit.transaction_hash,
        "log_index": deposit.log_index,
        "vault_deposit_id": deposit.vault_deposit_id,
    })
}

fn ledger_halt(deposit: &DecodedDeposit, error: LedgerError) -> IndexerError {
    let mut evidence = event_evidence(deposit);
    evidence["ledger_error"] = Value::String(error.to_string());
    safety(
        HaltReason::LedgerRejection,
        "ledger rejected a finalized deposit",
        evidence,
    )
}

fn stored_event_evidence(deposit: &AuditableDeposit) -> Value {
    json!({
        "deposit_id": deposit.deposit_id,
        "asset_id": deposit.asset_id,
        "transaction_hash": deposit.transaction_hash,
        "log_index": deposit.log_index,
        "vault_deposit_id": deposit.vault_deposit_id,
    })
}

fn stored_ledger_halt(deposit: &AuditableDeposit, error: LedgerError) -> IndexerError {
    let mut evidence = stored_event_evidence(deposit);
    evidence["ledger_error"] = Value::String(error.to_string());
    safety(
        HaltReason::LedgerRejection,
        "ledger rejected a canonical-history recovery command",
        evidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_network_must_match_supported_chain() {
        let mut input = NewDeployment {
            id: Uuid::new_v4(),
            chain_id: 84532,
            network: "base-sepolia".to_owned(),
            vault_address: format!("0x{}", "11".repeat(20)),
            deployment_version: "v3".to_owned(),
            runtime_code_hash: format!("0x{}", "22".repeat(32)),
            start_block: 0,
        };
        assert!(validate_new_deployment(&input).is_ok());
        input.network = "ethereum-sepolia".to_owned();
        assert!(validate_new_deployment(&input).is_err());
        input.chain_id = 11155111;
        assert!(validate_new_deployment(&input).is_err());
        input.chain_id = 1;
        input.network = "ethereum".to_owned();
        assert!(validate_new_deployment(&input).is_err());
        input.chain_id = 8453;
        input.network = "base".to_owned();
        assert!(validate_new_deployment(&input).is_ok());
        input.deployment_version = "v2".to_owned();
        assert!(validate_new_deployment(&input).is_err());
        input.deployment_version = "v3".to_owned();
        input.id = Uuid::nil();
        assert!(validate_new_deployment(&input).is_err());
    }

    #[test]
    fn registry_manifest_binds_v3_source_and_deposit_abi() {
        let mut manifest = DeploymentManifest {
            schema_version: 1,
            id: Uuid::new_v4(),
            chain_id: 84532,
            network: "base-sepolia".to_owned(),
            vault_address: format!("0x{}", "11".repeat(20)),
            deployment_version: "v3".to_owned(),
            runtime_code_hash: format!("0x{}", "22".repeat(32)),
            start_block: 1,
            source_commit: format!("0x{}", "33".repeat(32)),
            creation_bytecode_hash: format!("0x{}", "44".repeat(32)),
            runtime_template_hash: format!("0x{}", "55".repeat(32)),
            deposit_event_signature: DEPOSIT_EVENT_SIGNATURE.to_owned(),
            deposit_event_topic: DEPOSIT_EVENT_TOPIC.to_owned(),
        };
        let encoded = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(DeploymentManifest::parse(&encoded).unwrap().id, manifest.id);

        manifest.deposit_event_signature = "Deposited(address,uint256)".to_owned();
        assert!(manifest.validate().is_err());
        manifest.deposit_event_signature = DEPOSIT_EVENT_SIGNATURE.to_owned();
        manifest.source_commit = format!("0x{}", "00".repeat(32));
        assert!(manifest.validate().is_err());
        manifest.source_commit = format!("0x{}", "33".repeat(32));
        manifest.start_block = 0;
        assert!(manifest.validate().is_err());
    }

    fn config() -> Config {
        Config {
            database_url: "postgres://unused".to_owned(),
            deployment_id: Uuid::new_v4(),
            primary_name: "provider-a".to_owned(),
            primary_url: Url::parse("https://provider-a.example/rpc").unwrap(),
            secondary_name: "provider-b".to_owned(),
            secondary_url: Url::parse("https://provider-b.example/rpc").unwrap(),
            request_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_secs(2),
            retry_max: Duration::from_secs(30),
            max_head_age: Duration::from_secs(120),
            max_provider_block_lag: 3,
            batch_size: 500,
            audit_batch_size: 500,
            lease_duration: Duration::from_secs(30),
        }
    }

    fn deployment() -> Deployment {
        Deployment {
            id: Uuid::new_v4(),
            chain_id: 8453,
            network: "base".to_owned(),
            vault_address: "0x1111111111111111111111111111111111111111".to_owned(),
            deployment_version: "v3".to_owned(),
            runtime_code_hash: format!("0x{}", "22".repeat(32)),
            deposit_event_signature: DEPOSIT_EVENT_SIGNATURE.to_owned(),
            deposit_event_topic: DEPOSIT_EVENT_TOPIC.to_owned(),
            finality_policy: FINALITY_POLICY.to_owned(),
            start_block: 100,
            enabled: true,
        }
    }

    fn log() -> RpcLog {
        RpcLog {
            address: deployment().vault_address,
            topics: vec![
                DEPOSIT_EVENT_TOPIC.to_owned(),
                format!("0x{}", "33".repeat(32)),
                format!("0x{}{}", "00".repeat(12), "44".repeat(20)),
                format!("0x{}", "55".repeat(32)),
            ],
            data: format!(
                "0x{}{}{:064x}",
                "00".repeat(12),
                "66".repeat(20),
                1_500_000_u64
            ),
            block_number: "0x64".to_owned(),
            block_hash: format!("0x{}", "66".repeat(32)),
            transaction_hash: format!("0x{}", "77".repeat(32)),
            transaction_index: "0x2".to_owned(),
            log_index: "0x3".to_owned(),
            removed: false,
        }
    }

    #[test]
    fn decodes_only_the_pinned_deposit_abi() {
        let decoded = decode_deposit(log(), &deployment()).unwrap();
        assert_eq!(decoded.amount, BigDecimal::from(1_500_000_u64));
        assert_eq!(decoded.depositor, format!("0x{}", "44".repeat(20)));
        assert_eq!(
            decoded.credited_account_id,
            format!("0x{}", "55".repeat(32))
        );
        assert_eq!(
            decoded.asset_address,
            Some(format!("0x{}", "66".repeat(20)))
        );
        assert_eq!(decoded.log_index, 3);
    }

    #[test]
    fn rejects_removed_wrong_topic_and_noncanonical_address_topics() {
        let mut removed = log();
        removed.removed = true;
        assert!(matches!(
            decode_deposit(removed, &deployment()),
            Err(IndexerError::Safety {
                reason: HaltReason::MalformedLog,
                ..
            })
        ));

        let mut wrong_topic = log();
        wrong_topic.topics[0] = format!("0x{}", "99".repeat(32));
        assert!(decode_deposit(wrong_topic, &deployment()).is_err());

        let mut dirty_padding = log();
        dirty_padding.topics[2].replace_range(2..4, "01");
        assert!(decode_deposit(dirty_padding, &deployment()).is_err());

        let mut malformed_account_id = log();
        malformed_account_id.topics[3] = format!("0x{}", "55".repeat(31));
        assert!(decode_deposit(malformed_account_id, &deployment()).is_err());
    }

    #[test]
    fn parses_full_uint256_without_float_or_u128_loss() {
        let value = format!("0x{}", "ff".repeat(32));
        assert_eq!(
            u256_hex_to_decimal(&value).unwrap(),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
    }

    #[test]
    fn code_hash_matches_keccak_of_runtime_bytes() {
        assert_eq!(
            code_hash("0x00").unwrap(),
            "0xbc36789e7a1e281436464229828f817d6612f7b477d66591ff96a9e064bcc98a"
        );
        assert!(code_hash("0x").is_err());
    }

    #[test]
    fn pinned_topic_matches_the_contract_event_signature() {
        assert_eq!(
            format!(
                "0x{}",
                hex::encode(Keccak256::digest(DEPOSIT_EVENT_SIGNATURE.as_bytes()))
            ),
            DEPOSIT_EVENT_TOPIC
        );
    }

    #[test]
    fn duplicate_canonical_logs_are_rejected() {
        let duplicate = log();
        assert!(matches!(
            normalize_logs(vec![duplicate.clone(), duplicate]),
            Err(IndexerError::Safety {
                reason: HaltReason::MalformedLog,
                ..
            })
        ));
    }

    #[test]
    fn unsafe_runtime_timing_and_provider_configuration_is_rejected() {
        let mut value = config();
        value.secondary_url = value.primary_url.clone();
        assert!(matches!(value.validate(), Err(IndexerError::Config(_))));

        let mut value = config();
        value.lease_duration = Duration::from_secs(5);
        assert!(matches!(value.validate(), Err(IndexerError::Config(_))));

        let mut value = config();
        value.retry_max = Duration::from_secs(1);
        assert!(matches!(value.validate(), Err(IndexerError::Config(_))));
    }
}

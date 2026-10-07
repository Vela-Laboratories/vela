use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ledger::{
    FinalizedWithdrawalExecution, FinalizedWithdrawalNonExecution, FinalizedWithdrawalRevert,
    Ledger, RecordDroppedWithdrawalAttempt, RecordObservedWithdrawalReplacement,
};
use reqwest::Url;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use std::{env, future::Future, time::Duration};
use thiserror::Error;
use tokio::time::sleep;
use uuid::Uuid;

pub mod relayer_policy;

pub const WITHDRAWAL_EVENT_SIGNATURE: &str =
    "WithdrawalExecuted(bytes16,address,address,uint256,uint256,uint256,bytes32,bytes32)";
pub const WITHDRAWAL_EVENT_TOPIC: &str =
    "0xa951166031255fae1b92eb5b6b591a70e7c95c53f3de7f7c957cfc4a0e1d3300";

#[derive(Clone)]
pub struct Config {
    pub enabled: bool,
    pub database_url: String,
    pub deployment_id: Uuid,
    pub primary_name: String,
    pub primary_url: Url,
    pub secondary_name: String,
    pub secondary_url: Url,
    pub request_timeout: Duration,
    pub poll_interval: Duration,
    pub retry_max: Duration,
    pub lease_duration: Duration,
    pub batch_size: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, WorkerError> {
        let enabled = env::var("VELA_WITHDRAWAL_WORKER_ENABLED").ok().as_deref() == Some("true");
        if !enabled {
            return Err(WorkerError::Disabled);
        }
        let primary_url = required_url("VELA_WITHDRAWAL_RPC_PRIMARY_URL")?;
        let secondary_url = required_url("VELA_WITHDRAWAL_RPC_SECONDARY_URL")?;
        if origin(&primary_url) == origin(&secondary_url) {
            return Err(WorkerError::Config(
                "withdrawal RPC endpoints must use distinct origins".to_owned(),
            ));
        }
        let primary_name = required("VELA_WITHDRAWAL_RPC_PRIMARY_NAME")?;
        let secondary_name = required("VELA_WITHDRAWAL_RPC_SECONDARY_NAME")?;
        if primary_name.eq_ignore_ascii_case(&secondary_name) {
            return Err(WorkerError::Config(
                "withdrawal RPC provider names must differ".to_owned(),
            ));
        }
        Ok(Self {
            enabled,
            database_url: required("DATABASE_URL")?,
            deployment_id: required("VELA_WITHDRAWAL_DEPLOYMENT_ID")?
                .parse()
                .map_err(|_| WorkerError::Config("deployment ID must be a UUID".to_owned()))?,
            primary_name,
            primary_url,
            secondary_name,
            secondary_url,
            request_timeout: millis("VELA_WITHDRAWAL_RPC_TIMEOUT_MS", 5_000, 60_000)?,
            poll_interval: millis("VELA_WITHDRAWAL_POLL_MS", 2_000, 60_000)?,
            retry_max: millis("VELA_WITHDRAWAL_RETRY_MAX_MS", 30_000, 300_000)?,
            lease_duration: seconds("VELA_WITHDRAWAL_LEASE_SECONDS", 60, 600)?,
            batch_size: bounded("VELA_WITHDRAWAL_BATCH_SIZE", 100, 1_000)? as u32,
        })
    }

    pub fn validate(&self) -> Result<(), WorkerError> {
        if !self.enabled {
            return Err(WorkerError::Disabled);
        }
        if origin(&self.primary_url) == origin(&self.secondary_url)
            || self.primary_name.eq_ignore_ascii_case(&self.secondary_name)
        {
            return Err(WorkerError::Config(
                "two independent RPC origins and provider names are required".to_owned(),
            ));
        }
        let minimum_lease = self.request_timeout.checked_mul(8).ok_or_else(|| {
            WorkerError::Config("RPC timeout is too large for lease safety".to_owned())
        })?;
        if self.deployment_id.is_nil()
            || self.request_timeout.is_zero()
            || self.poll_interval.is_zero()
            || self.retry_max.is_zero()
            || self.lease_duration.is_zero()
            || self.batch_size == 0
        {
            return Err(WorkerError::Config(
                "deployment, bounded timing, and batch settings are required".to_owned(),
            ));
        }
        if self.lease_duration < minimum_lease {
            return Err(WorkerError::Config(
                "lease duration must cover eight bounded sequential RPC requests".to_owned(),
            ));
        }
        if self.retry_max < self.poll_interval {
            return Err(WorkerError::Config(
                "retry maximum must not be shorter than the normal poll interval".to_owned(),
            ));
        }
        Ok(())
    }
}

fn required(name: &str) -> Result<String, WorkerError> {
    env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| WorkerError::Config(format!("{name} must be set")))
}

fn required_url(name: &str) -> Result<Url, WorkerError> {
    let url = Url::parse(&required(name)?)
        .map_err(|_| WorkerError::Config(format!("{name} must be a valid URL")))?;
    let host = url.host_str().unwrap_or_default();
    let loopback = url.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "::1");
    if host.is_empty() || (url.scheme() != "https" && !loopback) {
        return Err(WorkerError::Config(format!(
            "{name} must use HTTPS unless it targets loopback"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(WorkerError::Config(format!(
            "{name} must not contain URL userinfo"
        )));
    }
    Ok(url)
}

fn origin(url: &Url) -> (String, String, Option<u16>) {
    (
        url.scheme().to_owned(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default(),
    )
}

fn bounded(name: &str, default: u64, maximum: u64) -> Result<u64, WorkerError> {
    let value = env::var(name)
        .map(|v| v.parse::<u64>())
        .unwrap_or(Ok(default))
        .map_err(|_| WorkerError::Config(format!("{name} must be a positive integer")))?;
    if value == 0 || value > maximum {
        return Err(WorkerError::Config(format!(
            "{name} must be between 1 and {maximum}"
        )));
    }
    Ok(value)
}

fn millis(name: &str, default: u64, maximum: u64) -> Result<Duration, WorkerError> {
    Ok(Duration::from_millis(bounded(name, default, maximum)?))
}

fn seconds(name: &str, default: u64, maximum: u64) -> Result<Duration, WorkerError> {
    Ok(Duration::from_secs(bounded(name, default, maximum)?))
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("withdrawal worker is disabled; set VELA_WITHDRAWAL_WORKER_ENABLED=true to opt in")]
    Disabled,
    #[error("invalid withdrawal worker configuration: {0}")]
    Config(String),
    #[error("withdrawal provider {provider} failed: {message}")]
    Provider { provider: String, message: String },
    #[error("withdrawal safety check failed: {0}")]
    Safety(String),
    #[error("withdrawal worker lease is owned by another process")]
    LeaseUnavailable,
    #[error("withdrawal worker persistence failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("ledger reconciliation failed: {0}")]
    Ledger(#[from] ledger::LedgerError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub id: Uuid,
    pub chain_id: u64,
    pub vault_address: String,
    pub runtime_code_hash: String,
    pub event_topic: String,
    pub start_block: u64,
    pub policy_version: String,
    pub min_confirmations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub withdrawal_id: Uuid,
    pub attempt_id: Uuid,
    pub transaction_hash: String,
    pub expires_at: DateTime<Utc>,
    pub destination: String,
    pub asset: String,
    pub amount: String,
    pub nonce: String,
    pub policy_hash: String,
    pub approver_hash: String,
    pub sender: String,
    pub sender_nonce: String,
    pub calldata_hash: String,
    pub updated_at: DateTime<Utc>,
    pub current_attempt_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizedBlock {
    pub number: u64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionFacts {
    pub hash: String,
    pub sender: String,
    pub sender_nonce: String,
    pub destination: String,
    pub calldata_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLog {
    pub block: FinalizedBlock,
    pub transaction_index: u32,
    pub log_index: u32,
    pub request_id: Uuid,
    pub destination: String,
    pub asset: String,
    pub amount: String,
    pub nonce: String,
    pub deadline: String,
    pub policy_hash: String,
    pub approver_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionStatus {
    NotFound,
    Pending(TransactionFacts),
    FinalizedRevert(TransactionFacts, FinalizedBlock),
    FinalizedExecution(TransactionFacts, Box<ExecutionLog>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderObservation {
    pub finalized: FinalizedBlock,
    pub status: TransactionStatus,
    pub request_id_unused: bool,
    pub withdrawal_nonce_unused: bool,
    pub no_execution_event: bool,
    pub provider_name: String,
    pub provider_reference: String,
}

// `async_trait` generates must-use futures; Clippy also flags their Result output.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ChainProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chain_id(&self) -> Result<u64, WorkerError>;
    async fn finalized_block(&self) -> Result<FinalizedBlock, WorkerError>;
    async fn runtime_code(&self, address: &str, block: u64) -> Result<Vec<u8>, WorkerError>;
    async fn observe(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
    ) -> Result<ProviderObservation, WorkerError>;
}

// `async_trait` generates must-use futures; Clippy also flags their Result output.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait WorkerStore: Send + Sync {
    async fn deployment(&self, id: Uuid) -> Result<Deployment, WorkerError>;
    async fn acquire_lease(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        duration: Duration,
    ) -> Result<(), WorkerError>;
    async fn release_lease(&self, deployment_id: Uuid, owner: Uuid) -> Result<(), WorkerError>;
    async fn candidates(
        &self,
        deployment: &Deployment,
        owner: Uuid,
        limit: u32,
    ) -> Result<Vec<Candidate>, WorkerError>;
    async fn all_attempts_terminal(&self, withdrawal_id: Uuid) -> Result<bool, WorkerError>;
    async fn record_execution(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError>;
    async fn record_dropped(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError>;
    async fn record_revert(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError>;
    async fn record_replacement(
        &self,
        deployment: &Deployment,
        current: &Candidate,
        replacement: &Candidate,
        submitted_at: DateTime<Utc>,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError>;
    async fn release_permanent_failure(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError>;
    async fn checkpoint(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        last: Option<&Candidate>,
        duration: Duration,
    ) -> Result<(), WorkerError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Executed,
    Dropped,
    Reverted,
    ReplacementRecorded,
    Released,
}

pub struct WithdrawalWorker<S, A, B> {
    config: Config,
    store: S,
    primary: A,
    secondary: B,
    deployment: Deployment,
    owner: Uuid,
}

impl<S, A, B> WithdrawalWorker<S, A, B>
where
    S: WorkerStore,
    A: ChainProvider,
    B: ChainProvider,
{
    pub async fn new(
        config: Config,
        store: S,
        primary: A,
        secondary: B,
    ) -> Result<Self, WorkerError> {
        config.validate()?;
        if primary.name().eq_ignore_ascii_case(secondary.name()) {
            return Err(WorkerError::Config(
                "provider identities must be distinct".to_owned(),
            ));
        }
        let deployment = store.deployment(config.deployment_id).await?;
        validate_startup(&deployment, &primary, &secondary).await?;
        Ok(Self {
            config,
            store,
            primary,
            secondary,
            deployment,
            owner: Uuid::new_v4(),
        })
    }

    pub async fn tick(&self) -> Result<Vec<Outcome>, WorkerError> {
        self.store
            .acquire_lease(self.deployment.id, self.owner, self.config.lease_duration)
            .await?;
        let candidates = self
            .store
            .candidates(&self.deployment, self.owner, self.config.batch_size)
            .await?;
        let mut outcomes = Vec::with_capacity(candidates.len());
        if candidates.is_empty() {
            self.store
                .checkpoint(
                    self.deployment.id,
                    self.owner,
                    None,
                    self.config.lease_duration,
                )
                .await?;
            return Ok(outcomes);
        }
        for candidate in candidates {
            self.store
                .acquire_lease(self.deployment.id, self.owner, self.config.lease_duration)
                .await?;
            outcomes.push(self.process(&candidate).await?);
            self.store
                .checkpoint(
                    self.deployment.id,
                    self.owner,
                    Some(&candidate),
                    self.config.lease_duration,
                )
                .await?;
        }
        Ok(outcomes)
    }

    pub async fn process(&self, candidate: &Candidate) -> Result<Outcome, WorkerError> {
        let (primary, secondary) = tokio::join!(
            self.primary.observe(&self.deployment, candidate),
            self.secondary.observe(&self.deployment, candidate)
        );
        let primary = primary?;
        let secondary = secondary?;
        ensure_agreement(&primary, &secondary, candidate, &self.deployment)?;

        match &primary.status {
            TransactionStatus::FinalizedExecution(_, _) => {
                self.store
                    .record_execution(&self.deployment, candidate, &primary, &secondary)
                    .await?;
                Ok(Outcome::Executed)
            }
            TransactionStatus::NotFound => {
                if !candidate.current_attempt_terminal {
                    self.store
                        .record_dropped(&self.deployment, candidate, &primary, &secondary)
                        .await?;
                }
                if primary.finalized.timestamp < candidate.expires_at {
                    return Ok(Outcome::Dropped);
                }
                if primary.request_id_unused
                    && primary.withdrawal_nonce_unused
                    && primary.no_execution_event
                    && self
                        .store
                        .all_attempts_terminal(candidate.withdrawal_id)
                        .await?
                {
                    self.store
                        .release_permanent_failure(
                            &self.deployment,
                            candidate,
                            &primary,
                            &secondary,
                        )
                        .await?;
                    Ok(Outcome::Released)
                } else {
                    Ok(Outcome::Pending)
                }
            }
            TransactionStatus::FinalizedRevert(_, _) => {
                self.store
                    .record_revert(&self.deployment, candidate, &primary, &secondary)
                    .await?;
                if primary.finalized.timestamp < candidate.expires_at {
                    return Ok(Outcome::Reverted);
                }
                if primary.request_id_unused
                    && primary.withdrawal_nonce_unused
                    && primary.no_execution_event
                    && self
                        .store
                        .all_attempts_terminal(candidate.withdrawal_id)
                        .await?
                {
                    self.store
                        .release_permanent_failure(
                            &self.deployment,
                            candidate,
                            &primary,
                            &secondary,
                        )
                        .await?;
                    Ok(Outcome::Released)
                } else {
                    Ok(Outcome::Reverted)
                }
            }
            TransactionStatus::Pending(_) => Ok(Outcome::Pending),
        }
    }

    /// Register a replacement hash supplied by a trusted relay or finalized-block scanner.
    /// Standard JSON-RPC cannot discover an arbitrary pending same-nonce replacement by the
    /// predecessor hash, so this boundary verifies a supplied hash against both providers.
    pub async fn ingest_replacement(
        &self,
        current: &Candidate,
        replacement_hash: &str,
        submitted_at: DateTime<Utc>,
    ) -> Result<Outcome, WorkerError> {
        if !is_hash(replacement_hash)
            || replacement_hash.eq_ignore_ascii_case(&current.transaction_hash)
        {
            return Err(WorkerError::Safety(
                "replacement candidate requires a distinct canonical transaction hash".to_owned(),
            ));
        }
        let mut replacement = current.clone();
        replacement.attempt_id = replacement_id(current, replacement_hash);
        replacement.transaction_hash = replacement_hash.to_ascii_lowercase();
        replacement.current_attempt_terminal = false;
        let (primary, secondary) = tokio::join!(
            self.primary.observe(&self.deployment, &replacement),
            self.secondary.observe(&self.deployment, &replacement)
        );
        let primary = primary?;
        let secondary = secondary?;
        ensure_agreement(&primary, &secondary, &replacement, &self.deployment)?;
        if matches!(primary.status, TransactionStatus::NotFound) {
            return Err(WorkerError::Safety(
                "both providers must observe the supplied replacement transaction".to_owned(),
            ));
        }
        self.store
            .record_replacement(
                &self.deployment,
                current,
                &replacement,
                submitted_at,
                &primary,
                &secondary,
            )
            .await?;
        Ok(Outcome::ReplacementRecorded)
    }

    pub async fn shutdown(&self) -> Result<(), WorkerError> {
        self.store
            .release_lease(self.deployment.id, self.owner)
            .await
    }

    pub async fn run(self) -> Result<(), WorkerError> {
        self.run_until_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .map_err(|_| WorkerError::Config("could not install shutdown handler".to_owned()))
        })
        .await
    }

    pub async fn run_until_shutdown<F>(self, shutdown: F) -> Result<(), WorkerError>
    where
        F: Future<Output = Result<(), WorkerError>> + Send,
    {
        tokio::pin!(shutdown);
        let mut retry = self.config.poll_interval;
        loop {
            let delay = tokio::select! {
                result = &mut shutdown => {
                    self.shutdown().await?;
                    result?;
                    return Ok(());
                }
                result = self.tick() => {
                    match result {
                        Ok(_) => {
                            retry = self.config.poll_interval;
                            self.config.poll_interval
                        }
                        Err(WorkerError::LeaseUnavailable) => self.config.poll_interval,
                        Err(error) => {
                            tracing::warn!(error = %error, "withdrawal worker cycle failed closed");
                            let delay = retry;
                            retry = retry.saturating_mul(2).min(self.config.retry_max);
                            delay
                        }
                    }
                }
            };
            tokio::select! {
                result = &mut shutdown => {
                    self.shutdown().await?;
                    result?;
                    return Ok(());
                }
                _ = sleep(delay) => {}
            }
        }
    }
}

async fn validate_startup<A: ChainProvider, B: ChainProvider>(
    deployment: &Deployment,
    primary: &A,
    secondary: &B,
) -> Result<(), WorkerError> {
    let (a_chain, b_chain) = tokio::join!(primary.chain_id(), secondary.chain_id());
    if a_chain? != deployment.chain_id || b_chain? != deployment.chain_id {
        return Err(WorkerError::Safety(
            "RPC chain ID does not match the registered deployment".to_owned(),
        ));
    }
    let (a_block, b_block) = tokio::join!(primary.finalized_block(), secondary.finalized_block());
    let a_block = a_block?;
    let b_block = b_block?;
    if a_block != b_block {
        return Err(WorkerError::Safety(
            "RPC providers disagree on the finalized checkpoint".to_owned(),
        ));
    }
    let common = a_block.number;
    let (a_code, b_code) = tokio::join!(
        primary.runtime_code(&deployment.vault_address, common),
        secondary.runtime_code(&deployment.vault_address, common)
    );
    let expected = deployment.runtime_code_hash.to_ascii_lowercase();
    for code in [a_code?, b_code?] {
        let actual = format!("0x{}", hex::encode(Keccak256::digest(code)));
        if actual != expected {
            return Err(WorkerError::Safety(
                "RPC runtime bytecode does not match the pinned deployment hash".to_owned(),
            ));
        }
    }
    Ok(())
}

fn ensure_agreement(
    a: &ProviderObservation,
    b: &ProviderObservation,
    candidate: &Candidate,
    deployment: &Deployment,
) -> Result<(), WorkerError> {
    if a.finalized != b.finalized
        || a.status != b.status
        || a.request_id_unused != b.request_id_unused
        || a.withdrawal_nonce_unused != b.withdrawal_nonce_unused
        || a.no_execution_event != b.no_execution_event
    {
        return Err(WorkerError::Safety(
            "independent RPC providers disagree on withdrawal evidence".to_owned(),
        ));
    }
    if a.provider_name.eq_ignore_ascii_case(&b.provider_name) {
        return Err(WorkerError::Safety(
            "withdrawal evidence requires two provider identities".to_owned(),
        ));
    }
    if a.finalized.number.saturating_add(1) < deployment.min_confirmations {
        return Err(WorkerError::Safety(
            "finalized evidence does not satisfy confirmation policy".to_owned(),
        ));
    }
    if !deployment
        .event_topic
        .eq_ignore_ascii_case(WITHDRAWAL_EVENT_TOPIC)
    {
        return Err(WorkerError::Safety(
            "registered withdrawal event topic does not match the omnibus V3 ABI".to_owned(),
        ));
    }
    match &a.status {
        TransactionStatus::Pending(tx)
        | TransactionStatus::FinalizedRevert(tx, _)
        | TransactionStatus::FinalizedExecution(tx, _) => {
            if !eq_hex(&tx.hash, &candidate.transaction_hash)
                || !eq_hex(&tx.sender, &candidate.sender)
                || tx.sender_nonce != candidate.sender_nonce
                || !eq_hex(&tx.destination, &deployment.vault_address)
                || !eq_hex(&tx.calldata_hash, &candidate.calldata_hash)
            {
                return Err(WorkerError::Safety(
                    "observed transaction does not match the current broadcast attempt".to_owned(),
                ));
            }
        }
        TransactionStatus::NotFound => {}
    }
    if let TransactionStatus::FinalizedExecution(_, log) = &a.status {
        let confirmations = a.finalized.number.saturating_sub(log.block.number) + 1;
        if confirmations < deployment.min_confirmations
            || log.request_id != candidate.withdrawal_id
            || !eq_hex(&log.destination, &candidate.destination)
            || !eq_hex(&log.asset, &candidate.asset)
            || log.amount != candidate.amount
            || log.nonce != candidate.nonce
            || log.deadline != candidate.expires_at.timestamp().to_string()
            || !eq_hex(&log.policy_hash, &candidate.policy_hash)
            || !eq_hex(&log.approver_hash, &candidate.approver_hash)
        {
            return Err(WorkerError::Safety(
                "finalized execution log does not match the authorization".to_owned(),
            ));
        }
    }
    Ok(())
}

fn eq_hex(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn is_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn replacement_id(candidate: &Candidate, replacement_hash: &str) -> Uuid {
    Uuid::new_v5(
        &candidate.withdrawal_id,
        format!(
            "replacement:{}:{}",
            candidate.transaction_hash.to_ascii_lowercase(),
            replacement_hash.to_ascii_lowercase()
        )
        .as_bytes(),
    )
}

#[derive(Clone)]
pub struct PgWorkerStore {
    pool: PgPool,
}

impl PgWorkerStore {
    pub async fn connect(database_url: &str) -> Result<Self, WorkerError> {
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
}

#[derive(FromRow)]
struct DeploymentRow {
    id: Uuid,
    chain_id: i64,
    vault_address: String,
    runtime_code_hash: String,
    execution_event_topic: String,
    policy_version: String,
    min_confirmations: i64,
    start_block: i64,
}

#[derive(FromRow)]
struct CandidateRow {
    withdrawal_id: Uuid,
    attempt_id: Uuid,
    transaction_hash: String,
    expires_at: DateTime<Utc>,
    destination: String,
    asset: String,
    amount: String,
    nonce: String,
    policy_hash: Vec<u8>,
    approver_hash: Vec<u8>,
    sender: String,
    sender_nonce: String,
    calldata_hash: Vec<u8>,
    updated_at: DateTime<Utc>,
    current_attempt_terminal: bool,
}

#[async_trait]
impl WorkerStore for PgWorkerStore {
    async fn deployment(&self, id: Uuid) -> Result<Deployment, WorkerError> {
        let row = sqlx::query_as::<_, DeploymentRow>(
            r#"
            SELECT deployment.id, deployment.chain_id, deployment.vault_address,
                   deployment.runtime_code_hash, capability.execution_event_topic,
                   policy.policy_version, policy.min_confirmations, deployment.start_block
            FROM ledger_chain_deployments deployment
            JOIN ledger_withdrawal_contract_capabilities capability ON capability.deployment_id = deployment.id
            JOIN LATERAL (
                SELECT policy_version, min_confirmations
                FROM ledger_withdrawal_confirmation_policies
                WHERE deployment_id = deployment.id AND effective_at <= transaction_timestamp()
                ORDER BY effective_at DESC LIMIT 1
            ) policy ON TRUE
            WHERE deployment.id = $1 AND deployment.enabled
              AND capability.authorization_version = 'eip712_v3'
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| WorkerError::Config("enabled registered omnibus V3 deployment and policy required".to_owned()))?;
        Ok(Deployment {
            id: row.id,
            chain_id: u64::try_from(row.chain_id)
                .map_err(|_| WorkerError::Config("invalid chain ID".to_owned()))?,
            vault_address: row.vault_address,
            runtime_code_hash: row.runtime_code_hash,
            event_topic: row.execution_event_topic,
            start_block: u64::try_from(row.start_block)
                .map_err(|_| WorkerError::Config("invalid deployment start block".to_owned()))?,
            policy_version: row.policy_version,
            min_confirmations: u64::try_from(row.min_confirmations)
                .map_err(|_| WorkerError::Config("invalid confirmation policy".to_owned()))?,
        })
    }

    async fn acquire_lease(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        duration: Duration,
    ) -> Result<(), WorkerError> {
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| WorkerError::Config("lease duration too large".to_owned()))?;
        let acquired = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO ledger_withdrawal_worker_checkpoints (deployment_id, lease_owner, lease_expires_at)
            VALUES ($1, $2, transaction_timestamp() + ($3 * interval '1 second'))
            ON CONFLICT (deployment_id) DO UPDATE SET
                lease_owner = EXCLUDED.lease_owner,
                lease_expires_at = EXCLUDED.lease_expires_at,
                updated_at = transaction_timestamp()
            WHERE ledger_withdrawal_worker_checkpoints.lease_owner IS NULL
               OR ledger_withdrawal_worker_checkpoints.lease_owner = EXCLUDED.lease_owner
               OR ledger_withdrawal_worker_checkpoints.lease_expires_at <= transaction_timestamp()
            RETURNING lease_owner
            "#,
        )
        .bind(deployment_id).bind(owner).bind(seconds)
        .fetch_optional(&self.pool).await?;
        if acquired != Some(owner) {
            return Err(WorkerError::LeaseUnavailable);
        }
        Ok(())
    }

    async fn record_revert(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        let TransactionStatus::FinalizedRevert(_, inclusion) = &observation.status else {
            return Err(WorkerError::Safety("revert evidence required".to_owned()));
        };
        let confirmations = observation
            .finalized
            .number
            .saturating_sub(inclusion.number)
            + 1;
        Ledger::from_pool(self.pool.clone())
            .record_finalized_withdrawal_revert(&FinalizedWithdrawalRevert {
                observation_id: evidence_id(candidate, "revert"),
                withdrawal_id: candidate.withdrawal_id,
                deployment_id: deployment.id,
                transaction_hash: candidate.transaction_hash.clone(),
                block_number: i64::try_from(inclusion.number)
                    .map_err(|_| WorkerError::Safety("block overflow".to_owned()))?,
                block_hash: inclusion.hash.clone(),
                block_timestamp: inclusion.timestamp,
                confirmations: i64::try_from(confirmations)
                    .map_err(|_| WorkerError::Safety("confirmation overflow".to_owned()))?,
                primary_provider: observation.provider_name.clone(),
                secondary_provider: secondary.provider_name.clone(),
                confirmation_policy_version: deployment.policy_version.clone(),
                evidence: evidence_json(observation, secondary),
            })
            .await?;
        Ok(())
    }

    async fn record_replacement(
        &self,
        _deployment: &Deployment,
        current: &Candidate,
        replacement: &Candidate,
        submitted_at: DateTime<Utc>,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        let sender_nonce = current
            .sender_nonce
            .parse::<u64>()
            .map_err(|_| WorkerError::Safety("sender nonce exceeds supported range".to_owned()))?;
        let calldata_hash: [u8; 32] = hex::decode(
            current
                .calldata_hash
                .strip_prefix("0x")
                .ok_or_else(|| WorkerError::Safety("calldata hash is malformed".to_owned()))?,
        )
        .map_err(|_| WorkerError::Safety("calldata hash is malformed".to_owned()))?
        .try_into()
        .map_err(|_| WorkerError::Safety("calldata hash is malformed".to_owned()))?;
        Ledger::from_pool(self.pool.clone())
            .record_observed_withdrawal_replacement(&RecordObservedWithdrawalReplacement {
                attempt_id: replacement.attempt_id,
                withdrawal_id: current.withdrawal_id,
                transaction_hash: replacement.transaction_hash.clone(),
                sender_address: current.sender.clone(),
                sender_nonce,
                calldata_hash,
                replacement_of: current.attempt_id,
                primary_provider: observation.provider_name.clone(),
                primary_reference: observation.provider_reference.clone(),
                secondary_provider: secondary.provider_name.clone(),
                secondary_reference: secondary.provider_reference.clone(),
                evidence: evidence_json(observation, secondary),
                submitted_at,
                observed_at: Utc::now(),
            })
            .await?;
        Ok(())
    }

    async fn release_lease(&self, deployment_id: Uuid, owner: Uuid) -> Result<(), WorkerError> {
        sqlx::query("UPDATE ledger_withdrawal_worker_checkpoints SET lease_owner = NULL, lease_expires_at = NULL, updated_at = transaction_timestamp() WHERE deployment_id = $1 AND lease_owner = $2")
            .bind(deployment_id).bind(owner).execute(&self.pool).await?;
        Ok(())
    }

    async fn candidates(
        &self,
        deployment: &Deployment,
        owner: Uuid,
        limit: u32,
    ) -> Result<Vec<Candidate>, WorkerError> {
        let select = r#"
            SELECT withdrawal.id AS withdrawal_id, attempt.id AS attempt_id, attempt.transaction_hash,
                   authorization.expires_at, withdrawal.destination_address AS destination,
                   authorization.asset_address AS asset, authorization.amount::TEXT AS amount,
                   authorization.nonce::TEXT AS nonce, attempt.sender_address AS sender,
                   attempt.sender_nonce::TEXT AS sender_nonce,
                   authorization.policy_hash, authorization.approver_hash, attempt.calldata_hash,
                   withdrawal.updated_at,
                   EXISTS (
                       SELECT 1 FROM ledger_withdrawal_chain_observations terminal
                       WHERE terminal.withdrawal_id = withdrawal.id
                         AND lower(terminal.transaction_hash) = lower(attempt.transaction_hash)
                         AND terminal.observation_type IN ('dropped', 'finalized_revert')
                   ) AS current_attempt_terminal
            FROM ledger_withdrawals withdrawal
            JOIN ledger_withdrawal_authorizations authorization ON authorization.withdrawal_id = withdrawal.id
            JOIN LATERAL (
                SELECT * FROM ledger_withdrawal_broadcast_attempts
                WHERE withdrawal_id = withdrawal.id ORDER BY attempt_number DESC LIMIT 1
            ) attempt ON TRUE
            WHERE withdrawal.chain_id = $1 AND lower(withdrawal.vault_address) = lower($2)
              AND withdrawal.state IN ('broadcast', 'replaced', 'dropped')
              AND EXISTS (
                  SELECT 1 FROM ledger_withdrawal_worker_checkpoints checkpoint
                  WHERE checkpoint.deployment_id = $3 AND checkpoint.lease_owner = $4
                    AND checkpoint.lease_expires_at > transaction_timestamp()
              )
              AND ($5::TIMESTAMPTZ IS NULL OR (withdrawal.updated_at, withdrawal.id) > ($5, $6))
            ORDER BY withdrawal.updated_at, withdrawal.id LIMIT $7
        "#;
        let checkpoint = sqlx::query_as::<_, (Option<DateTime<Utc>>, Option<Uuid>)>(
            "SELECT last_withdrawal_updated_at, last_withdrawal_id FROM ledger_withdrawal_worker_checkpoints WHERE deployment_id = $1 AND lease_owner = $2 AND lease_expires_at > transaction_timestamp()",
        ).bind(deployment.id).bind(owner).fetch_optional(&self.pool).await?.ok_or(WorkerError::LeaseUnavailable)?;
        let fetch = |cursor_time: Option<DateTime<Utc>>, cursor_id: Option<Uuid>| {
            sqlx::query_as::<_, CandidateRow>(select)
                .bind(i64::try_from(deployment.chain_id).unwrap_or(i64::MAX))
                .bind(&deployment.vault_address)
                .bind(deployment.id)
                .bind(owner)
                .bind(cursor_time)
                .bind(cursor_id)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
        };
        let mut rows = fetch(checkpoint.0, checkpoint.1).await?;
        if rows.is_empty() && checkpoint.0.is_some() {
            rows = fetch(None, None).await?;
        }
        rows.into_iter()
            .map(|r| {
                if r.calldata_hash.len() != 32
                    || r.policy_hash.len() != 32
                    || r.approver_hash.len() != 32
                {
                    return Err(WorkerError::Safety(
                        "stored authorization hash is malformed".to_owned(),
                    ));
                }
                Ok(Candidate {
                    withdrawal_id: r.withdrawal_id,
                    attempt_id: r.attempt_id,
                    transaction_hash: r.transaction_hash,
                    expires_at: r.expires_at,
                    destination: r.destination,
                    asset: r.asset,
                    amount: r.amount,
                    nonce: r.nonce,
                    policy_hash: format!("0x{}", hex::encode(r.policy_hash)),
                    approver_hash: format!("0x{}", hex::encode(r.approver_hash)),
                    sender: r.sender,
                    sender_nonce: r.sender_nonce,
                    calldata_hash: format!("0x{}", hex::encode(r.calldata_hash)),
                    updated_at: r.updated_at,
                    current_attempt_terminal: r.current_attempt_terminal,
                })
            })
            .collect()
    }

    async fn all_attempts_terminal(&self, withdrawal_id: Uuid) -> Result<bool, WorkerError> {
        let value = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS (SELECT 1 FROM ledger_withdrawal_broadcast_attempts WHERE withdrawal_id = $1)
               AND NOT EXISTS (
                   SELECT 1 FROM ledger_withdrawal_broadcast_attempts attempt
                   WHERE attempt.withdrawal_id = $1
                     AND NOT EXISTS (
                         SELECT 1 FROM ledger_withdrawal_broadcast_attempts replacement
                         WHERE replacement.replacement_of = attempt.id
                     )
                     AND NOT EXISTS (
                         SELECT 1 FROM ledger_withdrawal_chain_observations observation
                         WHERE observation.withdrawal_id = $1
                           AND observation.observation_type IN ('dropped', 'finalized_revert')
                           AND lower(observation.transaction_hash) = lower(attempt.transaction_hash)
                     )
               )"#,
        )
        .bind(withdrawal_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(value)
    }

    async fn record_execution(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        let TransactionStatus::FinalizedExecution(_, log) = &observation.status else {
            return Err(WorkerError::Safety(
                "execution evidence required".to_owned(),
            ));
        };
        let confirmations = observation
            .finalized
            .number
            .saturating_sub(log.block.number)
            + 1;
        Ledger::from_pool(self.pool.clone())
            .confirm_withdrawal_execution(&FinalizedWithdrawalExecution {
                observation_id: evidence_id(candidate, "execution"),
                completion_transaction_id: evidence_id(candidate, "completion"),
                withdrawal_id: candidate.withdrawal_id,
                deployment_id: deployment.id,
                transaction_hash: candidate.transaction_hash.clone(),
                block_number: i64::try_from(log.block.number)
                    .map_err(|_| WorkerError::Safety("block overflow".to_owned()))?,
                block_hash: log.block.hash.clone(),
                block_timestamp: log.block.timestamp,
                transaction_index: i32::try_from(log.transaction_index)
                    .map_err(|_| WorkerError::Safety("transaction index overflow".to_owned()))?,
                log_index: i32::try_from(log.log_index)
                    .map_err(|_| WorkerError::Safety("log index overflow".to_owned()))?,
                confirmations: i64::try_from(confirmations)
                    .map_err(|_| WorkerError::Safety("confirmation overflow".to_owned()))?,
                primary_provider: observation.provider_name.clone(),
                secondary_provider: secondary.provider_name.clone(),
                confirmation_policy_version: deployment.policy_version.clone(),
                evidence: evidence_json(observation, secondary),
            })
            .await?;
        Ok(())
    }

    async fn record_dropped(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        Ledger::from_pool(self.pool.clone())
            .record_dropped_withdrawal_attempt(&RecordDroppedWithdrawalAttempt {
                observation_id: evidence_id(candidate, "dropped"),
                withdrawal_id: candidate.withdrawal_id,
                deployment_id: deployment.id,
                transaction_hash: candidate.transaction_hash.clone(),
                primary_provider: observation.provider_name.clone(),
                secondary_provider: secondary.provider_name.clone(),
                evidence: evidence_json(observation, secondary),
            })
            .await?;
        Ok(())
    }

    async fn release_permanent_failure(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        observation: &ProviderObservation,
        secondary: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        Ledger::from_pool(self.pool.clone())
            .release_expired_withdrawal(&FinalizedWithdrawalNonExecution {
                observation_id: evidence_id(candidate, "non-execution"),
                release_transaction_id: evidence_id(candidate, "release"),
                withdrawal_id: candidate.withdrawal_id,
                deployment_id: deployment.id,
                last_transaction_hash: Some(candidate.transaction_hash.clone()),
                block_number: i64::try_from(observation.finalized.number)
                    .map_err(|_| WorkerError::Safety("block overflow".to_owned()))?,
                block_hash: observation.finalized.hash.clone(),
                block_timestamp: observation.finalized.timestamp,
                confirmations: i64::try_from(deployment.min_confirmations)
                    .map_err(|_| WorkerError::Safety("confirmation overflow".to_owned()))?,
                primary_provider: observation.provider_name.clone(),
                secondary_provider: secondary.provider_name.clone(),
                confirmation_policy_version: deployment.policy_version.clone(),
                request_id_unused: true,
                withdrawal_nonce_unused: true,
                no_execution_event: true,
                all_attempts_terminal: true,
                evidence: evidence_json(observation, secondary),
            })
            .await?;
        Ok(())
    }

    async fn checkpoint(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        last: Option<&Candidate>,
        duration: Duration,
    ) -> Result<(), WorkerError> {
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| WorkerError::Config("lease duration too large".to_owned()))?;
        let result = sqlx::query(
            "UPDATE ledger_withdrawal_worker_checkpoints SET scan_cycle = scan_cycle + 1, last_withdrawal_id = $3, last_withdrawal_updated_at = $4, last_completed_at = transaction_timestamp(), lease_expires_at = transaction_timestamp() + ($5 * interval '1 second'), updated_at = transaction_timestamp() WHERE deployment_id = $1 AND lease_owner = $2 AND lease_expires_at > transaction_timestamp()"
        ).bind(deployment_id).bind(owner).bind(last.map(|candidate| candidate.withdrawal_id)).bind(last.map(|candidate| candidate.updated_at)).bind(seconds).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(WorkerError::LeaseUnavailable);
        }
        Ok(())
    }
}

fn evidence_id(candidate: &Candidate, kind: &str) -> Uuid {
    Uuid::new_v5(
        &candidate.withdrawal_id,
        format!("{kind}:{}", candidate.transaction_hash).as_bytes(),
    )
}

fn evidence_json(observation: &ProviderObservation, secondary: &ProviderObservation) -> Value {
    json!({
        "primary_provider": observation.provider_name,
        "primary_reference": observation.provider_reference,
        "primary_status": status_evidence(&observation.status),
        "secondary_provider": secondary.provider_name,
        "secondary_reference": secondary.provider_reference,
        "secondary_status": status_evidence(&secondary.status),
        "finalized_block": observation.finalized.number,
        "finalized_hash": observation.finalized.hash,
        "request_id_unused": observation.request_id_unused,
        "withdrawal_nonce_unused": observation.withdrawal_nonce_unused,
        "no_execution_event": observation.no_execution_event,
    })
}

fn status_evidence(status: &TransactionStatus) -> Value {
    let transaction = |facts: &TransactionFacts| {
        json!({
            "hash": facts.hash,
            "sender": facts.sender,
            "sender_nonce": facts.sender_nonce,
            "destination": facts.destination,
            "calldata_hash": facts.calldata_hash,
        })
    };
    match status {
        TransactionStatus::NotFound => json!({"kind":"not_found"}),
        TransactionStatus::Pending(facts) => {
            json!({"kind":"pending", "transaction":transaction(facts)})
        }
        TransactionStatus::FinalizedRevert(facts, block) => json!({
            "kind":"finalized_revert",
            "receipt_status":0,
            "transaction":transaction(facts),
            "inclusion_block":{"number":block.number,"hash":block.hash,"timestamp":block.timestamp},
        }),
        TransactionStatus::FinalizedExecution(facts, log) => json!({
            "kind":"finalized_execution",
            "receipt_status":1,
            "transaction":transaction(facts),
            "inclusion_block":{"number":log.block.number,"hash":log.block.hash,"timestamp":log.block.timestamp},
            "transaction_index":log.transaction_index,
            "log_index":log.log_index,
            "request_id":log.request_id,
            "destination":log.destination,
            "asset":log.asset,
            "amount":log.amount,
            "nonce":log.nonce,
            "deadline":log.deadline,
            "policy_hash":log.policy_hash,
            "approver_hash":log.approver_hash,
        }),
    }
}

/// Broadcast boundary. Production defaults to this implementation and therefore
/// cannot hold a relayer key or submit a transaction.
#[derive(Default)]
pub struct DisabledBroadcaster;

impl DisabledBroadcaster {
    pub fn broadcast(&self, _signed_transaction: &[u8]) -> Result<String, WorkerError> {
        Err(WorkerError::Disabled)
    }
}

#[derive(Clone)]
pub struct HttpRpc {
    name: String,
    endpoint: Url,
    client: reqwest::Client,
}

impl HttpRpc {
    pub fn new(name: String, endpoint: Url, timeout: Duration) -> Result<Self, WorkerError> {
        if name.trim().is_empty() || timeout.is_zero() {
            return Err(WorkerError::Config(
                "RPC identity and bounded timeout are required".to_owned(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| {
                WorkerError::Config("could not construct bounded RPC client".to_owned())
            })?;
        Ok(Self {
            name,
            endpoint,
            client,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, WorkerError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| self.failure("request failed"))?;
        if !response.status().is_success() {
            return Err(self.failure("HTTP status was not successful"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| self.failure("response was not valid JSON"))?;
        if body.get("error").is_some() {
            return Err(self.failure("JSON-RPC returned an error"));
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| self.failure("JSON-RPC result was missing"))
    }

    fn failure(&self, message: &str) -> WorkerError {
        WorkerError::Provider {
            provider: self.name.clone(),
            message: message.to_owned(),
        }
    }

    async fn block(&self, tag: &str) -> Result<FinalizedBlock, WorkerError> {
        let value = self
            .call("eth_getBlockByNumber", json!([tag, false]))
            .await?;
        if value.is_null() {
            return Err(self.failure("block was not found"));
        }
        let number = parse_hex_u64(field(&value, "number", self)?, self)?;
        let hash = normalized_hash(field(&value, "hash", self)?, self)?;
        let timestamp = parse_hex_u64(field(&value, "timestamp", self)?, self)?;
        let timestamp = DateTime::from_timestamp(
            i64::try_from(timestamp).map_err(|_| self.failure("block timestamp overflow"))?,
            0,
        )
        .ok_or_else(|| self.failure("block timestamp was invalid"))?;
        Ok(FinalizedBlock {
            number,
            hash,
            timestamp,
        })
    }

    async fn bool_call(&self, to: &str, data: String, block: u64) -> Result<bool, WorkerError> {
        let value = self
            .call(
                "eth_call",
                json!([{"to":to,"data":data}, format!("0x{block:x}")]),
            )
            .await?;
        let raw = value
            .as_str()
            .ok_or_else(|| self.failure("eth_call result was not hex"))?;
        let bytes = decode_hex(raw, self)?;
        if bytes.len() != 32 {
            return Err(self.failure("eth_call boolean was malformed"));
        }
        Ok(bytes.iter().any(|byte| *byte != 0))
    }

    async fn non_execution_proof(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        finalized: &FinalizedBlock,
    ) -> Result<(bool, bool, bool), WorkerError> {
        if finalized.timestamp < candidate.expires_at {
            return Ok((false, false, false));
        }
        let mut request_data = selector("usedWithdrawalRequestIds(bytes16)").to_vec();
        request_data.extend_from_slice(candidate.withdrawal_id.as_bytes());
        request_data.extend_from_slice(&[0; 16]);
        let request_used = self
            .bool_call(
                &deployment.vault_address,
                format!("0x{}", hex::encode(request_data)),
                finalized.number,
            )
            .await?;

        let nonce_data = used_withdraw_nonce_calldata(&candidate.nonce, self)?;
        let nonce_used = self
            .bool_call(
                &deployment.vault_address,
                format!("0x{}", hex::encode(nonce_data)),
                finalized.number,
            )
            .await?;

        let request_topic = format!(
            "0x{}{}",
            hex::encode(candidate.withdrawal_id.as_bytes()),
            "00".repeat(16)
        );
        let logs = self
            .call(
                "eth_getLogs",
                json!([{
                    "address": deployment.vault_address,
                    "fromBlock": format!("0x{:x}", deployment.start_block),
                    "toBlock": format!("0x{:x}", finalized.number),
                    "topics": [deployment.event_topic, request_topic]
                }]),
            )
            .await?;
        let no_event = logs
            .as_array()
            .map(|logs| logs.is_empty())
            .ok_or_else(|| self.failure("log proof was malformed"))?;
        Ok((!request_used, !nonce_used, no_event))
    }

    async fn transaction(
        &self,
        hash: &str,
    ) -> Result<Option<(TransactionFacts, Value)>, WorkerError> {
        let value = self.call("eth_getTransactionByHash", json!([hash])).await?;
        if value.is_null() {
            return Ok(None);
        }
        let input = field(&value, "input", self)?;
        let calldata = decode_hex(input, self)?;
        let facts = TransactionFacts {
            hash: normalized_hash(field(&value, "hash", self)?, self)?,
            sender: normalized_address(field(&value, "from", self)?, self)?,
            sender_nonce: parse_hex_u64(field(&value, "nonce", self)?, self)?.to_string(),
            destination: normalized_address(field(&value, "to", self)?, self)?,
            calldata_hash: format!("0x{}", hex::encode(Keccak256::digest(calldata))),
        };
        Ok(Some((facts, value)))
    }

    async fn receipt_status(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
        facts: TransactionFacts,
        finalized: &FinalizedBlock,
    ) -> Result<TransactionStatus, WorkerError> {
        let receipt = self
            .call(
                "eth_getTransactionReceipt",
                json!([candidate.transaction_hash]),
            )
            .await?;
        if receipt.is_null() {
            return Ok(TransactionStatus::Pending(facts));
        }
        let receipt_transaction_hash =
            normalized_hash(field(&receipt, "transactionHash", self)?, self)?;
        let receipt_sender = normalized_address(field(&receipt, "from", self)?, self)?;
        let receipt_destination = normalized_address(field(&receipt, "to", self)?, self)?;
        if !eq_hex(&receipt_transaction_hash, &candidate.transaction_hash)
            || !eq_hex(&receipt_sender, &facts.sender)
            || !eq_hex(&receipt_destination, &facts.destination)
        {
            return Err(self.failure("receipt identity did not match the observed transaction"));
        }
        let number = parse_hex_u64(field(&receipt, "blockNumber", self)?, self)?;
        if number > finalized.number {
            return Ok(TransactionStatus::Pending(facts));
        }
        let block = self.block(&format!("0x{number:x}")).await?;
        let receipt_hash = normalized_hash(field(&receipt, "blockHash", self)?, self)?;
        if receipt_hash != block.hash {
            return Err(self.failure("receipt block hash was not canonical"));
        }
        let status = parse_hex_u64(field(&receipt, "status", self)?, self)?;
        if status > 1 {
            return Err(self.failure("receipt status was not a canonical boolean"));
        }
        if status == 0 {
            return Ok(TransactionStatus::FinalizedRevert(facts, block));
        }
        let logs = receipt
            .get("logs")
            .and_then(Value::as_array)
            .ok_or_else(|| self.failure("receipt logs were malformed"))?;
        let matching: Vec<&Value> = logs
            .iter()
            .filter(|log| {
                log.get("address")
                    .and_then(Value::as_str)
                    .is_some_and(|address| address.eq_ignore_ascii_case(&deployment.vault_address))
                    && log
                        .get("topics")
                        .and_then(Value::as_array)
                        .and_then(|topics| topics.first())
                        .and_then(Value::as_str)
                        .is_some_and(|topic| topic.eq_ignore_ascii_case(&deployment.event_topic))
            })
            .collect();
        if matching.len() != 1 {
            return Err(
                self.failure("receipt did not contain exactly one withdrawal execution log")
            );
        }
        let matching = matching[0];
        let log_transaction_hash =
            normalized_hash(field(matching, "transactionHash", self)?, self)?;
        let log_block_hash = normalized_hash(field(matching, "blockHash", self)?, self)?;
        let log_block_number = parse_hex_u64(field(matching, "blockNumber", self)?, self)?;
        let removed = matching
            .get("removed")
            .and_then(Value::as_bool)
            .ok_or_else(|| self.failure("execution log removal flag was malformed"))?;
        if removed
            || !eq_hex(&log_transaction_hash, &candidate.transaction_hash)
            || !eq_hex(&log_block_hash, &block.hash)
            || log_block_number != block.number
        {
            return Err(self.failure("execution log identity was not canonical"));
        }
        let log = parse_execution_log(matching, block, self)?;
        Ok(TransactionStatus::FinalizedExecution(facts, Box::new(log)))
    }
}

#[async_trait]
impl ChainProvider for HttpRpc {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chain_id(&self) -> Result<u64, WorkerError> {
        let value = self.call("eth_chainId", json!([])).await?;
        parse_hex_u64(
            value
                .as_str()
                .ok_or_else(|| self.failure("chain ID was not hex"))?,
            self,
        )
    }

    async fn finalized_block(&self) -> Result<FinalizedBlock, WorkerError> {
        self.block("finalized").await
    }

    async fn runtime_code(&self, address: &str, block: u64) -> Result<Vec<u8>, WorkerError> {
        let value = self
            .call("eth_getCode", json!([address, format!("0x{block:x}")]))
            .await?;
        decode_hex(
            value
                .as_str()
                .ok_or_else(|| self.failure("runtime bytecode was not hex"))?,
            self,
        )
    }

    async fn observe(
        &self,
        deployment: &Deployment,
        candidate: &Candidate,
    ) -> Result<ProviderObservation, WorkerError> {
        let finalized = self.finalized_block().await?;
        let status = match self.transaction(&candidate.transaction_hash).await? {
            None => TransactionStatus::NotFound,
            Some((facts, _)) => {
                self.receipt_status(deployment, candidate, facts, &finalized)
                    .await?
            }
        };
        let (request_id_unused, withdrawal_nonce_unused, no_execution_event) = self
            .non_execution_proof(deployment, candidate, &finalized)
            .await?;
        Ok(ProviderObservation {
            finalized,
            status,
            request_id_unused,
            withdrawal_nonce_unused,
            no_execution_event,
            provider_name: self.name.clone(),
            provider_reference: self.name.clone(),
        })
    }
}

fn field<'a>(value: &'a Value, name: &str, rpc: &HttpRpc) -> Result<&'a str, WorkerError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| rpc.failure("JSON-RPC field was missing or malformed"))
}

fn decode_hex(value: &str, rpc: &HttpRpc) -> Result<Vec<u8>, WorkerError> {
    let raw = value
        .strip_prefix("0x")
        .ok_or_else(|| rpc.failure("hex value lacked prefix"))?;
    hex::decode(raw).map_err(|_| rpc.failure("hex value was malformed"))
}

fn parse_hex_u64(value: &str, rpc: &HttpRpc) -> Result<u64, WorkerError> {
    u64::from_str_radix(
        value
            .strip_prefix("0x")
            .ok_or_else(|| rpc.failure("quantity lacked hex prefix"))?,
        16,
    )
    .map_err(|_| rpc.failure("hex quantity was malformed"))
}

fn normalized_hash(value: &str, rpc: &HttpRpc) -> Result<String, WorkerError> {
    if value.len() != 66 || decode_hex(value, rpc)?.len() != 32 {
        return Err(rpc.failure("hash was malformed"));
    }
    Ok(value.to_ascii_lowercase())
}

fn normalized_address(value: &str, rpc: &HttpRpc) -> Result<String, WorkerError> {
    if value.len() != 42 || decode_hex(value, rpc)?.len() != 20 {
        return Err(rpc.failure("address was malformed"));
    }
    Ok(value.to_ascii_lowercase())
}

fn selector(signature: &str) -> [u8; 4] {
    Keccak256::digest(signature.as_bytes())[..4]
        .try_into()
        .expect("four byte selector")
}

fn decimal_word(value: &str, rpc: &HttpRpc) -> Result<[u8; 32], WorkerError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(rpc.failure("nonce proof input was malformed"));
    }
    let mut out = [0u8; 32];
    for digit in value.bytes().map(|byte| byte - b'0') {
        let mut carry = digit as u16;
        for byte in out.iter_mut().rev() {
            let next = (*byte as u16) * 10 + carry;
            *byte = next as u8;
            carry = next >> 8;
        }
        if carry != 0 {
            return Err(rpc.failure("nonce exceeded uint256"));
        }
    }
    Ok(out)
}

fn used_withdraw_nonce_calldata(nonce: &str, rpc: &HttpRpc) -> Result<Vec<u8>, WorkerError> {
    let mut data = selector("usedWithdrawNonces(uint256)").to_vec();
    data.extend_from_slice(&decimal_word(nonce, rpc)?);
    Ok(data)
}

fn parse_execution_log(
    value: &Value,
    block: FinalizedBlock,
    rpc: &HttpRpc,
) -> Result<ExecutionLog, WorkerError> {
    let topics = value
        .get("topics")
        .and_then(Value::as_array)
        .ok_or_else(|| rpc.failure("execution topics were malformed"))?;
    if topics.len() != 4 {
        return Err(rpc.failure("execution topics were incomplete"));
    }
    let request = decode_hex(
        topics[1]
            .as_str()
            .ok_or_else(|| rpc.failure("request topic was malformed"))?,
        rpc,
    )?;
    if request.len() != 32 || request[16..].iter().any(|byte| *byte != 0) {
        return Err(rpc.failure("request ID topic was malformed"));
    }
    let request_id =
        Uuid::from_slice(&request[..16]).map_err(|_| rpc.failure("request ID was malformed"))?;
    let topic_address = |topic: &Value| -> Result<String, WorkerError> {
        let bytes = decode_hex(
            topic
                .as_str()
                .ok_or_else(|| rpc.failure("address topic was malformed"))?,
            rpc,
        )?;
        if bytes.len() != 32 || bytes[..12].iter().any(|byte| *byte != 0) {
            return Err(rpc.failure("address topic was malformed"));
        }
        Ok(format!("0x{}", hex::encode(&bytes[12..])))
    };
    let data = decode_hex(field(value, "data", rpc)?, rpc)?;
    if data.len() != 32 * 5 {
        return Err(rpc.failure("execution data was malformed"));
    }
    let word = |index: usize| &data[index * 32..(index + 1) * 32];
    Ok(ExecutionLog {
        block,
        transaction_index: u32::try_from(parse_hex_u64(
            field(value, "transactionIndex", rpc)?,
            rpc,
        )?)
        .map_err(|_| rpc.failure("transaction index overflow"))?,
        log_index: u32::try_from(parse_hex_u64(field(value, "logIndex", rpc)?, rpc)?)
            .map_err(|_| rpc.failure("log index overflow"))?,
        request_id,
        destination: topic_address(&topics[2])?,
        asset: topic_address(&topics[3])?,
        amount: uint_word_decimal(word(0)),
        nonce: uint_word_decimal(word(1)),
        deadline: uint_word_decimal(word(2)),
        policy_hash: format!("0x{}", hex::encode(word(3))),
        approver_hash: format!("0x{}", hex::encode(word(4))),
    })
}

fn uint_word_decimal(word: &[u8]) -> String {
    let mut digits = vec![0u8];
    for byte in word {
        let mut carry = *byte as u16;
        for digit in digits.iter_mut().rev() {
            let next = (*digit as u16) * 256 + carry;
            *digit = (next % 10) as u8;
            carry = next / 10;
        }
        while carry > 0 {
            digits.insert(0, (carry % 10) as u8);
            carry /= 10;
        }
    }
    digits
        .into_iter()
        .map(|digit| char::from(b'0' + digit))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(value: u64) -> [u8; 32] {
        let mut result = [0_u8; 32];
        result[24..].copy_from_slice(&value.to_be_bytes());
        result
    }

    #[test]
    fn execution_log_decoder_preserves_every_authorized_field() {
        let rpc = HttpRpc::new(
            "decoder-test".to_owned(),
            Url::parse("http://127.0.0.1:8545").unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
        let request_id = Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let mut request_topic = [0_u8; 32];
        request_topic[..16].copy_from_slice(request_id.as_bytes());
        let address_topic = |byte: u8| {
            let mut topic = [0_u8; 32];
            topic[12..].fill(byte);
            format!("0x{}", hex::encode(topic))
        };
        let mut data = Vec::new();
        data.extend_from_slice(&word(1_000));
        data.extend_from_slice(&word(7));
        data.extend_from_slice(&word(2_000_000_000));
        data.extend_from_slice(&[0x66; 32]);
        data.extend_from_slice(&[0x77; 32]);
        let block = FinalizedBlock {
            number: 100,
            hash: format!("0x{}", "aa".repeat(32)),
            timestamp: DateTime::from_timestamp(2_000_000_100, 0).unwrap(),
        };
        let decoded = parse_execution_log(
            &json!({
                "topics": [
                    WITHDRAWAL_EVENT_TOPIC,
                    format!("0x{}", hex::encode(request_topic)),
                    address_topic(0x33),
                    address_topic(0x44)
                ],
                "data": format!("0x{}", hex::encode(data)),
                "transactionIndex": "0x2",
                "logIndex": "0x3"
            }),
            block,
            &rpc,
        )
        .unwrap();
        assert_eq!(decoded.request_id, request_id);
        assert_eq!(decoded.destination, format!("0x{}", "33".repeat(20)));
        assert_eq!(decoded.asset, format!("0x{}", "44".repeat(20)));
        assert_eq!(decoded.amount, "1000");
        assert_eq!(decoded.nonce, "7");
        assert_eq!(decoded.deadline, "2000000000");
        assert_eq!(decoded.policy_hash, format!("0x{}", "66".repeat(32)));
        assert_eq!(decoded.approver_hash, format!("0x{}", "77".repeat(32)));
        assert_eq!(decoded.transaction_index, 2);
        assert_eq!(decoded.log_index, 3);
    }

    #[test]
    fn execution_log_decoder_rejects_noncanonical_bytes16_topic() {
        let rpc = HttpRpc::new(
            "decoder-test".to_owned(),
            Url::parse("http://127.0.0.1:8545").unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
        let malformed = json!({
            "topics": [WITHDRAWAL_EVENT_TOPIC, format!("0x{}", "ff".repeat(32)), format!("0x{}", "00".repeat(32)), format!("0x{}", "00".repeat(32))],
            "data": format!("0x{}", "00".repeat(32 * 5)),
            "transactionIndex": "0x0",
            "logIndex": "0x0"
        });
        assert!(parse_execution_log(
            &malformed,
            FinalizedBlock {
                number: 1,
                hash: format!("0x{}", "aa".repeat(32)),
                timestamp: Utc::now(),
            },
            &rpc,
        )
        .is_err());
    }

    #[test]
    fn v3_event_topic_and_global_nonce_getter_match_the_contract_abi() {
        assert_eq!(
            format!(
                "0x{}",
                hex::encode(Keccak256::digest(WITHDRAWAL_EVENT_SIGNATURE.as_bytes()))
            ),
            WITHDRAWAL_EVENT_TOPIC
        );
        let rpc = HttpRpc::new(
            "decoder-test".to_owned(),
            Url::parse("http://127.0.0.1:8545").unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
        let calldata = used_withdraw_nonce_calldata("7", &rpc).unwrap();
        assert_eq!(&calldata[..4], &selector("usedWithdrawNonces(uint256)"));
        assert_eq!(&calldata[4..], &word(7));
    }
}

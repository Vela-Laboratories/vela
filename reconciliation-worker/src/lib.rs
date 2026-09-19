use async_trait::async_trait;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, TimeZone, Utc};
use ledger::{
    Ledger, ObservedCustodyBalance, ReconciliationAsset, ReconciliationDeployment,
    RecordReconciliationCycle, RecordReconciliationFailure, RecordedReconciliation,
};
use reqwest::Url;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use sha2::{Digest as ShaDigest, Sha256};
use sha3::Keccak256;
use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;
use tracing::{error, info, warn};
use uuid::Uuid;

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
    pub max_finalized_age: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, WorkerError> {
        if env::var("VELA_RECONCILIATION_ENABLED").ok().as_deref() != Some("true") {
            return Err(WorkerError::Disabled);
        }
        let primary_url = required_url("VELA_RECONCILIATION_RPC_PRIMARY_URL")?;
        let secondary_url = required_url("VELA_RECONCILIATION_RPC_SECONDARY_URL")?;
        let config = Self {
            enabled: true,
            database_url: required("DATABASE_URL")?,
            deployment_id: required("VELA_RECONCILIATION_DEPLOYMENT_ID")?
                .parse()
                .map_err(|_| WorkerError::Config("deployment ID must be a UUID".to_owned()))?,
            primary_name: required("VELA_RECONCILIATION_RPC_PRIMARY_NAME")?,
            primary_url,
            secondary_name: required("VELA_RECONCILIATION_RPC_SECONDARY_NAME")?,
            secondary_url,
            request_timeout: millis("VELA_RECONCILIATION_RPC_TIMEOUT_MS", 5_000, 60_000)?,
            poll_interval: millis("VELA_RECONCILIATION_POLL_MS", 30_000, 300_000)?,
            retry_max: millis("VELA_RECONCILIATION_RETRY_MAX_MS", 60_000, 600_000)?,
            lease_duration: seconds("VELA_RECONCILIATION_LEASE_SECONDS", 120, 900)?,
            max_finalized_age: seconds(
                "VELA_RECONCILIATION_MAX_FINALIZED_AGE_SECONDS",
                300,
                3_600,
            )?,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), WorkerError> {
        if !self.enabled {
            return Err(WorkerError::Disabled);
        }
        if self.deployment_id.is_nil()
            || origin(&self.primary_url) == origin(&self.secondary_url)
            || self.primary_name.eq_ignore_ascii_case(&self.secondary_name)
        {
            return Err(WorkerError::Config(
                "deployment and two distinct RPC origins/provider names are required".to_owned(),
            ));
        }
        let minimum = self.request_timeout.checked_mul(12).ok_or_else(|| {
            WorkerError::Config("RPC timeout is too large for lease safety".to_owned())
        })?;
        if self.lease_duration < minimum {
            return Err(WorkerError::Config(
                "lease must cover twelve bounded sequential RPC requests".to_owned(),
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
        .filter(|value| !value.trim().is_empty())
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
        .map(|value| value.parse::<u64>())
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
    #[error("reconciliation worker is disabled; set VELA_RECONCILIATION_ENABLED=true to opt in")]
    Disabled,
    #[error("invalid reconciliation configuration: {0}")]
    Config(String),
    #[error("reconciliation provider {provider} failed: {message}")]
    Provider { provider: String, message: String },
    #[error("reconciliation safety check failed ({reason}): {message}")]
    Safety {
        reason: &'static str,
        message: String,
        evidence: Value,
    },
    #[error("another process owns the reconciliation lease")]
    LeaseUnavailable,
    #[error(transparent)]
    Ledger(#[from] ledger::LedgerError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizedBlock {
    pub number: u64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
}

#[async_trait]
pub trait Rpc: Send + Sync {
    fn name(&self) -> &str;
    async fn chain_id(&self) -> Result<u64, WorkerError>;
    async fn finalized_block(&self) -> Result<FinalizedBlock, WorkerError>;
    async fn code(&self, address: &str, block: u64) -> Result<String, WorkerError>;
    async fn native_balance(&self, address: &str, block: u64) -> Result<BigDecimal, WorkerError>;
    async fn token_balance(
        &self,
        token: &str,
        address: &str,
        block: u64,
    ) -> Result<BigDecimal, WorkerError>;
}

pub struct HttpRpc {
    name: String,
    url: Url,
    client: reqwest::Client,
    request_id: AtomicU64,
}

impl HttpRpc {
    pub fn new(name: String, url: Url, timeout: Duration) -> Result<Self, WorkerError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| WorkerError::Config("could not construct RPC client".to_owned()))?;
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
    ) -> Result<T, WorkerError> {
        #[derive(Deserialize)]
        struct Response<T> {
            result: Option<T>,
            error: Option<RpcError>,
        }
        #[derive(Deserialize)]
        struct RpcError {
            code: i64,
        }
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| self.provider_error("request failed or timed out"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(self.provider_error(&format!("HTTP status {status}")));
        }
        let body: Response<T> = response
            .json()
            .await
            .map_err(|_| self.provider_error("response was not valid JSON-RPC"))?;
        if let Some(error) = body.error {
            return Err(self.provider_error(&format!("JSON-RPC error {}", error.code)));
        }
        body.result
            .ok_or_else(|| self.provider_error("JSON-RPC response omitted result"))
    }

    fn provider_error(&self, message: &str) -> WorkerError {
        WorkerError::Provider {
            provider: self.name.clone(),
            message: message.to_owned(),
        }
    }
}

#[derive(Deserialize)]
struct WireBlock {
    number: String,
    hash: String,
    timestamp: String,
}

#[async_trait]
impl Rpc for HttpRpc {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chain_id(&self) -> Result<u64, WorkerError> {
        let value: String = self.call("eth_chainId", json!([])).await?;
        parse_u64_quantity(&value)
    }

    async fn finalized_block(&self) -> Result<FinalizedBlock, WorkerError> {
        let block: WireBlock = self
            .call("eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        if !is_hash(&block.hash) {
            return Err(self.provider_error("finalized block hash was malformed"));
        }
        let timestamp = parse_u64_quantity(&block.timestamp)?;
        let timestamp = i64::try_from(timestamp)
            .map_err(|_| self.provider_error("finalized timestamp was out of range"))?;
        Ok(FinalizedBlock {
            number: parse_u64_quantity(&block.number)?,
            hash: block.hash.to_ascii_lowercase(),
            timestamp: Utc
                .timestamp_opt(timestamp, 0)
                .single()
                .ok_or_else(|| self.provider_error("finalized timestamp was out of range"))?,
        })
    }

    async fn code(&self, address: &str, block: u64) -> Result<String, WorkerError> {
        self.call("eth_getCode", json!([address, quantity(block)]))
            .await
    }

    async fn native_balance(&self, address: &str, block: u64) -> Result<BigDecimal, WorkerError> {
        let value: String = self
            .call("eth_getBalance", json!([address, quantity(block)]))
            .await?;
        parse_u256_quantity(&value)
    }

    async fn token_balance(
        &self,
        token: &str,
        address: &str,
        block: u64,
    ) -> Result<BigDecimal, WorkerError> {
        let raw = address
            .strip_prefix("0x")
            .ok_or_else(|| self.provider_error("vault address was malformed"))?;
        let data = format!("0x70a08231{:0>64}", raw);
        let value: String = self
            .call(
                "eth_call",
                json!([{"to":token,"data":data}, quantity(block)]),
            )
            .await?;
        parse_u256_quantity(&value)
    }
}

pub struct ReconciliationWorker<P, S> {
    config: Config,
    ledger: Ledger,
    primary: P,
    secondary: S,
    deployment: ReconciliationDeployment,
    assets: Vec<ReconciliationAsset>,
    owner: Uuid,
}

impl<P: Rpc, S: Rpc> ReconciliationWorker<P, S> {
    pub async fn new(
        config: Config,
        ledger: Ledger,
        primary: P,
        secondary: S,
    ) -> Result<Self, WorkerError> {
        config.validate()?;
        let (deployment, assets) = ledger
            .reconciliation_deployment(config.deployment_id)
            .await?;
        Ok(Self {
            config,
            ledger,
            primary,
            secondary,
            deployment,
            assets,
            owner: Uuid::new_v4(),
        })
    }

    pub async fn run_once(&self) -> Result<RecordedReconciliation, WorkerError> {
        if !self
            .ledger
            .acquire_reconciliation_lease(
                self.deployment.id,
                self.owner,
                self.config.lease_duration,
            )
            .await?
        {
            return Err(WorkerError::LeaseUnavailable);
        }
        match self.observe_and_record().await {
            Ok(result) => Ok(result),
            Err(error @ WorkerError::Safety { .. }) | Err(error @ WorkerError::Provider { .. }) => {
                self.persist_failure(&error).await?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn observe_and_record(&self) -> Result<RecordedReconciliation, WorkerError> {
        let (primary_chain, secondary_chain) =
            tokio::try_join!(self.primary.chain_id(), self.secondary.chain_id())?;
        let expected_chain = u64::try_from(self.deployment.chain_id)
            .map_err(|_| WorkerError::Config("registered chain ID is invalid".to_owned()))?;
        validate_chain_ids(expected_chain, primary_chain, secondary_chain)?;
        let (primary_block, secondary_block) = tokio::try_join!(
            self.primary.finalized_block(),
            self.secondary.finalized_block()
        )?;
        validate_finalized_blocks(
            &primary_block,
            &secondary_block,
            Utc::now(),
            self.config.max_finalized_age,
        )?;
        if let Some((previous_number, previous_hash)) = self
            .ledger
            .reconciliation_checkpoint(self.deployment.id)
            .await?
        {
            let current_number = i64::try_from(primary_block.number)
                .map_err(|_| WorkerError::Config("finalized block exceeds bigint".to_owned()))?;
            if current_number < previous_number
                || (current_number == previous_number
                    && !primary_block.hash.eq_ignore_ascii_case(&previous_hash))
            {
                return Err(safety(
                    "finalized_regression",
                    "agreed finalized block regressed or changed hash",
                    json!({
                        "previous_number": previous_number,
                        "previous_hash": previous_hash,
                        "current_number": current_number,
                        "current_hash": primary_block.hash,
                    }),
                ));
            }
        }
        let (primary_code, secondary_code) = tokio::try_join!(
            self.primary
                .code(&self.deployment.vault_address, primary_block.number),
            self.secondary
                .code(&self.deployment.vault_address, primary_block.number)
        )?;
        let primary_code_hash = code_hash(&primary_code)?;
        let secondary_code_hash = code_hash(&secondary_code)?;
        validate_code_hashes(
            &self.deployment.runtime_code_hash,
            &primary_code_hash,
            &secondary_code_hash,
            primary_block.number,
        )?;

        let mut balances = Vec::with_capacity(self.assets.len());
        for asset in &self.assets {
            if !self
                .ledger
                .acquire_reconciliation_lease(
                    self.deployment.id,
                    self.owner,
                    self.config.lease_duration,
                )
                .await?
            {
                return Err(safety(
                    "lease_lost",
                    "reconciliation lease was lost during a custody observation",
                    json!({"asset_id": asset.id, "asset_code": asset.code}),
                ));
            }
            let (primary_amount, secondary_amount) = if let Some(token) = &asset.contract_address {
                tokio::try_join!(
                    self.primary.token_balance(
                        token,
                        &self.deployment.vault_address,
                        primary_block.number
                    ),
                    self.secondary.token_balance(
                        token,
                        &self.deployment.vault_address,
                        primary_block.number
                    )
                )?
            } else {
                tokio::try_join!(
                    self.primary
                        .native_balance(&self.deployment.vault_address, primary_block.number),
                    self.secondary
                        .native_balance(&self.deployment.vault_address, primary_block.number)
                )?
            };
            if primary_amount != secondary_amount {
                return Err(safety(
                    "provider_disagreement",
                    "RPC providers disagree on finalized custody balance",
                    json!({
                        "asset_id": asset.id,
                        "asset_code": asset.code,
                        "block_number": primary_block.number,
                        "primary_amount": primary_amount.to_string(),
                        "secondary_amount": secondary_amount.to_string(),
                    }),
                ));
            }
            balances.push(ObservedCustodyBalance {
                asset_id: asset.id,
                amount: primary_amount,
            });
        }
        let evidence = json!({
            "chain_id": expected_chain,
            "block_number": primary_block.number,
            "block_hash": primary_block.hash,
            "block_timestamp": primary_block.timestamp,
            "vault_address": self.deployment.vault_address,
            "runtime_code_hash": primary_code_hash,
            "primary_provider": self.primary.name(),
            "secondary_provider": self.secondary.name(),
            "balances": balances,
        });
        let input_hash: [u8; 32] = Sha256::digest(serde_json::to_vec(&evidence).map_err(|_| {
            WorkerError::Config("could not encode reconciliation evidence".to_owned())
        })?)
        .into();
        self.ledger
            .record_reconciliation_cycle(&RecordReconciliationCycle {
                deployment_id: self.deployment.id,
                lease_owner: self.owner,
                chain_id: self.deployment.chain_id,
                block_number: i64::try_from(primary_block.number).map_err(|_| {
                    WorkerError::Config("finalized block exceeds bigint".to_owned())
                })?,
                block_hash: primary_block.hash,
                block_timestamp: primary_block.timestamp,
                vault_address: self.deployment.vault_address.clone(),
                deployment_version: self.deployment.deployment_version.clone(),
                primary_provider: self.primary.name().to_owned(),
                secondary_provider: self.secondary.name().to_owned(),
                input_hash,
                evidence,
                balances,
            })
            .await
            .map_err(WorkerError::from)
    }

    async fn persist_failure(&self, error: &WorkerError) -> Result<(), WorkerError> {
        let (reason, evidence) = match error {
            WorkerError::Safety {
                reason, evidence, ..
            } => (*reason, evidence.clone()),
            WorkerError::Provider { provider, message } => (
                "malformed_rpc",
                json!({"provider": provider, "message": message}),
            ),
            _ => return Ok(()),
        };
        let evidence = json!({
            "reason": reason,
            "deployment_id": self.deployment.id,
            "primary_provider": self.primary.name(),
            "secondary_provider": self.secondary.name(),
            "details": evidence,
        });
        let evidence_hash: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&evidence).map_err(|_| {
                WorkerError::Config("could not encode failure evidence".to_owned())
            })?)
            .into();
        self.ledger
            .record_reconciliation_failure(&RecordReconciliationFailure {
                failure_id: Uuid::new_v5(&Uuid::NAMESPACE_OID, &evidence_hash),
                deployment_id: self.deployment.id,
                reason: reason.to_owned(),
                evidence,
                evidence_hash,
            })
            .await?;
        Ok(())
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
                    self.ledger.release_reconciliation_lease(self.deployment.id, self.owner).await?;
                    result?;
                    info!(deployment_id = %self.deployment.id, "reconciliation worker stopped");
                    return Ok(());
                }
                result = self.run_once() => {
                    match result {
                        Ok(recorded) => {
                            info!(snapshot_id = %recorded.snapshot_id, replayed = recorded.replayed, "reconciliation cycle committed");
                            retry = self.config.poll_interval;
                            self.config.poll_interval
                        }
                        Err(WorkerError::LeaseUnavailable) => {
                            warn!("reconciliation lease held by another worker");
                            self.config.poll_interval
                        }
                        Err(error) => {
                            error!(error = %error, "reconciliation cycle failed closed");
                            let delay = retry;
                            retry = (retry * 2).min(self.config.retry_max);
                            delay
                        }
                    }
                }
            };
            tokio::select! {
                result = &mut shutdown => {
                    self.ledger.release_reconciliation_lease(self.deployment.id, self.owner).await?;
                    result?;
                    info!(deployment_id = %self.deployment.id, "reconciliation worker stopped");
                    return Ok(());
                }
                _ = sleep(delay) => {}
            }
        }
    }
}

fn safety(reason: &'static str, message: &str, evidence: Value) -> WorkerError {
    WorkerError::Safety {
        reason,
        message: message.to_owned(),
        evidence,
    }
}

fn validate_chain_ids(expected: u64, primary: u64, secondary: u64) -> Result<(), WorkerError> {
    if primary != expected || secondary != expected {
        return Err(safety(
            "provider_chain_mismatch",
            "RPC chain IDs do not match the registered deployment",
            json!({"expected": expected, "primary": primary, "secondary": secondary}),
        ));
    }
    Ok(())
}

fn validate_finalized_blocks(
    primary: &FinalizedBlock,
    secondary: &FinalizedBlock,
    now: DateTime<Utc>,
    max_age: Duration,
) -> Result<(), WorkerError> {
    if primary.number != secondary.number
        || !primary.hash.eq_ignore_ascii_case(&secondary.hash)
        || primary.timestamp != secondary.timestamp
    {
        return Err(safety(
            "provider_disagreement",
            "RPC providers do not agree on the exact finalized block",
            json!({
                "primary_number": primary.number,
                "primary_hash": primary.hash,
                "secondary_number": secondary.number,
                "secondary_hash": secondary.hash,
            }),
        ));
    }
    let age = now
        .signed_duration_since(primary.timestamp)
        .to_std()
        .unwrap_or(Duration::ZERO);
    if primary.timestamp > now || age > max_age {
        return Err(safety(
            "provider_stale",
            "agreed finalized block timestamp is stale or in the future",
            json!({"block_number": primary.number, "block_timestamp": primary.timestamp}),
        ));
    }
    Ok(())
}

fn validate_code_hashes(
    expected: &str,
    primary: &str,
    secondary: &str,
    block: u64,
) -> Result<(), WorkerError> {
    if primary != secondary || !primary.eq_ignore_ascii_case(expected) {
        return Err(safety(
            "vault_code_mismatch",
            "vault bytecode does not match the pinned runtime hash",
            json!({
                "block_number": block,
                "expected_hash": expected,
                "primary_hash": primary,
                "secondary_hash": secondary,
            }),
        ));
    }
    Ok(())
}

fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

fn parse_u64_quantity(value: &str) -> Result<u64, WorkerError> {
    let raw = checked_hex(value)?;
    u64::from_str_radix(raw, 16)
        .map_err(|_| safety("malformed_rpc", "RPC quantity exceeded u64", json!({})))
}

fn parse_u256_quantity(value: &str) -> Result<BigDecimal, WorkerError> {
    let raw = checked_hex(value)?;
    if raw.len() > 64 {
        return Err(safety(
            "malformed_rpc",
            "RPC quantity exceeded uint256",
            json!({}),
        ));
    }
    let mut result = BigDecimal::zero();
    for byte in raw.bytes() {
        let digit = (byte as char)
            .to_digit(16)
            .ok_or_else(|| safety("malformed_rpc", "RPC quantity was malformed", json!({})))?;
        result = result * BigDecimal::from(16_u8) + BigDecimal::from(digit);
    }
    Ok(result)
}

fn checked_hex(value: &str) -> Result<&str, WorkerError> {
    let raw = value.strip_prefix("0x").ok_or_else(|| {
        safety(
            "malformed_rpc",
            "RPC quantity was not 0x-prefixed",
            json!({}),
        )
    })?;
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(safety(
            "malformed_rpc",
            "RPC quantity was malformed",
            json!({}),
        ));
    }
    Ok(raw)
}

fn code_hash(code: &str) -> Result<String, WorkerError> {
    let raw = checked_hex(code)?;
    let bytes = hex::decode(raw).map_err(|_| {
        safety(
            "vault_code_mismatch",
            "vault bytecode was malformed",
            json!({}),
        )
    })?;
    if bytes.is_empty() {
        return Err(safety(
            "vault_code_mismatch",
            "vault has no runtime bytecode",
            json!({}),
        ));
    }
    Ok(format!("0x{}", hex::encode(Keccak256::digest(bytes))))
}

fn is_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_uint256_without_precision_loss() {
        let value = parse_u256_quantity(&format!("0x{}", "f".repeat(64))).unwrap();
        assert_eq!(
            value.to_string(),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
    }

    #[test]
    fn rejects_malformed_and_oversized_quantities() {
        assert!(parse_u256_quantity("12").is_err());
        assert!(parse_u256_quantity("0xzz").is_err());
        assert!(parse_u256_quantity(&format!("0x1{}", "0".repeat(64))).is_err());
    }

    #[test]
    fn wrong_chain_fails_closed() {
        assert!(matches!(
            validate_chain_ids(84532, 84532, 1),
            Err(WorkerError::Safety {
                reason: "provider_chain_mismatch",
                ..
            })
        ));
    }

    #[test]
    fn finalized_provider_disagreement_and_staleness_fail_closed() {
        let now = Utc::now();
        let first = FinalizedBlock {
            number: 10,
            hash: format!("0x{}", "11".repeat(32)),
            timestamp: now,
        };
        let mut second = first.clone();
        second.hash = format!("0x{}", "22".repeat(32));
        assert!(matches!(
            validate_finalized_blocks(&first, &second, now, Duration::from_secs(60)),
            Err(WorkerError::Safety {
                reason: "provider_disagreement",
                ..
            })
        ));
        let stale = FinalizedBlock {
            timestamp: now - chrono::Duration::seconds(61),
            ..first
        };
        assert!(matches!(
            validate_finalized_blocks(&stale, &stale, now, Duration::from_secs(60)),
            Err(WorkerError::Safety {
                reason: "provider_stale",
                ..
            })
        ));
    }

    #[test]
    fn wrong_or_disagreeing_bytecode_fails_closed() {
        let expected = format!("0x{}", "11".repeat(32));
        let wrong = format!("0x{}", "22".repeat(32));
        assert!(matches!(
            validate_code_hashes(&expected, &wrong, &wrong, 10),
            Err(WorkerError::Safety {
                reason: "vault_code_mismatch",
                ..
            })
        ));
        assert!(validate_code_hashes(&expected, &expected, &expected, 10).is_ok());
    }

    #[test]
    fn retry_ceiling_cannot_be_shorter_than_poll_interval() {
        let mut config = Config {
            enabled: true,
            database_url: "postgres://unused".to_owned(),
            deployment_id: Uuid::new_v4(),
            primary_name: "provider-a".to_owned(),
            primary_url: Url::parse("https://provider-a.example/rpc").unwrap(),
            secondary_name: "provider-b".to_owned(),
            secondary_url: Url::parse("https://provider-b.example/rpc").unwrap(),
            request_timeout: Duration::from_secs(1),
            poll_interval: Duration::from_secs(30),
            retry_max: Duration::from_secs(60),
            lease_duration: Duration::from_secs(120),
            max_finalized_age: Duration::from_secs(300),
        };
        assert!(config.validate().is_ok());
        config.retry_max = Duration::from_secs(29);
        assert!(matches!(config.validate(), Err(WorkerError::Config(_))));
    }
}

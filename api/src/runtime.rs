use std::path::{Path, PathBuf};

use engine::MatchingEngine;
use types::{RuntimeMode, UserId};

const SEPOLIA_CHAIN_ID: u64 = 11_155_111;
const SEPOLIA_SETTLEMENT_ADDRESS: &str = "0xaa8e680c11a883f9bf6eb980b2d4e9d18dd25686";
const DEFAULT_SEPOLIA_RPC: &str = "https://eth-sepolia.g.alchemy.com/v2/demo";

/// Nonsecret process configuration resolved once at startup.
#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    pub mode: RuntimeMode,
    pub chain_id: u64,
    pub settlement_address: Option<String>,
    pub legacy_contract_address: Option<String>,
    pub rpc_url: Option<String>,
    pub snapshot_dir: PathBuf,
    pub zkvm_provider: String,
    pub tee_platform: String,
    pub database_url_configured: bool,
    pub database_max_connections: u32,
    pub database_acquire_timeout_ms: u64,
    pub database_statement_timeout_ms: u64,
    pub database_serializable_retries: u32,
    pub engine_replica_configured: bool,
    pub build_id: Option<String>,
}

impl RuntimeSettings {
    pub fn from_env() -> Result<Self, String> {
        let mode = RuntimeMode::from_env().map_err(|error| error.to_string())?;
        let chain_id = std::env::var("VELA_CHAIN_ID")
            .unwrap_or_else(|_| SEPOLIA_CHAIN_ID.to_string())
            .parse::<u64>()
            .map_err(|_| "VELA_CHAIN_ID must be a u64".to_string())?;

        Ok(Self {
            mode,
            chain_id,
            settlement_address: nonempty_env("VELA_SETTLEMENT_ADDRESS"),
            legacy_contract_address: nonempty_env("VELA_CONTRACT_ADDRESS"),
            rpc_url: nonempty_env("ALCHEMY_API_URL"),
            snapshot_dir: PathBuf::from(
                std::env::var("SNAPSHOT_DIR").unwrap_or_else(|_| "/data".to_string()),
            ),
            zkvm_provider: std::env::var("ZKVM_PROVIDER")
                .ok()
                .or_else(|| std::env::var("VELA_PROVER").ok())
                .unwrap_or_else(|| "placeholder".to_string()),
            tee_platform: std::env::var("TEE_PLATFORM")
                .unwrap_or_else(|_| "placeholder".to_string()),
            database_url_configured: nonempty_env("DATABASE_URL").is_some(),
            database_max_connections: parse_positive_env("VELA_DATABASE_MAX_CONNECTIONS", 10)?,
            database_acquire_timeout_ms: parse_positive_env(
                "VELA_DATABASE_ACQUIRE_TIMEOUT_MS",
                2_000,
            )?,
            database_statement_timeout_ms: parse_positive_env(
                "VELA_DATABASE_STATEMENT_TIMEOUT_MS",
                2_000,
            )?,
            database_serializable_retries: parse_env("VELA_DATABASE_SERIALIZABLE_RETRIES", 3)?,
            engine_replica_configured: nonempty_env("VELA_ENGINE_REPLICA_DIR").is_some(),
            build_id: nonempty_env("VELA_BUILD_ID"),
        })
    }

    pub fn synthetic_state_enabled(&self) -> bool {
        self.mode.allows_demo_state()
    }

    pub fn internal_mm_enabled(&self) -> bool {
        self.mode.allows_demo_state()
    }

    pub fn validate(&self) -> Result<(), StartupValidationError> {
        if !self.mode.is_production() {
            return Ok(());
        }

        let mut problems = Vec::new();

        if self.chain_id == SEPOLIA_CHAIN_ID {
            problems.push("VELA_CHAIN_ID is Sepolia (11155111)".to_string());
        }

        validate_production_address(
            "VELA_SETTLEMENT_ADDRESS",
            self.settlement_address.as_deref(),
            &mut problems,
        );
        if let Some(address) = self.legacy_contract_address.as_deref() {
            validate_production_address("VELA_CONTRACT_ADDRESS", Some(address), &mut problems);
        }

        match self.rpc_url.as_deref() {
            None => problems.push("ALCHEMY_API_URL is not configured".to_string()),
            Some(url) if is_placeholder_rpc(url) => problems.push(
                "ALCHEMY_API_URL points to a testnet, demo, or placeholder endpoint".to_string(),
            ),
            Some(_) => {}
        }

        if is_temporary_path(&self.snapshot_dir) {
            problems.push(format!(
                "SNAPSHOT_DIR {} is temporary storage",
                self.snapshot_dir.display()
            ));
        }

        if self.synthetic_state_enabled() {
            problems.push("synthetic balances or order books are enabled".to_string());
        }
        if self.internal_mm_enabled() {
            problems.push("the internal market-making bot is enabled".to_string());
        }

        if !self.database_url_configured {
            problems.push("DATABASE_URL is not configured".to_string());
        }
        if !self.engine_replica_configured {
            problems.push("VELA_ENGINE_REPLICA_DIR is not configured".to_string());
        }
        match self.build_id.as_deref() {
            None => problems.push("VELA_BUILD_ID is not configured".to_string()),
            Some(value) if is_placeholder_build_id(value) => {
                problems.push("VELA_BUILD_ID is not a specific deployed build identity".to_string())
            }
            Some(_) => {}
        }

        if self.zkvm_provider.eq_ignore_ascii_case("placeholder")
            || self.zkvm_provider.trim().is_empty()
        {
            problems.push("ZKVM_PROVIDER uses the placeholder prover".to_string());
        }
        if self.tee_platform.eq_ignore_ascii_case("placeholder")
            || self.tee_platform.trim().is_empty()
        {
            problems.push("TEE_PLATFORM uses simulated attestations".to_string());
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(StartupValidationError { problems })
        }
    }

    pub fn log_nonsecret_report(&self) {
        tracing::info!(
            runtime_mode = %self.mode,
            chain_id = self.chain_id,
            settlement_address_configured = self.settlement_address.is_some(),
            snapshot_dir = %self.snapshot_dir.display(),
            zkvm_provider = %self.zkvm_provider,
            tee_platform = %self.tee_platform,
            synthetic_state_enabled = self.synthetic_state_enabled(),
            internal_mm_enabled = self.internal_mm_enabled(),
            database_configured = self.database_url_configured,
            database_max_connections = self.database_max_connections,
            database_acquire_timeout_ms = self.database_acquire_timeout_ms,
            database_statement_timeout_ms = self.database_statement_timeout_ms,
            database_serializable_retries = self.database_serializable_retries,
            engine_replica_configured = self.engine_replica_configured,
            build_id = self.build_id.as_deref().unwrap_or("not_configured"),
            "resolved startup configuration"
        );
    }

    /// Reject snapshots or WAL recovery that carry the well-known demo
    /// account into a production process.
    pub fn validate_engine_state(
        &self,
        engine: &MatchingEngine,
    ) -> Result<(), StartupValidationError> {
        if !self.mode.is_production() {
            return Ok(());
        }

        let demo_user = UserId([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let has_demo_balance = engine
            .snapshot_balances()
            .keys()
            .any(|(user, _)| user == &demo_user);

        if has_demo_balance {
            Err(StartupValidationError {
                problems: vec![
                    "production state contains the reserved 0x0000000000000000000000000000000000000001 demo account"
                        .to_string(),
                ],
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupValidationError {
    pub problems: Vec<String>,
}

impl std::fmt::Display for StartupValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "production startup validation failed:")?;
        for problem in &self.problems {
            writeln!(f, "- {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for StartupValidationError {}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn parse_env<T>(name: &str, default: T) -> Result<T, String>
where
    T: std::str::FromStr,
{
    match nonempty_env(name) {
        Some(value) => value
            .parse::<T>()
            .map_err(|_| format!("{name} has an invalid value")),
        None => Ok(default),
    }
}

fn parse_positive_env<T>(name: &str, default: T) -> Result<T, String>
where
    T: std::str::FromStr + PartialOrd + From<u8>,
{
    let value = parse_env(name, default)?;
    if value <= T::from(0) {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(value)
}

fn validate_production_address(name: &str, address: Option<&str>, problems: &mut Vec<String>) {
    match address {
        None => problems.push(format!("{name} is not configured")),
        Some(value)
            if value
                .trim()
                .eq_ignore_ascii_case(SEPOLIA_SETTLEMENT_ADDRESS) =>
        {
            problems.push(format!("{name} is the known Sepolia settlement address"));
        }
        Some(_) => {}
    }
}

fn is_placeholder_rpc(url: &str) -> bool {
    let normalized = url.trim().to_ascii_lowercase();
    normalized == DEFAULT_SEPOLIA_RPC
        || normalized.contains("sepolia")
        || normalized.ends_with("/demo")
        || normalized.contains("localhost")
        || normalized.contains("127.0.0.1")
        || normalized.contains("example.invalid")
        || normalized.contains("changeme")
        || normalized.contains("your-api-key")
}

fn is_placeholder_build_id(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "unknown" | "latest" | "dev"
    )
}

fn is_temporary_path(path: &Path) -> bool {
    path.starts_with(std::env::temp_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_production_settings() -> RuntimeSettings {
        RuntimeSettings {
            mode: RuntimeMode::Production,
            chain_id: 1,
            settlement_address: Some("0x1111111111111111111111111111111111111111".to_string()),
            legacy_contract_address: None,
            rpc_url: Some("https://eth-mainnet.g.alchemy.com/v2/real-key".to_string()),
            snapshot_dir: PathBuf::from("/data"),
            zkvm_provider: "sp1".to_string(),
            tee_platform: "amd-sev-snp".to_string(),
            database_url_configured: true,
            database_max_connections: 10,
            database_acquire_timeout_ms: 2_000,
            database_statement_timeout_ms: 2_000,
            database_serializable_retries: 3,
            engine_replica_configured: true,
            build_id: Some("0123456789abcdef".to_owned()),
        }
    }

    #[test]
    fn production_accepts_non_demo_configuration() {
        assert!(valid_production_settings().validate().is_ok());
    }

    #[test]
    fn production_rejects_testnet_and_placeholder_configuration() {
        let settings = RuntimeSettings {
            mode: RuntimeMode::Production,
            chain_id: SEPOLIA_CHAIN_ID,
            settlement_address: Some(SEPOLIA_SETTLEMENT_ADDRESS.to_string()),
            legacy_contract_address: Some(SEPOLIA_SETTLEMENT_ADDRESS.to_string()),
            rpc_url: Some(DEFAULT_SEPOLIA_RPC.to_string()),
            snapshot_dir: std::env::temp_dir().join("vela"),
            zkvm_provider: "placeholder".to_string(),
            tee_platform: "placeholder".to_string(),
            database_url_configured: false,
            database_max_connections: 10,
            database_acquire_timeout_ms: 2_000,
            database_statement_timeout_ms: 2_000,
            database_serializable_retries: 3,
            engine_replica_configured: false,
            build_id: None,
        };

        let error = settings.validate().expect_err("configuration must fail");
        assert!(error.problems.iter().any(|p| p.contains("Sepolia")));
        assert!(error
            .problems
            .iter()
            .any(|p| p.contains("placeholder endpoint")));
        assert!(error.problems.iter().any(|p| p.contains("temporary")));
        assert!(error
            .problems
            .iter()
            .any(|p| p.contains("placeholder prover")));
        assert!(error
            .problems
            .iter()
            .any(|p| p.contains("simulated attestations")));
    }

    #[test]
    fn production_guard_matrix_rejects_each_unsafe_setting_independently() {
        type Mutator = fn(&mut RuntimeSettings);
        let cases: &[(&str, Mutator, &str)] = &[
            (
                "sepolia chain",
                |s| s.chain_id = SEPOLIA_CHAIN_ID,
                "Sepolia",
            ),
            (
                "missing settlement address",
                |s| s.settlement_address = None,
                "VELA_SETTLEMENT_ADDRESS is not configured",
            ),
            (
                "sepolia settlement address",
                |s| s.settlement_address = Some(SEPOLIA_SETTLEMENT_ADDRESS.to_string()),
                "known Sepolia settlement address",
            ),
            (
                "missing rpc",
                |s| s.rpc_url = None,
                "ALCHEMY_API_URL is not configured",
            ),
            (
                "demo rpc",
                |s| s.rpc_url = Some(DEFAULT_SEPOLIA_RPC.to_string()),
                "placeholder endpoint",
            ),
            (
                "localhost rpc",
                |s| s.rpc_url = Some("http://localhost:8545".to_string()),
                "placeholder endpoint",
            ),
            (
                "temporary state",
                |s| s.snapshot_dir = std::env::temp_dir().join("vela"),
                "temporary storage",
            ),
            (
                "placeholder prover",
                |s| s.zkvm_provider = "placeholder".to_string(),
                "placeholder prover",
            ),
            (
                "simulated tee",
                |s| s.tee_platform = "placeholder".to_string(),
                "simulated attestations",
            ),
            (
                "missing database",
                |s| s.database_url_configured = false,
                "DATABASE_URL is not configured",
            ),
            (
                "missing engine replica",
                |s| s.engine_replica_configured = false,
                "VELA_ENGINE_REPLICA_DIR is not configured",
            ),
            (
                "missing build identity",
                |s| s.build_id = None,
                "VELA_BUILD_ID is not configured",
            ),
            (
                "placeholder build identity",
                |s| s.build_id = Some("latest".to_owned()),
                "not a specific deployed build identity",
            ),
        ];

        for (name, mutate, expected) in cases {
            let mut settings = valid_production_settings();
            mutate(&mut settings);
            let error = match settings.validate() {
                Ok(()) => panic!("{name} unexpectedly passed validation"),
                Err(error) => error,
            };
            assert!(
                error
                    .problems
                    .iter()
                    .any(|problem| problem.contains(expected)),
                "{name} failed for the wrong reason: {:?}",
                error.problems
            );
        }
    }

    #[test]
    fn only_testnet_demo_enables_synthetic_state_and_internal_mm() {
        let mut settings = valid_production_settings();
        assert!(!settings.synthetic_state_enabled());
        assert!(!settings.internal_mm_enabled());

        settings.mode = RuntimeMode::Development;
        assert!(!settings.synthetic_state_enabled());
        assert!(!settings.internal_mm_enabled());

        settings.mode = RuntimeMode::TestnetDemo;
        assert!(settings.synthetic_state_enabled());
        assert!(settings.internal_mm_enabled());
    }

    #[test]
    fn production_rejects_demo_account_in_engine_state() {
        use types::{AssetId, DepositRequest, Request};

        let settings = valid_production_settings();
        let demo_user = UserId([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let mut engine = MatchingEngine::new(types::FeeConfig::default(), 5.0);
        engine.process(
            Request::Deposit(DepositRequest {
                user: demo_user,
                asset: AssetId::from_str("USDC"),
                amount: 1,
                l1_tx_hash: [1; 32],
                source_chain: Default::default(),
                bridge_id: None,
            }),
            0,
        );

        assert!(settings.validate_engine_state(&engine).is_err());
    }
}

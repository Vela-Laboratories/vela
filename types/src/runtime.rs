use std::str::FromStr;

use thiserror::Error;

/// The operating mode for a Vela process.
///
/// `TestnetDemo` is the only mode permitted to create synthetic balances,
/// seeded order books, or the internal market-making bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Development,
    TestnetDemo,
    Production,
}

impl RuntimeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::TestnetDemo => "testnet-demo",
            Self::Production => "production",
        }
    }

    pub const fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }

    pub const fn allows_demo_state(self) -> bool {
        matches!(self, Self::TestnetDemo)
    }

    /// Resolve the runtime mode from the process environment.
    ///
    /// `VELA_RUNTIME_MODE` is authoritative when present. The legacy
    /// `ENVIRONMENT` variable remains supported, including the important
    /// `ENVIRONMENT=production` fail-closed behavior.
    pub fn from_env() -> Result<Self, RuntimeModeError> {
        let explicit = std::env::var("VELA_RUNTIME_MODE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let legacy = std::env::var("ENVIRONMENT")
            .ok()
            .filter(|value| !value.trim().is_empty());

        let mode = match explicit.as_deref() {
            Some(value) => value.parse()?,
            None => match legacy.as_deref() {
                Some(value) => value.parse()?,
                None => Self::Development,
            },
        };

        // Never allow a newly added variable to silently downgrade a process
        // that an existing deployment still marks as production.
        if legacy
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("production"))
            && !mode.is_production()
        {
            return Err(RuntimeModeError::ProductionDowngrade {
                runtime_mode: mode.as_str().to_string(),
            });
        }

        Ok(mode)
    }
}

impl std::fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RuntimeMode {
    type Err = RuntimeModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "testnet-demo" | "testnet_demo" | "demo" => Ok(Self::TestnetDemo),
            "production" | "prod" => Ok(Self::Production),
            other => Err(RuntimeModeError::Invalid(other.to_string())),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeModeError {
    #[error("invalid runtime mode {0:?}; expected development, testnet-demo, or production")]
    Invalid(String),
    #[error("ENVIRONMENT=production cannot be overridden by VELA_RUNTIME_MODE={runtime_mode}")]
    ProductionDowngrade { runtime_mode: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_runtime_modes() {
        assert_eq!("development".parse(), Ok(RuntimeMode::Development));
        assert_eq!("testnet-demo".parse(), Ok(RuntimeMode::TestnetDemo));
        assert_eq!("production".parse(), Ok(RuntimeMode::Production));
    }

    #[test]
    fn only_testnet_demo_allows_synthetic_state() {
        assert!(!RuntimeMode::Development.allows_demo_state());
        assert!(RuntimeMode::TestnetDemo.allows_demo_state());
        assert!(!RuntimeMode::Production.allows_demo_state());
    }

    #[test]
    fn rejects_unknown_runtime_mode() {
        assert!(matches!(
            "staging".parse::<RuntimeMode>(),
            Err(RuntimeModeError::Invalid(value)) if value == "staging"
        ));
    }
}

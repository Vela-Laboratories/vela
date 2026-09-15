use crate::durability::StorageHealth;
use ledger::{EngineLeaderToken, Ledger};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const ACTIVE: u8 = 1;
const LOST: u8 = 2;
const RELEASED: u8 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct LeadershipSnapshot {
    pub configured: bool,
    pub state: &'static str,
    pub generation: i64,
    pub lease_expires_at_ms: u64,
}

pub struct EngineLeadershipGuard {
    ledger: Arc<Ledger>,
    token: EngineLeaderToken,
    lease_duration: Duration,
    operation_timeout: Duration,
    state: AtomicU8,
    lease_expires_at_ms: AtomicU64,
    health: Arc<StorageHealth>,
}

impl EngineLeadershipGuard {
    pub async fn acquire(
        ledger: Arc<Ledger>,
        health: Arc<StorageHealth>,
    ) -> Result<Arc<Self>, String> {
        let lease_duration = duration_from_env("VELA_ENGINE_LEASE_MS", 15_000, 1_000)?;
        let renew_interval = duration_from_env("VELA_ENGINE_LEASE_RENEW_MS", 5_000, 100)?;
        let operation_timeout = duration_from_env("VELA_ENGINE_LEASE_TIMEOUT_MS", 2_000, 100)?;
        validate_timing(lease_duration, renew_interval, operation_timeout)?;
        let owner_id = Uuid::new_v4();
        let leadership = tokio::time::timeout(
            operation_timeout,
            ledger.acquire_engine_leadership(owner_id, lease_duration),
        )
        .await
        .map_err(|_| "engine leadership acquisition timed out".to_owned())?
        .map_err(|error| format!("engine leadership acquisition failed: {error}"))?
        .ok_or_else(|| "another engine holds the authoritative leadership lease".to_owned())?;
        let guard = Arc::new(Self {
            ledger,
            token: leadership.token,
            lease_duration,
            operation_timeout,
            state: AtomicU8::new(ACTIVE),
            lease_expires_at_ms: AtomicU64::new(timestamp_ms(leadership.lease_expires_at)),
            health,
        });
        Self::spawn_renewal(Arc::clone(&guard), renew_interval);
        Ok(guard)
    }

    fn spawn_renewal(guard: Arc<Self>, renew_interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(renew_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if !guard.is_active() {
                    return;
                }
                let renewal = tokio::time::timeout(
                    guard.operation_timeout,
                    guard
                        .ledger
                        .renew_engine_leadership(guard.token, guard.lease_duration),
                )
                .await;
                if !guard.is_active() {
                    return;
                }
                match renewal {
                    Ok(Ok(Some(expires_at))) => {
                        guard
                            .lease_expires_at_ms
                            .store(timestamp_ms(expires_at), Ordering::Release);
                    }
                    Ok(Ok(None)) => {
                        guard.lose("engine_leadership_lost", "lease was expired or superseded");
                        return;
                    }
                    Ok(Err(error)) => {
                        guard.lose("engine_leadership_renewal", error);
                        return;
                    }
                    Err(_) => {
                        guard.lose("engine_leadership_indeterminate", "lease renewal timed out");
                        return;
                    }
                }
            }
        });
    }

    pub fn token(&self) -> EngineLeaderToken {
        self.token
    }

    pub fn is_active(&self) -> bool {
        self.state.load(Ordering::Acquire) == ACTIVE && self.health.is_healthy()
    }

    pub async fn validate_at_commit(&self) -> Result<(), String> {
        if !self.is_active() {
            return Err("engine leadership is fenced".to_owned());
        }
        let validation = tokio::time::timeout(
            self.operation_timeout,
            self.ledger
                .renew_engine_leadership(self.token, self.lease_duration),
        )
        .await;
        if !self.is_active() {
            return Err("engine leadership is no longer active".to_owned());
        }
        match validation {
            Ok(Ok(Some(expires_at))) => {
                self.lease_expires_at_ms
                    .store(timestamp_ms(expires_at), Ordering::Release);
                Ok(())
            }
            Ok(Ok(None)) => {
                self.lose(
                    "engine_leadership_lost",
                    "commit-time lease validation failed",
                );
                Err("engine leadership lease is absent, expired, or superseded".to_owned())
            }
            Ok(Err(error)) => {
                self.lose("engine_leadership_validation", &error);
                Err(error.to_string())
            }
            Err(_) => {
                self.lose(
                    "engine_leadership_indeterminate",
                    "commit-time validation timed out",
                );
                Err("engine leadership validation timed out".to_owned())
            }
        }
    }

    fn lose(&self, code: &'static str, detail: impl std::fmt::Display) {
        if self
            .state
            .compare_exchange(ACTIVE, LOST, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.health.fence(code, detail);
        }
    }

    pub fn snapshot(&self) -> LeadershipSnapshot {
        let state = match self.state.load(Ordering::Acquire) {
            ACTIVE if self.health.is_healthy() => "active",
            RELEASED => "released",
            _ => "fenced",
        };
        LeadershipSnapshot {
            configured: true,
            state,
            generation: self.token.generation,
            lease_expires_at_ms: self.lease_expires_at_ms.load(Ordering::Acquire),
        }
    }

    pub async fn release(&self) -> Result<bool, String> {
        if self
            .state
            .compare_exchange(ACTIVE, RELEASED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(false);
        }
        tokio::time::timeout(
            self.operation_timeout,
            self.ledger.release_engine_leadership(self.token),
        )
        .await
        .map_err(|_| "engine leadership release timed out".to_owned())?
        .map_err(|error| error.to_string())
    }

    pub fn wal_commit_timeout(&self) -> Duration {
        self.operation_timeout
    }
}

fn duration_from_env(name: &str, default_ms: u64, minimum_ms: u64) -> Result<Duration, String> {
    let value = std::env::var(name)
        .ok()
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| format!("{name} must be an integer"))
        })
        .transpose()?
        .unwrap_or(default_ms);
    if value < minimum_ms {
        return Err(format!("{name} must be at least {minimum_ms}ms"));
    }
    Ok(Duration::from_millis(value))
}

fn timestamp_ms(timestamp: chrono::DateTime<chrono::Utc>) -> u64 {
    u64::try_from(timestamp.timestamp_millis()).unwrap_or_default()
}

fn validate_timing(
    lease_duration: Duration,
    renew_interval: Duration,
    operation_timeout: Duration,
) -> Result<(), String> {
    if operation_timeout >= lease_duration {
        return Err(
            "VELA_ENGINE_LEASE_TIMEOUT_MS must be shorter than VELA_ENGINE_LEASE_MS".to_owned(),
        );
    }
    let renewal_budget = renew_interval
        .checked_add(operation_timeout)
        .ok_or_else(|| "engine leadership timing values overflow".to_owned())?;
    if renewal_budget >= lease_duration {
        return Err("VELA_ENGINE_LEASE_RENEW_MS plus VELA_ENGINE_LEASE_TIMEOUT_MS must be shorter than VELA_ENGINE_LEASE_MS".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timing_leaves_a_full_timeout_before_expiry() {
        assert!(validate_timing(
            Duration::from_secs(15),
            Duration::from_secs(5),
            Duration::from_secs(2)
        )
        .is_ok());
    }

    #[test]
    fn rejects_timeout_or_renewal_budget_at_lease_boundary() {
        assert!(validate_timing(
            Duration::from_secs(2),
            Duration::from_millis(500),
            Duration::from_secs(2)
        )
        .is_err());
        assert!(validate_timing(
            Duration::from_secs(3),
            Duration::from_secs(1),
            Duration::from_secs(2)
        )
        .is_err());
    }
}

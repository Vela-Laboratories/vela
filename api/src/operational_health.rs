use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const DEPENDENCIES: [(&str, &str); 4] = [
    ("indexer", "VELA_INDEXER_READINESS_URL"),
    ("signer", "VELA_SIGNER_READINESS_URL"),
    ("custody_rpc", "VELA_CUSTODY_RPC_READINESS_URL"),
    ("reconciler", "VELA_RECONCILER_READINESS_URL"),
];

#[derive(Debug, Clone, Serialize)]
pub struct DependencyHealth {
    pub configured: bool,
    pub healthy: bool,
    pub failure_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationalHealthSnapshot {
    pub healthy: bool,
    pub dependencies: BTreeMap<String, DependencyHealth>,
}

struct Target {
    name: &'static str,
    url: Option<String>,
    invalid: bool,
}

impl Target {
    fn from_value(name: &'static str, value: Option<String>) -> Self {
        match value {
            None => Self {
                name,
                url: None,
                invalid: false,
            },
            Some(value) if valid_http_endpoint(&value) => Self {
                name,
                url: Some(value),
                invalid: false,
            },
            Some(_) => Self {
                name,
                url: None,
                invalid: true,
            },
        }
    }
}

pub struct OperationalHealth {
    client: reqwest::Client,
    targets: Vec<Target>,
    alert_webhook: Option<String>,
    timeout: Duration,
    last_alert_fingerprint: Mutex<Option<String>>,
}

impl OperationalHealth {
    pub fn from_env() -> Arc<Self> {
        let timeout_ms = std::env::var("VELA_DEPENDENCY_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2_000)
            .clamp(100, 30_000);
        Arc::new(Self {
            client: reqwest::Client::new(),
            targets: DEPENDENCIES
                .iter()
                .map(|(name, variable)| Target::from_value(name, nonempty_env(variable)))
                .collect(),
            alert_webhook: nonempty_env("VELA_ALERT_WEBHOOK_URL")
                .filter(|value| valid_http_endpoint(value)),
            timeout: Duration::from_millis(timeout_ms),
            last_alert_fingerprint: Mutex::new(None),
        })
    }

    pub async fn probe(&self, required: bool) -> OperationalHealthSnapshot {
        let probes = self.targets.iter().map(|target| async move {
            let health = match &target.url {
                None if target.invalid => DependencyHealth {
                    configured: true,
                    healthy: false,
                    failure_code: Some("invalid_url".to_owned()),
                },
                None => DependencyHealth {
                    configured: false,
                    healthy: !required,
                    failure_code: required.then(|| "not_configured".to_owned()),
                },
                Some(url) => {
                    match tokio::time::timeout(self.timeout, self.client.get(url).send()).await {
                        Err(_) => DependencyHealth {
                            configured: true,
                            healthy: false,
                            failure_code: Some("timeout".to_owned()),
                        },
                        Ok(Err(_)) => DependencyHealth {
                            configured: true,
                            healthy: false,
                            failure_code: Some("request_failed".to_owned()),
                        },
                        Ok(Ok(response)) if response.status().is_success() => DependencyHealth {
                            configured: true,
                            healthy: true,
                            failure_code: None,
                        },
                        Ok(Ok(_)) => DependencyHealth {
                            configured: true,
                            healthy: false,
                            failure_code: Some("unhealthy_status".to_owned()),
                        },
                    }
                }
            };
            (target.name.to_owned(), health)
        });
        let dependencies: BTreeMap<_, _> = futures_util::future::join_all(probes)
            .await
            .into_iter()
            .collect();
        OperationalHealthSnapshot {
            healthy: dependencies.values().all(|health| health.healthy),
            dependencies,
        }
    }

    pub async fn run(self: Arc<Self>, state: Arc<crate::AppState>) {
        let interval = std::env::var("VELA_DEPENDENCY_POLL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15)
            .clamp(5, 300);
        loop {
            let dependencies = self.probe(state.runtime_mode.is_production()).await;
            let storage = state.storage_health.snapshot();
            let recovery_healthy = state
                .recovery_replica
                .as_ref()
                .is_none_or(|replica| replica.snapshot().healthy);
            let healthy = dependencies.healthy && storage.healthy && recovery_healthy;
            self.notify_state_change(healthy, &dependencies, storage.failure_code.as_deref())
                .await;
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    }

    async fn notify_state_change(
        &self,
        healthy: bool,
        dependencies: &OperationalHealthSnapshot,
        storage_failure: Option<&str>,
    ) {
        let Some(webhook) = &self.alert_webhook else {
            return;
        };
        let payload = serde_json::json!({
            "schema": "vela.operational-alert.v1",
            "service": "vela-engine",
            "healthy": healthy,
            "matcher_storage_failure": storage_failure,
            "dependencies": dependencies.dependencies,
        });
        let fingerprint = hex::encode(Sha256::digest(payload.to_string().as_bytes()));
        let mut last = self.last_alert_fingerprint.lock().await;
        if last.as_deref() == Some(&fingerprint) || (healthy && last.is_none()) {
            return;
        }
        match tokio::time::timeout(
            self.timeout,
            self.client.post(webhook).json(&payload).send(),
        )
        .await
        {
            Ok(Ok(response)) if response.status().is_success() => {
                *last = Some(fingerprint);
            }
            Ok(Ok(response)) => {
                tracing::error!(status = %response.status(), "operational alert webhook rejected notification");
            }
            Ok(Err(_)) => {
                tracing::error!("operational alert webhook delivery failed");
            }
            Err(_) => tracing::error!("operational alert webhook timed out"),
        }
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn valid_http_endpoint(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unconfigured() -> OperationalHealth {
        OperationalHealth {
            client: reqwest::Client::new(),
            targets: DEPENDENCIES
                .iter()
                .map(|(name, _)| Target {
                    name,
                    url: None,
                    invalid: false,
                })
                .collect(),
            alert_webhook: None,
            timeout: Duration::from_millis(100),
            last_alert_fingerprint: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn production_requires_every_operational_dependency() {
        let snapshot = unconfigured().probe(true).await;
        assert!(!snapshot.healthy);
        assert!(snapshot.dependencies.values().all(|health| {
            !health.configured
                && !health.healthy
                && health.failure_code.as_deref() == Some("not_configured")
        }));
    }

    #[tokio::test]
    async fn development_allows_unconfigured_operational_dependencies() {
        let snapshot = unconfigured().probe(false).await;
        assert!(snapshot.healthy);
        assert!(snapshot
            .dependencies
            .values()
            .all(|health| !health.configured && health.healthy));
    }

    #[tokio::test]
    async fn malformed_or_credential_bearing_dependency_url_fails_closed() {
        let mut health = unconfigured();
        health.targets[0] = Target::from_value("indexer", Some("not a URL".to_owned()));
        health.targets[1] = Target::from_value(
            "signer",
            Some("https://operator:secret@example.invalid/ready".to_owned()),
        );
        let snapshot = health.probe(false).await;
        for name in ["indexer", "signer"] {
            let dependency = &snapshot.dependencies[name];
            assert!(dependency.configured);
            assert!(!dependency.healthy);
            assert_eq!(dependency.failure_code.as_deref(), Some("invalid_url"));
        }
        assert!(!snapshot.healthy);
    }
}

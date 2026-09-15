use crate::wal::{Wal, WalMutationBatch, MUTATION_BATCH};
use engine::{FinancialCommitter, FinancialEffectSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use types::{Request, Response, Timestamp};

#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageHealthSnapshot {
    pub healthy: bool,
    pub fenced_at_ms: Option<u64>,
    pub failure_code: Option<String>,
    pub failure_count: u64,
}

pub struct StorageHealth {
    healthy: AtomicBool,
    fenced_at_ms: AtomicU64,
    failure_count: AtomicU64,
    failure_code: Mutex<Option<String>>,
}

impl StorageHealth {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            healthy: AtomicBool::new(true),
            fenced_at_ms: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
            failure_code: Mutex::new(None),
        })
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    pub fn fence(&self, failure_code: &'static str, detail: impl std::fmt::Display) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        if self
            .healthy
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.fenced_at_ms.store(now_ms.max(1), Ordering::Release);
            *self.failure_code.lock().unwrap() = Some(failure_code.to_owned());
            tracing::error!(failure_code, error = %detail, "durable storage fenced; mutating traffic disabled until process restart");
        }
    }

    pub fn snapshot(&self) -> StorageHealthSnapshot {
        let fenced_at_ms = self.fenced_at_ms.load(Ordering::Acquire);
        StorageHealthSnapshot {
            healthy: self.is_healthy(),
            fenced_at_ms: (fenced_at_ms != 0).then_some(fenced_at_ms),
            failure_code: self.failure_code.lock().unwrap().clone(),
            failure_count: self.failure_count.load(Ordering::Relaxed),
        }
    }
}

pub fn shutdown_stop_reason(snapshot_succeeded: bool, health: &StorageHealth) -> &'static str {
    if snapshot_succeeded && health.is_healthy() {
        "clean"
    } else if !health.is_healthy() {
        "durability_fenced"
    } else {
        "snapshot_failed"
    }
}

pub struct DurableMutationCommitter {
    journal: Arc<dyn MutationJournal>,
    ledger: Option<Arc<dyn FinancialCommitter>>,
    leadership: Option<Arc<crate::leadership::EngineLeadershipGuard>>,
    health: Arc<StorageHealth>,
}

impl DurableMutationCommitter {
    pub fn new(
        wal: Arc<Wal>,
        ledger: Option<Arc<dyn FinancialCommitter>>,
        leadership: Option<Arc<crate::leadership::EngineLeadershipGuard>>,
        health: Arc<StorageHealth>,
    ) -> Self {
        Self {
            journal: Arc::new(WalMutationJournal { wal }),
            ledger,
            leadership,
            health,
        }
    }

    #[cfg(test)]
    fn with_journal(journal: Arc<dyn MutationJournal>, health: Arc<StorageHealth>) -> Self {
        Self {
            journal,
            ledger: None,
            leadership: None,
            health,
        }
    }

    async fn commit_batch(
        &self,
        requests: &[(Request, Timestamp)],
        responses: &[Vec<Response>],
        effects: Option<&FinancialEffectSet>,
    ) -> Result<(), String> {
        if !self.health.is_healthy() {
            return Err("durable storage is fenced".to_owned());
        }

        // PostgreSQL remains authoritative for financial balances. It commits
        // first; the local WAL then gates engine publication. These are two
        // durability domains, so any failure fences the process and unclean
        // recovery remains operator-gated rather than pretending 2PC exists.
        if let (Some(ledger), Some(effects)) = (&self.ledger, effects) {
            if let Err(error) = ledger.commit(effects).await {
                self.health.fence("ledger_commit", &error);
                return Err(error);
            }
        }

        // Zero-net and resting-order mutations do not create journal legs, but
        // they still cross the same commit boundary and must prove this process
        // owns the current PostgreSQL generation before local publication.
        if let Some(leadership) = &self.leadership {
            if let Err(error) = leadership.validate_at_commit().await {
                self.health.fence("engine_leadership_commit", &error);
                return Err(error);
            }
        }

        let batch = WalMutationBatch {
            requests: requests.to_vec(),
            responses: responses.to_vec(),
        };
        let journal_result = match &self.leadership {
            Some(leadership) => {
                tokio::time::timeout(leadership.wal_commit_timeout(), self.journal.commit(&batch))
                    .await
                    .unwrap_or_else(|_| Err("WAL mutation commit timed out".to_owned()))
            }
            None => self.journal.commit(&batch).await,
        };
        if let Err(error) = journal_result {
            let message = format!("WAL mutation commit failed: {error}");
            self.health.fence("wal_commit", &error);
            return Err(message);
        }
        Ok(())
    }
}

trait MutationJournal: Send + Sync {
    fn commit<'a>(
        &'a self,
        batch: &'a WalMutationBatch,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

struct WalMutationJournal {
    wal: Arc<Wal>,
}

impl MutationJournal for WalMutationJournal {
    fn commit<'a>(
        &'a self,
        batch: &'a WalMutationBatch,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            self.wal
                .append(MUTATION_BATCH, batch)
                .await
                .map_err(|error| format!("append failed: {error}"))?;
            self.wal
                .flush()
                .await
                .map_err(|error| format!("flush failed: {error}"))
        })
    }
}

impl FinancialCommitter for DurableMutationCommitter {
    fn requires_financial_effects(&self) -> bool {
        self.ledger.is_some()
    }

    fn commit<'a>(
        &'a self,
        effects: &'a FinancialEffectSet,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            match &self.ledger {
                Some(ledger) => ledger.commit(effects).await,
                None => Ok(()),
            }
        })
    }

    fn commit_mutation<'a>(
        &'a self,
        requests: &'a [(Request, Timestamp)],
        responses: &'a [Vec<Response>],
        effects: Option<&'a FinancialEffectSet>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(self.commit_batch(requests, responses, effects))
    }

    fn reject_mutation(&self, error: &str) {
        self.health.fence("financial_effect_validation", error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingJournal(AtomicUsize);

    impl MutationJournal for FailingJournal {
        fn commit<'a>(
            &'a self,
            _batch: &'a WalMutationBatch,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("injected fsync failure".to_owned()) })
        }
    }

    #[tokio::test]
    async fn journal_failure_latches_fence_and_blocks_later_commits() {
        let health = StorageHealth::new();
        let journal = Arc::new(FailingJournal(AtomicUsize::new(0)));
        let committer =
            DurableMutationCommitter::with_journal(journal.clone(), Arc::clone(&health));

        assert!(committer.commit_batch(&[], &[], None).await.is_err());
        assert!(!health.is_healthy());
        assert_eq!(journal.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            committer.commit_batch(&[], &[], None).await.unwrap_err(),
            "durable storage is fenced"
        );
        assert_eq!(journal.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            health.snapshot().failure_code.as_deref(),
            Some("wal_commit")
        );
    }

    #[test]
    fn storage_fence_is_irreversible_and_preserves_first_failure() {
        let health = StorageHealth::new();
        health.fence("wal_commit", "first");
        health.fence("snapshot_publish", "second");
        let snapshot = health.snapshot();
        assert!(!snapshot.healthy);
        assert_eq!(snapshot.failure_code.as_deref(), Some("wal_commit"));
        assert_eq!(snapshot.failure_count, 2);
        assert_eq!(shutdown_stop_reason(true, &health), "durability_fenced");
    }

    #[test]
    fn clean_stop_requires_both_snapshot_success_and_healthy_storage() {
        let health = StorageHealth::new();
        assert_eq!(shutdown_stop_reason(true, &health), "clean");
        assert_eq!(shutdown_stop_reason(false, &health), "snapshot_failed");
    }

    #[test]
    fn invalid_financial_effect_fences_future_mutations() {
        let health = StorageHealth::new();
        let committer = DurableMutationCommitter::with_journal(
            Arc::new(FailingJournal(AtomicUsize::new(0))),
            Arc::clone(&health),
        );

        committer.reject_mutation("unbalanced proposal");

        let snapshot = health.snapshot();
        assert!(!snapshot.healthy);
        assert_eq!(
            snapshot.failure_code.as_deref(),
            Some("financial_effect_validation")
        );
    }
}

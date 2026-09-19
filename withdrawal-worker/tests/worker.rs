use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use parking_lot::Mutex;
use sha3::{Digest, Keccak256};
use std::{collections::HashSet, sync::Arc, time::Duration};
use url::Url;
use uuid::Uuid;
use withdrawal_worker::*;

const VAULT: &str = "0x1111111111111111111111111111111111111111";
const CUSTOMER: &str = "0x2222222222222222222222222222222222222222";
const DESTINATION: &str = "0x3333333333333333333333333333333333333333";
const ASSET: &str = "0x4444444444444444444444444444444444444444";

#[test]
fn registered_event_topic_matches_the_v3_contract_abi() {
    assert_eq!(
        WITHDRAWAL_EVENT_TOPIC,
        format!(
            "0x{}",
            hex::encode(Keccak256::digest(WITHDRAWAL_EVENT_SIGNATURE.as_bytes()))
        )
    );
}

#[derive(Clone)]
struct FakeProvider {
    name: String,
    chain: u64,
    block: FinalizedBlock,
    code: Vec<u8>,
    observation: ProviderObservation,
}

#[async_trait]
impl ChainProvider for FakeProvider {
    fn name(&self) -> &str {
        &self.name
    }
    async fn chain_id(&self) -> Result<u64, WorkerError> {
        Ok(self.chain)
    }
    async fn finalized_block(&self) -> Result<FinalizedBlock, WorkerError> {
        Ok(self.block.clone())
    }
    async fn runtime_code(&self, _: &str, _: u64) -> Result<Vec<u8>, WorkerError> {
        Ok(self.code.clone())
    }
    async fn observe(
        &self,
        _: &Deployment,
        _: &Candidate,
    ) -> Result<ProviderObservation, WorkerError> {
        Ok(self.observation.clone())
    }
}

#[derive(Default)]
struct State {
    candidates: Vec<Candidate>,
    cursor: usize,
    served: HashSet<Uuid>,
    terminal: bool,
    executions: HashSet<Uuid>,
    drops: usize,
    releases: usize,
    reverts: HashSet<Uuid>,
    replacements: HashSet<Uuid>,
    lease: Option<Uuid>,
    checkpoints: usize,
}

#[derive(Clone)]
struct FakeStore {
    deployment: Deployment,
    state: Arc<Mutex<State>>,
}

#[async_trait]
impl WorkerStore for FakeStore {
    async fn deployment(&self, _: Uuid) -> Result<Deployment, WorkerError> {
        Ok(self.deployment.clone())
    }
    async fn acquire_lease(&self, _: Uuid, owner: Uuid, _: Duration) -> Result<(), WorkerError> {
        let mut s = self.state.lock();
        if s.lease.is_some_and(|v| v != owner) {
            return Err(WorkerError::LeaseUnavailable);
        }
        s.lease = Some(owner);
        Ok(())
    }
    async fn release_lease(&self, _: Uuid, owner: Uuid) -> Result<(), WorkerError> {
        let mut s = self.state.lock();
        if s.lease == Some(owner) {
            s.lease = None;
        }
        Ok(())
    }
    async fn candidates(
        &self,
        _: &Deployment,
        _: Uuid,
        limit: u32,
    ) -> Result<Vec<Candidate>, WorkerError> {
        let mut state = self.state.lock();
        if state.candidates.is_empty() {
            return Ok(vec![]);
        }
        if state.cursor >= state.candidates.len() {
            state.cursor = 0;
        }
        let rows: Vec<_> = state
            .candidates
            .iter()
            .skip(state.cursor)
            .take(limit as usize)
            .cloned()
            .collect();
        state
            .served
            .extend(rows.iter().map(|candidate| candidate.withdrawal_id));
        Ok(rows)
    }
    async fn all_attempts_terminal(&self, _: Uuid) -> Result<bool, WorkerError> {
        Ok(self.state.lock().terminal)
    }
    async fn record_execution(
        &self,
        _: &Deployment,
        c: &Candidate,
        _: &ProviderObservation,
        _: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        self.state.lock().executions.insert(c.withdrawal_id);
        Ok(())
    }
    async fn record_dropped(
        &self,
        _: &Deployment,
        _: &Candidate,
        _: &ProviderObservation,
        _: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        self.state.lock().drops += 1;
        Ok(())
    }
    async fn record_revert(
        &self,
        _: &Deployment,
        c: &Candidate,
        _: &ProviderObservation,
        _: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        self.state.lock().reverts.insert(c.attempt_id);
        Ok(())
    }
    async fn record_replacement(
        &self,
        _: &Deployment,
        _: &Candidate,
        replacement: &Candidate,
        _: chrono::DateTime<Utc>,
        _: &ProviderObservation,
        _: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        self.state
            .lock()
            .replacements
            .insert(replacement.attempt_id);
        Ok(())
    }
    async fn release_permanent_failure(
        &self,
        _: &Deployment,
        _: &Candidate,
        _: &ProviderObservation,
        _: &ProviderObservation,
    ) -> Result<(), WorkerError> {
        self.state.lock().releases += 1;
        Ok(())
    }
    async fn checkpoint(
        &self,
        _: Uuid,
        owner: Uuid,
        last: Option<&Candidate>,
        _: Duration,
    ) -> Result<(), WorkerError> {
        let mut s = self.state.lock();
        if s.lease != Some(owner) {
            return Err(WorkerError::LeaseUnavailable);
        }
        s.checkpoints += 1;
        if let Some(last) = last {
            if let Some(index) = s
                .candidates
                .iter()
                .position(|candidate| candidate.withdrawal_id == last.withdrawal_id)
            {
                s.cursor = index + 1;
            }
        }
        Ok(())
    }
}

fn block(offset: i64) -> FinalizedBlock {
    FinalizedBlock {
        number: 120,
        hash: hash(0xaa),
        timestamp: Utc::now() + ChronoDuration::seconds(offset),
    }
}
fn hash(byte: u8) -> String {
    format!("0x{}", format!("{byte:02x}").repeat(32))
}
fn candidate(expiry_offset: i64) -> Candidate {
    Candidate {
        withdrawal_id: Uuid::new_v4(),
        attempt_id: Uuid::new_v4(),
        transaction_hash: hash(0x55),
        expires_at: Utc::now() + ChronoDuration::seconds(expiry_offset),
        destination: DESTINATION.into(),
        asset: ASSET.into(),
        amount: "1000".into(),
        nonce: "7".into(),
        policy_hash: hash(0x66),
        approver_hash: hash(0x77),
        sender: CUSTOMER.into(),
        sender_nonce: "42".into(),
        calldata_hash: hash(0x88),
        updated_at: Utc::now(),
        current_attempt_terminal: false,
    }
}
fn deployment(code: &[u8]) -> Deployment {
    Deployment {
        id: Uuid::new_v4(),
        chain_id: 84532,
        vault_address: VAULT.into(),
        runtime_code_hash: format!("0x{}", hex::encode(Keccak256::digest(code))),
        event_topic: WITHDRAWAL_EVENT_TOPIC.into(),
        start_block: 1,
        policy_version: "v1".into(),
        min_confirmations: 5,
    }
}
fn config(id: Uuid) -> Config {
    Config {
        enabled: true,
        database_url: "redacted".into(),
        deployment_id: id,
        primary_name: "a".into(),
        primary_url: Url::parse("https://a.example/rpc").unwrap(),
        secondary_name: "b".into(),
        secondary_url: Url::parse("https://b.example/rpc").unwrap(),
        request_timeout: Duration::from_secs(1),
        poll_interval: Duration::from_millis(1),
        retry_max: Duration::from_millis(5),
        lease_duration: Duration::from_secs(30),
        batch_size: 10,
    }
}
fn tx(c: &Candidate) -> TransactionFacts {
    TransactionFacts {
        hash: c.transaction_hash.clone(),
        sender: c.sender.clone(),
        sender_nonce: c.sender_nonce.clone(),
        destination: VAULT.into(),
        calldata_hash: c.calldata_hash.clone(),
    }
}
fn observation(
    name: &str,
    _candidate: &Candidate,
    status: TransactionStatus,
    proof: bool,
    finalized: FinalizedBlock,
) -> ProviderObservation {
    ProviderObservation {
        finalized,
        status,
        request_id_unused: proof,
        withdrawal_nonce_unused: proof,
        no_execution_event: proof,
        provider_name: name.into(),
        provider_reference: format!("{name}-ref"),
    }
}
fn pair(
    c: &Candidate,
    status: TransactionStatus,
    proof: bool,
    finalized: FinalizedBlock,
    code: Vec<u8>,
) -> (FakeProvider, FakeProvider) {
    let a = observation("a", c, status.clone(), proof, finalized.clone());
    let b = observation("b", c, status, proof, finalized.clone());
    (
        FakeProvider {
            name: "a".into(),
            chain: 84532,
            block: finalized.clone(),
            code: code.clone(),
            observation: a,
        },
        FakeProvider {
            name: "b".into(),
            chain: 84532,
            block: finalized,
            code,
            observation: b,
        },
    )
}
async fn worker(
    c: Candidate,
    a: FakeProvider,
    b: FakeProvider,
    terminal: bool,
) -> (
    WithdrawalWorker<FakeStore, FakeProvider, FakeProvider>,
    Arc<Mutex<State>>,
) {
    let d = deployment(&a.code);
    let state = Arc::new(Mutex::new(State {
        candidates: vec![c],
        terminal,
        ..Default::default()
    }));
    let store = FakeStore {
        deployment: d.clone(),
        state: state.clone(),
    };
    (
        WithdrawalWorker::new(config(d.id), store, a, b)
            .await
            .unwrap(),
        state,
    )
}

#[test]
fn disabled_and_same_origin_configuration_is_rejected() {
    let code = vec![1];
    let d = deployment(&code);
    let mut c = config(d.id);
    c.enabled = false;
    assert!(matches!(c.validate(), Err(WorkerError::Disabled)));
    let mut c = config(d.id);
    c.secondary_url = c.primary_url.clone();
    assert!(matches!(c.validate(), Err(WorkerError::Config(_))));
    let mut c = config(d.id);
    c.lease_duration = Duration::from_secs(7);
    assert!(matches!(c.validate(), Err(WorkerError::Config(_))));
    let mut c = config(d.id);
    c.retry_max = Duration::from_nanos(1);
    assert!(matches!(c.validate(), Err(WorkerError::Config(_))));
}

#[tokio::test]
async fn provider_disagreement_fails_closed() {
    let c = candidate(60);
    let code = vec![1, 2];
    let (a, mut b) = pair(
        &c,
        TransactionStatus::Pending(tx(&c)),
        false,
        block(0),
        code,
    );
    b.observation.status = TransactionStatus::NotFound;
    let (w, s) = worker(c, a, b, false).await;
    assert!(matches!(w.tick().await, Err(WorkerError::Safety(_))));
    assert_eq!(s.lock().releases, 0);
}

#[tokio::test]
async fn older_replaced_transaction_cannot_control_current_attempt() {
    let c = candidate(60);
    let code = vec![3];
    let mut old = tx(&c);
    old.hash = hash(0x44);
    let (a, b) = pair(&c, TransactionStatus::Pending(old), false, block(0), code);
    let (w, _) = worker(c, a, b, false).await;
    assert!(matches!(w.tick().await, Err(WorkerError::Safety(_))));
}

#[tokio::test]
async fn no_release_before_finalized_deadline_or_complete_proof() {
    let c = candidate(3600);
    let code = vec![4];
    let (a, b) = pair(&c, TransactionStatus::NotFound, true, block(0), code);
    let (w, s) = worker(c, a, b, true).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Dropped]);
    assert_eq!(s.lock().releases, 0);
    let c = candidate(-3600);
    let code = vec![5];
    let (a, b) = pair(&c, TransactionStatus::NotFound, false, block(0), code);
    let (w, s) = worker(c, a, b, true).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Pending]);
    assert_eq!(s.lock().releases, 0);
}

#[tokio::test]
async fn permanent_release_requires_terminal_attempts() {
    let c = candidate(-3600);
    let code = vec![6];
    let (a, b) = pair(&c, TransactionStatus::NotFound, true, block(0), code);
    let (w, s) = worker(c.clone(), a.clone(), b.clone(), false).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Pending]);
    let (w, s2) = worker(c, a, b, true).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Released]);
    assert_eq!(s.lock().releases, 0);
    assert_eq!(s2.lock().releases, 1);
}

#[tokio::test]
async fn finalized_revert_is_not_misclassified_or_released() {
    let c = candidate(-3600);
    let code = vec![11];
    let finalized = block(0);
    let (a, b) = pair(
        &c,
        TransactionStatus::FinalizedRevert(tx(&c), finalized.clone()),
        false,
        finalized,
        code,
    );
    let (w, state) = worker(c, a, b, true).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Reverted]);
    assert_eq!(state.lock().drops, 0);
    assert_eq!(state.lock().releases, 0);
    assert_eq!(state.lock().reverts.len(), 1);
}

#[tokio::test]
async fn finalized_revert_releases_only_after_expiry_and_complete_unused_proof() {
    let c = candidate(-3600);
    let code = vec![13];
    let finalized = block(0);
    let (a, b) = pair(
        &c,
        TransactionStatus::FinalizedRevert(tx(&c), finalized.clone()),
        true,
        finalized,
        code,
    );
    let (w, state) = worker(c, a, b, true).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Released]);
    assert_eq!(state.lock().reverts.len(), 1);
    assert_eq!(state.lock().releases, 1);
}

#[tokio::test]
async fn supplied_replacement_is_verified_by_both_providers_before_registration() {
    let c = candidate(600);
    let replacement_hash = hash(0x99);
    let code = vec![14];
    let mut replacement_tx = tx(&c);
    replacement_tx.hash = replacement_hash.clone();
    let (a, b) = pair(
        &c,
        TransactionStatus::Pending(replacement_tx),
        false,
        block(0),
        code,
    );
    let (w, state) = worker(c.clone(), a, b, false).await;
    let submitted_at = Utc::now();
    assert_eq!(
        w.ingest_replacement(&c, &replacement_hash, submitted_at)
            .await
            .unwrap(),
        Outcome::ReplacementRecorded
    );
    assert_eq!(
        w.ingest_replacement(&c, &replacement_hash, submitted_at)
            .await
            .unwrap(),
        Outcome::ReplacementRecorded
    );
    assert_eq!(state.lock().replacements.len(), 1);
}

#[tokio::test]
async fn replacement_provider_disagreement_fails_closed() {
    let c = candidate(600);
    let replacement_hash = hash(0x98);
    let code = vec![15];
    let mut replacement_tx = tx(&c);
    replacement_tx.hash = replacement_hash.clone();
    let (a, mut b) = pair(
        &c,
        TransactionStatus::Pending(replacement_tx),
        false,
        block(0),
        code,
    );
    b.observation.status = TransactionStatus::NotFound;
    let (w, state) = worker(c.clone(), a, b, false).await;
    assert!(matches!(
        w.ingest_replacement(&c, &replacement_hash, Utc::now())
            .await,
        Err(WorkerError::Safety(_))
    ));
    assert!(state.lock().replacements.is_empty());
}

#[tokio::test]
async fn replacement_with_wrong_sender_nonce_fails_closed() {
    let c = candidate(600);
    let replacement_hash = hash(0x97);
    let code = vec![16];
    let mut replacement_tx = tx(&c);
    replacement_tx.hash = replacement_hash.clone();
    replacement_tx.sender_nonce = "43".to_owned();
    let (a, b) = pair(
        &c,
        TransactionStatus::Pending(replacement_tx),
        false,
        block(0),
        code,
    );
    let (w, state) = worker(c.clone(), a, b, false).await;
    assert!(matches!(
        w.ingest_replacement(&c, &replacement_hash, Utc::now())
            .await,
        Err(WorkerError::Safety(_))
    ));
    assert!(state.lock().replacements.is_empty());
}

#[tokio::test]
async fn finalized_execution_is_exact_and_restart_idempotent() {
    let c = candidate(-1);
    let code = vec![7];
    let inclusion = FinalizedBlock {
        number: 115,
        hash: hash(0xbb),
        timestamp: c.expires_at - ChronoDuration::seconds(1),
    };
    let log = ExecutionLog {
        block: inclusion,
        transaction_index: 2,
        log_index: 3,
        request_id: c.withdrawal_id,
        destination: c.destination.clone(),
        asset: c.asset.clone(),
        amount: c.amount.clone(),
        nonce: c.nonce.clone(),
        deadline: c.expires_at.timestamp().to_string(),
        policy_hash: c.policy_hash.clone(),
        approver_hash: c.approver_hash.clone(),
    };
    let (a, b) = pair(
        &c,
        TransactionStatus::FinalizedExecution(tx(&c), Box::new(log)),
        false,
        block(0),
        code,
    );
    let (w, s) = worker(c, a, b, false).await;
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Executed]);
    assert_eq!(w.tick().await.unwrap(), vec![Outcome::Executed]);
    assert_eq!(s.lock().executions.len(), 1);
    assert_eq!(s.lock().checkpoints, 2);
}

#[tokio::test]
async fn insufficient_finality_and_startup_hash_disagreement_fail() {
    let c = candidate(60);
    let code = vec![8];
    let (a, b) = pair(
        &c,
        TransactionStatus::Pending(tx(&c)),
        false,
        block(0),
        code.clone(),
    );
    let (w, _) = worker(c.clone(), a, b, false).await;
    let mut shallow = tx(&c);
    shallow.hash = c.transaction_hash.clone();
    // A finalized execution above the agreed finalized height cannot meet the policy.
    let future = FinalizedBlock {
        number: 119,
        hash: hash(9),
        timestamp: Utc::now(),
    };
    let log = ExecutionLog {
        block: future,
        transaction_index: 0,
        log_index: 0,
        request_id: c.withdrawal_id,
        destination: c.destination.clone(),
        asset: c.asset.clone(),
        amount: c.amount.clone(),
        nonce: c.nonce.clone(),
        deadline: c.expires_at.timestamp().to_string(),
        policy_hash: c.policy_hash.clone(),
        approver_hash: c.approver_hash.clone(),
    };
    let _ = w;
    let (mut a, b) = pair(
        &c,
        TransactionStatus::FinalizedExecution(shallow, Box::new(log)),
        false,
        block(0),
        code,
    );
    a.block.hash = hash(0xcc);
    let d = deployment(&a.code);
    let store = FakeStore {
        deployment: d.clone(),
        state: Arc::new(Mutex::new(State::default())),
    };
    assert!(matches!(
        WithdrawalWorker::new(config(d.id), store, a, b).await,
        Err(WorkerError::Safety(_))
    ));
}

#[tokio::test]
async fn execution_below_confirmation_policy_fails_closed() {
    let c = candidate(60);
    let code = vec![12];
    let inclusion = FinalizedBlock {
        number: 118,
        hash: hash(0xbb),
        timestamp: Utc::now(),
    };
    let log = ExecutionLog {
        block: inclusion,
        transaction_index: 0,
        log_index: 0,
        request_id: c.withdrawal_id,
        destination: c.destination.clone(),
        asset: c.asset.clone(),
        amount: c.amount.clone(),
        nonce: c.nonce.clone(),
        deadline: c.expires_at.timestamp().to_string(),
        policy_hash: c.policy_hash.clone(),
        approver_hash: c.approver_hash.clone(),
    };
    let (a, b) = pair(
        &c,
        TransactionStatus::FinalizedExecution(tx(&c), Box::new(log)),
        false,
        block(0),
        code,
    );
    let (worker, state) = worker(c, a, b, false).await;
    assert!(matches!(worker.tick().await, Err(WorkerError::Safety(_))));
    assert!(state.lock().executions.is_empty());
}

#[tokio::test]
async fn shutdown_releases_owned_lease() {
    let c = candidate(60);
    let code = vec![9];
    let (a, b) = pair(
        &c,
        TransactionStatus::Pending(tx(&c)),
        false,
        block(0),
        code,
    );
    let (w, s) = worker(c, a, b, false).await;
    w.tick().await.unwrap();
    assert!(s.lock().lease.is_some());
    w.shutdown().await.unwrap();
    assert!(s.lock().lease.is_none());
}

#[tokio::test]
async fn shutdown_interrupts_poll_delay_and_releases_owned_lease() {
    let c = candidate(60);
    let code = vec![17];
    let (a, b) = pair(
        &c,
        TransactionStatus::Pending(tx(&c)),
        false,
        block(0),
        code,
    );
    let d = deployment(&a.code);
    let mut cfg = config(d.id);
    cfg.poll_interval = Duration::from_secs(60);
    cfg.retry_max = Duration::from_secs(60);
    let state = Arc::new(Mutex::new(State {
        candidates: vec![c],
        ..Default::default()
    }));
    let store = FakeStore {
        deployment: d.clone(),
        state: state.clone(),
    };
    let worker = WithdrawalWorker::new(cfg, store, a, b).await.unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(worker.run_until_shutdown(async {
        stop_rx.await.map_err(|_| {
            WorkerError::Config("test shutdown sender unexpectedly dropped".to_owned())
        })
    }));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state.lock().lease.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(state.lock().lease.is_none());
}

#[tokio::test]
async fn durable_cursor_prevents_batch_starvation_across_restart_cycles() {
    let first = candidate(60);
    let code = vec![10];
    let (a, b) = pair(
        &first,
        TransactionStatus::Pending(tx(&first)),
        false,
        block(0),
        code,
    );
    let d = deployment(&a.code);
    let mut cfg = config(d.id);
    cfg.batch_size = 2;
    let mut candidates = vec![first.clone()];
    for index in 1..5 {
        let mut next = first.clone();
        next.withdrawal_id = Uuid::new_v4();
        next.updated_at = first.updated_at + ChronoDuration::seconds(index);
        candidates.push(next);
    }
    let state = Arc::new(Mutex::new(State {
        candidates,
        ..Default::default()
    }));
    let store = FakeStore {
        deployment: d,
        state: state.clone(),
    };
    let worker = WithdrawalWorker::new(cfg, store, a, b).await.unwrap();
    worker.tick().await.unwrap();
    worker.tick().await.unwrap();
    worker.tick().await.unwrap();
    assert_eq!(state.lock().served.len(), 5);
}

#[sqlx::test(migrations = "../ledger/migrations")]
async fn postgres_v3_registry_and_lease_are_restart_safe(pool: sqlx::PgPool) {
    let deployment_id = Uuid::new_v4();
    sqlx::query("INSERT INTO ledger_chain_deployments (id,chain_id,network,vault_address,deployment_version,runtime_code_hash,deposit_event_signature,deposit_event_topic,finality_policy,start_block) VALUES ($1,84532,'base-sepolia',$2,'v3',$3,'Deposited(bytes32,address,bytes32,address,uint256)',$4,'rpc_finalized_two_provider',1)")
        .bind(deployment_id).bind(VAULT).bind(hash(1)).bind(hash(2)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ledger_withdrawal_contract_capabilities (deployment_id,authorization_version,execution_event_signature,execution_event_topic,configured_by,evidence) VALUES ($1,'eip712_v3','WithdrawalExecuted(bytes16,address,address,uint256,uint256,uint256,bytes32,bytes32)',$2,'test','{}')")
        .bind(deployment_id).bind(WITHDRAWAL_EVENT_TOPIC).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ledger_withdrawal_confirmation_policies (deployment_id,policy_version,min_confirmations,require_finalized,required_provider_count,effective_at,configured_by,evidence) VALUES ($1,'v3-test',5,TRUE,2,transaction_timestamp() - interval '1 second','test','{}')")
        .bind(deployment_id).execute(&pool).await.unwrap();
    let store = PgWorkerStore::from_pool(pool.clone());
    let registered = store.deployment(deployment_id).await.unwrap();
    assert_eq!(registered.event_topic, WITHDRAWAL_EVENT_TOPIC);
    assert_eq!(registered.policy_version, "v3-test");
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    store
        .acquire_lease(deployment_id, a, Duration::from_secs(30))
        .await
        .unwrap();
    assert!(matches!(
        store
            .acquire_lease(deployment_id, b, Duration::from_secs(30))
            .await,
        Err(WorkerError::LeaseUnavailable)
    ));
    store
        .checkpoint(deployment_id, a, None, Duration::from_secs(30))
        .await
        .unwrap();
    store.release_lease(deployment_id, a).await.unwrap();
    store
        .acquire_lease(deployment_id, b, Duration::from_secs(30))
        .await
        .unwrap();
    let cycle: i64 = sqlx::query_scalar(
        "SELECT scan_cycle FROM ledger_withdrawal_worker_checkpoints WHERE deployment_id=$1",
    )
    .bind(deployment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cycle, 1);
}

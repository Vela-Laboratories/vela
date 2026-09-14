use bigdecimal::BigDecimal;
use ledger::{
    DepositChainEvent, DepositContradictionOutcome, DepositState, FinalizeDeposit, JournalLegInput,
    Ledger, LedgerError, NewAccount, NewAsset, ObserveDeposit, PostTransaction,
    ReverseCreditedDeposit, RpcCanonicalContradiction, RpcFinalityObservation,
};
use sqlx::PgPool;
use uuid::Uuid;

const CUSTOMER: &str = "0x0000000000000000000000000000000000000001";
const VAULT: &str = "0x1111111111111111111111111111111111111111";
const SENDER: &str = "0x2222222222222222222222222222222222222222";
const USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";

struct Fixture {
    ledger: Ledger,
    asset_id: Uuid,
    customer_account_id: Uuid,
    pending_account_id: Uuid,
    custody_account_id: Uuid,
    deployment_id: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let ledger = Ledger::from_pool(pool);
        let asset_id = Uuid::new_v4();
        ledger
            .create_asset(&NewAsset {
                id: asset_id,
                code: "USDC".to_owned(),
                decimals: 6,
                chain_id: Some(8453),
                contract_address: Some(USDC.to_owned()),
            })
            .await
            .unwrap();
        let customer_account_id = Uuid::new_v4();
        let pending_account_id = Uuid::new_v4();
        let custody_account_id = Uuid::new_v4();
        for account in [
            NewAccount {
                id: customer_account_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: CUSTOMER.to_owned(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: pending_account_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: CUSTOMER.to_owned(),
                account_type: "customer_pending".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: custody_account_id,
                asset_id,
                owner_type: "custody".to_owned(),
                owner_reference: "custody:base:vault-v1".to_owned(),
                account_type: "custody_vault".to_owned(),
                allow_negative: true,
            },
        ] {
            ledger.create_account(&account).await.unwrap();
        }
        let deployment_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ledger_chain_deployments (
                id, chain_id, network, vault_address, deployment_version,
                runtime_code_hash, deposit_event_signature, deposit_event_topic,
                finality_policy, start_block
            ) VALUES ($1, 8453, 'base', $2, 'v2', $3, $4, $5,
                      'rpc_finalized_two_provider', 1)
            "#,
        )
        .bind(deployment_id)
        .bind(VAULT)
        .bind(hash(0xaa))
        .bind("Deposited(bytes32,address,address,uint256)")
        .bind(hash(0xbb))
        .execute(ledger.pool())
        .await
        .unwrap();
        Self {
            ledger,
            asset_id,
            customer_account_id,
            pending_account_id,
            custody_account_id,
            deployment_id,
        }
    }

    fn observation(&self) -> ObserveDeposit {
        ObserveDeposit {
            deposit_id: Uuid::new_v4(),
            customer_reference: CUSTOMER.to_owned(),
            destination_account_id: self.customer_account_id,
            asset_id: self.asset_id,
            amount: BigDecimal::from(1_500_000_i64),
            event: DepositChainEvent {
                chain_id: 8453,
                block_number: 12_345,
                block_hash: hash(0x11),
                transaction_hash: hash(0x22),
                transaction_index: 7,
                log_index: 3,
                vault_address: VAULT.to_owned(),
                vault_deposit_id: "vault-deposit-42".to_owned(),
                sender_address: SENDER.to_owned(),
                asset_address: Some(USDC.to_owned()),
                actual_amount: BigDecimal::from(1_500_000_i64),
            },
        }
    }

    fn finality(&self, deposit_id: Uuid) -> FinalizeDeposit {
        let observation = RpcFinalityObservation {
            provider: "provider-a".to_owned(),
            chain_id: 8453,
            finalized_block_number: 12_400,
            finalized_block_hash: hash(0x33),
            deposit_block_hash: hash(0x11),
            transaction_hash: hash(0x22),
            log_index: 3,
            vault_address: VAULT.to_owned(),
            vault_deposit_id: "vault-deposit-42".to_owned(),
            sender_address: SENDER.to_owned(),
            beneficiary_reference: CUSTOMER.to_owned(),
            asset_address: Some(USDC.to_owned()),
            actual_amount: BigDecimal::from(1_500_000_i64),
        };
        FinalizeDeposit {
            deposit_id,
            first: observation.clone(),
            second: RpcFinalityObservation {
                provider: "provider-b".to_owned(),
                ..observation
            },
        }
    }

    fn contradiction(&self, deposit_id: Uuid) -> ReverseCreditedDeposit {
        let observation = RpcCanonicalContradiction {
            provider: "provider-a".to_owned(),
            chain_id: 8453,
            finalized_block_number: 12_500,
            finalized_block_hash: hash(0x77),
            deposit_block_number: 12_345,
            stored_block_hash: hash(0x11),
            canonical_block_hash: hash(0x66),
            transaction_hash: hash(0x22),
            log_index: 3,
            canonical_log_matches: false,
        };
        ReverseCreditedDeposit {
            deposit_id,
            deployment_id: self.deployment_id,
            pending_account_id: self.pending_account_id,
            reversal_transaction_id: Uuid::new_v4(),
            halt_id: Uuid::new_v4(),
            first: observation.clone(),
            second: RpcCanonicalContradiction {
                provider: "provider-b".to_owned(),
                ..observation
            },
        }
    }

    async fn credit(&self, observation: &ObserveDeposit) {
        self.ledger.observe_deposit(observation).await.unwrap();
        self.ledger
            .mark_deposit_confirming(observation.deposit_id, 12, 12_360, &hash(0x44))
            .await
            .unwrap();
        self.ledger
            .finalize_deposit(&self.finality(observation.deposit_id))
            .await
            .unwrap();
        self.ledger
            .credit_finalized_deposit(
                observation.deposit_id,
                self.custody_account_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap();
    }
}

fn hash(byte: u8) -> String {
    format!("0x{}", format!("{byte:02x}").repeat(32))
}

#[sqlx::test(migrations = "./migrations")]
async fn observation_is_idempotent_and_canonical_event_is_unique(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let observation = fixture.observation();

    let first = fixture.ledger.observe_deposit(&observation).await.unwrap();
    let retried = fixture.ledger.observe_deposit(&observation).await.unwrap();
    assert_eq!(first.id, retried.id);
    assert_eq!(retried.state, DepositState::Observed);

    let history_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_deposit_state_history WHERE deposit_id = $1",
    )
    .bind(observation.deposit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history_count, 1);

    let mut conflicting = observation.clone();
    conflicting.deposit_id = Uuid::new_v4();
    assert!(matches!(
        fixture.ledger.observe_deposit(&conflicting).await,
        Err(LedgerError::DepositObservationConflict)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_deposit_status_is_scoped_filterable_and_chain_backed(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let observation = fixture.observation();
    fixture.credit(&observation).await;

    let deposits = fixture
        .ledger
        .customer_deposits(&CUSTOMER.to_ascii_uppercase(), None, 25)
        .await
        .unwrap();
    assert_eq!(deposits.len(), 1);
    let status = &deposits[0];
    assert_eq!(status.id, observation.deposit_id);
    assert_eq!(status.asset_code, "USDC");
    assert_eq!(status.asset_decimals, 6);
    assert_eq!(status.amount, BigDecimal::from(1_500_000_i64));
    assert_eq!(status.state, DepositState::Credited);
    assert_eq!(status.chain_id, 8453);
    assert_eq!(status.transaction_hash, hash(0x22));
    assert!(status.credited_at.is_some());
    assert!(status.reversed_at.is_none());

    let uppercase_hash = format!("0x{}", hash(0x22)[2..].to_ascii_uppercase());
    let by_hash = fixture
        .ledger
        .customer_deposits(CUSTOMER, Some(&uppercase_hash), 1)
        .await
        .unwrap();
    assert_eq!(by_hash.len(), 1);
    let absent = fixture
        .ledger
        .customer_deposits(CUSTOMER, Some(&hash(0x99)), 1)
        .await
        .unwrap();
    assert!(absent.is_empty());
    assert!(matches!(
        fixture.ledger.customer_deposits(CUSTOMER, None, 0).await,
        Err(LedgerError::InvalidDeposit(_))
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn finality_requires_two_distinct_agreeing_providers(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let observation = fixture.observation();
    fixture.ledger.observe_deposit(&observation).await.unwrap();
    fixture
        .ledger
        .mark_deposit_confirming(observation.deposit_id, 12, 12_360, &hash(0x44))
        .await
        .unwrap();

    let mut disagreement = fixture.finality(observation.deposit_id);
    disagreement.second.finalized_block_hash = hash(0x55);
    assert!(matches!(
        fixture.ledger.finalize_deposit(&disagreement).await,
        Err(LedgerError::DepositFinalityDisagreement)
    ));
    let mut decoded_log_disagreement = fixture.finality(observation.deposit_id);
    decoded_log_disagreement.second.actual_amount = BigDecimal::from(1_500_001_i64);
    assert!(matches!(
        fixture
            .ledger
            .finalize_deposit(&decoded_log_disagreement)
            .await,
        Err(LedgerError::DepositFinalityDisagreement)
    ));
    assert_eq!(
        fixture
            .ledger
            .deposit(observation.deposit_id)
            .await
            .unwrap()
            .state,
        DepositState::Confirming
    );

    let finalized = fixture
        .ledger
        .finalize_deposit(&fixture.finality(observation.deposit_id))
        .await
        .unwrap();
    assert_eq!(finalized.state, DepositState::Finalized);
}

#[sqlx::test(migrations = "./migrations")]
async fn finalized_credit_is_atomic_balanced_and_idempotent(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let observation = fixture.observation();
    fixture.ledger.observe_deposit(&observation).await.unwrap();
    fixture
        .ledger
        .mark_deposit_confirming(observation.deposit_id, 12, 12_360, &hash(0x44))
        .await
        .unwrap();
    fixture
        .ledger
        .finalize_deposit(&fixture.finality(observation.deposit_id))
        .await
        .unwrap();

    let transaction_id = Uuid::new_v4();
    let credited = fixture
        .ledger
        .credit_finalized_deposit(
            observation.deposit_id,
            fixture.custody_account_id,
            transaction_id,
        )
        .await
        .unwrap();
    assert_eq!(credited.deposit.state, DepositState::Credited);
    assert_eq!(credited.transaction.id, transaction_id);
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.customer_account_id)
            .await
            .unwrap(),
        BigDecimal::from(1_500_000_i64)
    );
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.custody_account_id)
            .await
            .unwrap(),
        BigDecimal::from(-1_500_000_i64)
    );

    let retried = fixture
        .ledger
        .credit_finalized_deposit(
            observation.deposit_id,
            fixture.custody_account_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap();
    assert_eq!(retried.transaction.id, transaction_id);
    let journal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'deposit_credit'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journal_count, 1);
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
async fn deposit_cannot_credit_before_finality_or_skip_database_transitions(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let observation = fixture.observation();
    fixture.ledger.observe_deposit(&observation).await.unwrap();

    assert!(matches!(
        fixture
            .ledger
            .credit_finalized_deposit(
                observation.deposit_id,
                fixture.custody_account_id,
                Uuid::new_v4(),
            )
            .await,
        Err(LedgerError::InvalidDepositTransition { .. })
    ));
    let direct_update = sqlx::query(
        "UPDATE ledger_deposits SET state = 'credited', credit_transaction_id = $2 WHERE id = $1",
    )
    .bind(observation.deposit_id)
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await;
    assert!(direct_update.is_err());
    assert_eq!(
        fixture
            .ledger
            .deposit(observation.deposit_id)
            .await
            .unwrap()
            .state,
        DepositState::Observed
    );

    let missing_history =
        sqlx::query("UPDATE ledger_deposits SET state = 'confirming' WHERE id = $1")
            .bind(observation.deposit_id)
            .execute(&pool)
            .await;
    assert!(missing_history.is_err());
    assert_eq!(
        fixture
            .ledger
            .deposit(observation.deposit_id)
            .await
            .unwrap()
            .state,
        DepositState::Observed
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn orphaned_precredit_deposit_never_reaches_available_balance(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let observation = fixture.observation();
    fixture.ledger.observe_deposit(&observation).await.unwrap();
    let orphaned = fixture
        .ledger
        .orphan_deposit(
            observation.deposit_id,
            "canonical block changed before finality",
            serde_json::json!({"replacement_block_hash": hash(0x66)}),
        )
        .await
        .unwrap();
    assert_eq!(orphaned.state, DepositState::Orphaned);
    assert!(matches!(
        fixture
            .ledger
            .credit_finalized_deposit(
                observation.deposit_id,
                fixture.custody_account_id,
                Uuid::new_v4(),
            )
            .await,
        Err(LedgerError::InvalidDepositTransition { .. })
    ));
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.customer_account_id)
            .await
            .unwrap(),
        BigDecimal::from(0)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn credited_contradiction_halts_asset_and_moves_available_funds_to_pending(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let observation = fixture.observation();
    fixture.credit(&observation).await;

    let command = fixture.contradiction(observation.deposit_id);
    let first = fixture
        .ledger
        .reverse_credited_deposit(&command)
        .await
        .unwrap();
    let transaction_id = match first {
        DepositContradictionOutcome::Reversed {
            deposit,
            transaction,
        } => {
            assert_eq!(deposit.state, DepositState::Reversed);
            assert_eq!(deposit.reversal_transaction_id, Some(transaction.id));
            transaction.id
        }
        DepositContradictionOutcome::Quarantined { .. } => panic!("expected complete reversal"),
    };
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.customer_account_id)
            .await
            .unwrap(),
        BigDecimal::from(0)
    );
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.pending_account_id)
            .await
            .unwrap(),
        BigDecimal::from(1_500_000_i64)
    );
    let active_halts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_asset_halts WHERE resolved_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active_halts, 1);

    let retried = fixture
        .ledger
        .reverse_credited_deposit(&fixture.contradiction(observation.deposit_id))
        .await
        .unwrap();
    assert!(matches!(
        retried,
        DepositContradictionOutcome::Reversed { transaction, .. }
            if transaction.id == transaction_id
    ));
    let reversal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'deposit_reversal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reversal_count, 1);

    for operation_type in ["deposit_credit", "trade", "withdrawal_reservation"] {
        let blocked = fixture
            .ledger
            .post(&PostTransaction {
                id: Uuid::new_v4(),
                operation_type: operation_type.to_owned(),
                idempotency_key: format!("halted-{operation_type}:{}", Uuid::new_v4()),
                request_hash: [9; 32],
                external_reference: None,
                metadata: serde_json::json!({}),
                legs: vec![
                    JournalLegInput {
                        account_id: fixture.pending_account_id,
                        amount: BigDecimal::from(-1),
                    },
                    JournalLegInput {
                        account_id: fixture.customer_account_id,
                        amount: BigDecimal::from(1),
                    },
                ],
            })
            .await;
        assert!(blocked.is_err(), "{operation_type} bypassed the asset halt");
    }
    fixture
        .ledger
        .post(&PostTransaction {
            id: Uuid::new_v4(),
            operation_type: "order_release".to_owned(),
            idempotency_key: format!("safe-release:{}", Uuid::new_v4()),
            request_hash: [7; 32],
            external_reference: None,
            metadata: serde_json::json!({}),
            legs: vec![
                JournalLegInput {
                    account_id: fixture.pending_account_id,
                    amount: BigDecimal::from(-1),
                },
                JournalLegInput {
                    account_id: fixture.customer_account_id,
                    amount: BigDecimal::from(1),
                },
            ],
        })
        .await
        .unwrap();

    let halt_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM ledger_asset_halts WHERE asset_id = $1 AND resolved_at IS NULL",
    )
    .bind(fixture.asset_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(sqlx::query(
        r#"
        UPDATE ledger_asset_halts
        SET resolution_evidence = '{}'::jsonb,
            resolution_initiated_by = 'operator-a',
            resolved_at = transaction_timestamp(),
            resolved_by = 'operator-a'
        WHERE id = $1
        "#,
    )
    .bind(halt_id)
    .execute(&pool)
    .await
    .is_err());
    sqlx::query(
        r#"
        UPDATE ledger_asset_halts
        SET resolution_evidence = '{"reconciliation":"passed"}'::jsonb,
            resolution_initiated_by = 'operator-a',
            resolved_at = transaction_timestamp(),
            resolved_by = 'operator-b'
        WHERE id = $1
        "#,
    )
    .bind(halt_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(sqlx::query("DELETE FROM ledger_asset_halts WHERE id = $1")
        .bind(halt_id)
        .execute(&pool)
        .await
        .is_err());
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
async fn spent_credited_deposit_quarantines_and_isolates_remaining_balance(pool: PgPool) {
    let fixture = Fixture::new(pool.clone()).await;
    let observation = fixture.observation();
    fixture.credit(&observation).await;
    fixture
        .ledger
        .post(&PostTransaction {
            id: Uuid::new_v4(),
            operation_type: "trade".to_owned(),
            idempotency_key: format!("pre-halt-spend:{}", Uuid::new_v4()),
            request_hash: [8; 32],
            external_reference: None,
            metadata: serde_json::json!({}),
            legs: vec![
                JournalLegInput {
                    account_id: fixture.customer_account_id,
                    amount: BigDecimal::from(-1_000_000_i64),
                },
                JournalLegInput {
                    account_id: fixture.custody_account_id,
                    amount: BigDecimal::from(1_000_000_i64),
                },
            ],
        })
        .await
        .unwrap();

    let outcome = fixture
        .ledger
        .reverse_credited_deposit(&fixture.contradiction(observation.deposit_id))
        .await
        .unwrap();
    match outcome {
        DepositContradictionOutcome::Quarantined {
            deposit,
            transaction,
            available_balance,
            shortfall,
        } => {
            assert_eq!(deposit.state, DepositState::Quarantined);
            assert_eq!(
                deposit.reversal_transaction_id,
                transaction.as_ref().map(|posted| posted.id)
            );
            assert!(transaction.is_some());
            assert_eq!(available_balance, BigDecimal::from(500_000_i64));
            assert_eq!(shortfall, BigDecimal::from(1_000_000_i64));
        }
        DepositContradictionOutcome::Reversed { .. } => panic!("expected quarantine"),
    }
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.customer_account_id)
            .await
            .unwrap(),
        BigDecimal::from(0)
    );
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.pending_account_id)
            .await
            .unwrap(),
        BigDecimal::from(500_000_i64)
    );
    let reversal_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ledger_journal_transactions WHERE operation_type = 'deposit_reversal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reversal_count, 1);
    assert!(fixture.ledger.verify_balance_projection().await.unwrap());
}

use bigdecimal::BigDecimal;
use ledger::{
    DepositChainEvent, DepositState, FinalizeDeposit, Ledger, LedgerError, NewAccount, NewAsset,
    ObserveDeposit, RpcFinalityObservation,
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
    custody_account_id: Uuid,
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
        Self {
            ledger,
            asset_id,
            customer_account_id,
            custody_account_id,
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

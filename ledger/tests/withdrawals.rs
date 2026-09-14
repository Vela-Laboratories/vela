use bigdecimal::BigDecimal;
use chrono::Utc;
use ledger::{
    ApproveWithdrawal, JournalLegInput, Ledger, LedgerError, NewAccount, NewAsset, PostTransaction,
    RecordWithdrawalSignature, ReserveWithdrawal, WithdrawalState,
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const CUSTOMER: &str = "0x1111111111111111111111111111111111111111";
const VAULT: &str = "0x2222222222222222222222222222222222222222";

struct Fixture {
    ledger: Ledger,
    available_id: Uuid,
    reserved_id: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool, balance: i64) -> Self {
        let ledger = Ledger::from_pool(pool);
        let asset_id = Uuid::new_v4();
        ledger
            .create_asset(&NewAsset {
                id: asset_id,
                code: "USDC".to_owned(),
                decimals: 6,
                chain_id: Some(84532),
                contract_address: Some("0x3333333333333333333333333333333333333333".to_owned()),
            })
            .await
            .unwrap();
        let available_id = Uuid::new_v4();
        let reserved_id = Uuid::new_v4();
        let operations_id = Uuid::new_v4();
        for account in [
            NewAccount {
                id: available_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: CUSTOMER.to_owned(),
                account_type: "customer_available".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: reserved_id,
                asset_id,
                owner_type: "customer".to_owned(),
                owner_reference: CUSTOMER.to_owned(),
                account_type: "customer_withdrawal_reserved".to_owned(),
                allow_negative: false,
            },
            NewAccount {
                id: Uuid::new_v4(),
                asset_id,
                owner_type: "custody".to_owned(),
                owner_reference: VAULT.to_owned(),
                account_type: "custody_vault".to_owned(),
                allow_negative: true,
            },
            NewAccount {
                id: operations_id,
                asset_id,
                owner_type: "platform".to_owned(),
                owner_reference: "platform:test".to_owned(),
                account_type: "operational_adjustment".to_owned(),
                allow_negative: true,
            },
        ] {
            ledger.create_account(&account).await.unwrap();
        }
        ledger
            .post(&PostTransaction {
                id: Uuid::new_v4(),
                operation_type: "deposit_credit".to_owned(),
                idempotency_key: format!("seed:{available_id}"),
                request_hash: [7; 32],
                external_reference: None,
                metadata: json!({ "test": true }),
                legs: vec![
                    JournalLegInput {
                        account_id: operations_id,
                        amount: BigDecimal::from(-balance),
                    },
                    JournalLegInput {
                        account_id: available_id,
                        amount: BigDecimal::from(balance),
                    },
                ],
            })
            .await
            .unwrap();
        Self {
            ledger,
            available_id,
            reserved_id,
        }
    }

    fn reservation(&self, nonce: u64, amount: i64) -> ReserveWithdrawal {
        ReserveWithdrawal {
            withdrawal_id: Uuid::new_v4(),
            reservation_transaction_id: Uuid::new_v4(),
            customer_reference: CUSTOMER.to_owned(),
            asset_code: "USDC".to_owned(),
            destination_address: CUSTOMER.to_owned(),
            amount: BigDecimal::from(amount),
            nonce,
            expires_at: None,
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn reservation_is_atomic_idempotent_and_customer_scoped(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let command = fixture.reservation(10, 400_000);

    let first = fixture.ledger.reserve_withdrawal(&command).await.unwrap();
    let replay = fixture.ledger.reserve_withdrawal(&command).await.unwrap();
    assert_eq!(first.withdrawal.state, WithdrawalState::Reserved);
    assert_eq!(first.withdrawal.id, replay.withdrawal.id);
    assert_eq!(first.transaction.id, replay.transaction.id);
    assert_eq!(
        fixture.ledger.balance(fixture.available_id).await.unwrap(),
        BigDecimal::from(600_000)
    );
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(400_000)
    );

    let customer = fixture
        .ledger
        .customer_withdrawals(&CUSTOMER.to_ascii_uppercase(), 25)
        .await
        .unwrap();
    assert_eq!(customer.len(), 1);
    assert_eq!(customer[0].vault_address, VAULT);
    assert!(fixture
        .ledger
        .customer_withdrawals("0x9999999999999999999999999999999999999999", 25)
        .await
        .unwrap()
        .is_empty());

    let mut conflict = command.clone();
    conflict.amount = BigDecimal::from(500_000);
    assert!(matches!(
        fixture.ledger.reserve_withdrawal(&conflict).await,
        Err(LedgerError::IdempotencyConflict)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn cancellation_releases_the_reservation_exactly_once(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let command = fixture.reservation(11, 400_000);
    fixture.ledger.reserve_withdrawal(&command).await.unwrap();

    let release_id = Uuid::new_v4();
    let cancelled = fixture
        .ledger
        .cancel_reserved_withdrawal(command.withdrawal_id, CUSTOMER, release_id)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .cancel_reserved_withdrawal(command.withdrawal_id, CUSTOMER, Uuid::new_v4())
        .await
        .unwrap();
    assert_eq!(cancelled.state, WithdrawalState::Cancelled);
    assert_eq!(replay.release_transaction_id, Some(release_id));
    assert_eq!(
        fixture.ledger.balance(fixture.available_id).await.unwrap(),
        BigDecimal::from(1_000_000)
    );
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(0)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_reservations_cannot_overspend(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let ledger = Arc::new(fixture.ledger.clone());
    let left = fixture.reservation(21, 700_000);
    let right = fixture.reservation(22, 700_000);
    let (left_result, right_result) = tokio::join!(
        ledger.reserve_withdrawal(&left),
        ledger.reserve_withdrawal(&right)
    );
    assert_ne!(left_result.is_ok(), right_result.is_ok());
    let error = left_result.err().or_else(|| right_result.err()).unwrap();
    assert!(matches!(error, LedgerError::InsufficientFunds { .. }));
    assert_eq!(
        ledger.balance(fixture.available_id).await.unwrap(),
        BigDecimal::from(300_000)
    );
    assert_eq!(
        ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(700_000)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn database_requires_guarded_identity_and_state_history(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let command = fixture.reservation(31, 100_000);
    fixture.ledger.reserve_withdrawal(&command).await.unwrap();

    let identity_change =
        sqlx::query("UPDATE ledger_withdrawals SET destination_address = $2 WHERE id = $1")
            .bind(command.withdrawal_id)
            .bind("0x9999999999999999999999999999999999999999")
            .execute(fixture.ledger.pool())
            .await;
    assert!(identity_change.is_err());

    let direct_transition =
        sqlx::query("UPDATE ledger_withdrawals SET state = 'approved' WHERE id = $1")
            .bind(command.withdrawal_id)
            .execute(fixture.ledger.pool())
            .await;
    assert!(direct_transition.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn authorization_and_signature_evidence_are_bound_immutable_and_replay_safe(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let reservation = fixture.reservation(41, 250_000);
    fixture
        .ledger
        .reserve_withdrawal(&reservation)
        .await
        .unwrap();

    let approved_at = Utc::now();
    let expires_at = chrono::DateTime::from_timestamp(approved_at.timestamp() + 600, 0).unwrap();
    let approval = ApproveWithdrawal {
        authorization_id: Uuid::new_v4(),
        withdrawal_id: reservation.withdrawal_id,
        policy_version: "withdrawal-policy.v1".to_owned(),
        approved_by: "policy:test".to_owned(),
        approved_at,
        expires_at,
    };
    let (authorization, replay) = tokio::join!(
        fixture.ledger.authorize_withdrawal(&approval),
        fixture.ledger.authorize_withdrawal(&approval)
    );
    let authorization = authorization.unwrap();
    let replay = replay.unwrap();
    assert_eq!(authorization, replay);
    let reservation_replay = fixture
        .ledger
        .reserve_withdrawal(&reservation)
        .await
        .unwrap();
    assert_eq!(
        reservation_replay.withdrawal.state,
        WithdrawalState::Approved
    );
    for expected in [
        reservation.withdrawal_id.to_string(),
        "chain_id=84532".to_owned(),
        format!("vault={VAULT}"),
        format!("customer={CUSTOMER}"),
        "asset=USDC".to_owned(),
        "asset_address=0x3333333333333333333333333333333333333333".to_owned(),
        "amount_atomic=250000".to_owned(),
        "nonce=41".to_owned(),
    ] {
        assert!(authorization.canonical_payload.contains(&expected));
    }

    let begun = fixture
        .ledger
        .begin_withdrawal_signing(reservation.withdrawal_id, authorization.id)
        .await
        .unwrap();
    assert_eq!(begun.digest, authorization.digest);
    let mut signature = [7_u8; 65];
    signature[64] = 1;
    let signed_at = Utc::now();
    let record = RecordWithdrawalSignature {
        signature_id: Uuid::new_v4(),
        withdrawal_id: reservation.withdrawal_id,
        authorization_id: authorization.id,
        authorization_digest: authorization.digest,
        signer_key_id: "test-only-key".to_owned(),
        signer_public_key: vec![2; 33],
        signature,
        signed_at,
    };
    let stored = fixture
        .ledger
        .record_withdrawal_signature(&record)
        .await
        .unwrap();
    let mut replay_record = record.clone();
    replay_record.signature_id = Uuid::new_v4();
    let replayed = fixture
        .ledger
        .record_withdrawal_signature(&replay_record)
        .await
        .unwrap();
    assert_eq!(stored, replayed);
    assert_eq!(
        fixture
            .ledger
            .customer_withdrawals(CUSTOMER, 1)
            .await
            .unwrap()[0]
            .state,
        WithdrawalState::Signed
    );
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(250_000)
    );

    let mutation = sqlx::query(
        "UPDATE ledger_withdrawal_authorizations SET policy_version = 'tampered' WHERE id = $1",
    )
    .bind(authorization.id)
    .execute(fixture.ledger.pool())
    .await;
    assert!(mutation.is_err());

    assert!(matches!(
        fixture
            .ledger
            .cancel_reserved_withdrawal(reservation.withdrawal_id, CUSTOMER, Uuid::new_v4())
            .await,
        Err(LedgerError::InvalidWithdrawalTransition { .. })
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn database_rejects_authorization_that_changes_withdrawal_facts(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let reservation = fixture.reservation(51, 100_000);
    let reserved = fixture
        .ledger
        .reserve_withdrawal(&reservation)
        .await
        .unwrap()
        .withdrawal;
    let approved_at = Utc::now();
    let expires_at = chrono::DateTime::from_timestamp(approved_at.timestamp() + 600, 0).unwrap();

    let mismatched = sqlx::query(
        r#"
        INSERT INTO ledger_withdrawal_authorizations (
            id, withdrawal_id, envelope_version, chain_id, vault_address,
            customer_reference, destination_address, asset_id, asset_address,
            amount, nonce, expires_at, canonical_payload, digest,
            policy_version, approved_by, approved_at
        ) VALUES ($1, $2, 'vela.withdrawal.authorization.v1', $3, $4, $5, $6,
                  $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(reservation.withdrawal_id)
    .bind(1_i64)
    .bind(VAULT)
    .bind(CUSTOMER)
    .bind(CUSTOMER)
    .bind(reserved.asset_id)
    .bind("0x3333333333333333333333333333333333333333")
    .bind(BigDecimal::from(100_000))
    .bind(BigDecimal::from(51))
    .bind(expires_at)
    .bind("tampered-chain")
    .bind(vec![8_u8; 32])
    .bind("withdrawal-policy.v1")
    .bind("policy:test")
    .bind(approved_at)
    .execute(fixture.ledger.pool())
    .await;
    assert!(mismatched.is_err());
    assert_eq!(
        fixture
            .ledger
            .customer_withdrawals(CUSTOMER, 1)
            .await
            .unwrap()[0]
            .state,
        WithdrawalState::Reserved
    );
}

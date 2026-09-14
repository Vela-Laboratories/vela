use bigdecimal::BigDecimal;
use chrono::Utc;
use ledger::{
    ActivateGlobalWithdrawalHalt, ApproveWithdrawal, ComplianceDisposition,
    EvaluateWithdrawalPolicy, FinalizedWithdrawalExecution, FinalizedWithdrawalNonExecution,
    FinalizedWithdrawalRevert, JournalLegInput, Ledger, LedgerError, NewAccount, NewAsset,
    PostTransaction, RecordDroppedWithdrawalAttempt, RecordObservedWithdrawalReplacement,
    RecordWithdrawalBroadcastAttempt, RecordWithdrawalSignature, ReserveWithdrawal,
    ResolveGlobalWithdrawalHalt, WithdrawalAuthorization, WithdrawalPolicyDisposition,
    WithdrawalState,
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const CUSTOMER: &str = "0x1111111111111111111111111111111111111111";
const VAULT: &str = "0x2222222222222222222222222222222222222222";

struct Fixture {
    ledger: Ledger,
    asset_id: Uuid,
    available_id: Uuid,
    reserved_id: Uuid,
    custody_withdrawn_id: Uuid,
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
        let custody_withdrawn_id = Uuid::new_v4();
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
                id: custody_withdrawn_id,
                asset_id,
                owner_type: "custody".to_owned(),
                owner_reference: VAULT.to_owned(),
                account_type: "custody_withdrawn".to_owned(),
                allow_negative: false,
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
            asset_id,
            available_id,
            reserved_id,
            custody_withdrawn_id,
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

    async fn configure_policy(
        &self,
        version: &str,
        single: i64,
        customer_amount: i64,
        customer_count: i64,
        platform_amount: i64,
    ) -> Uuid {
        let deployment_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ledger_chain_deployments (
                id, chain_id, network, vault_address, deployment_version,
                runtime_code_hash, deposit_event_signature, deposit_event_topic,
                finality_policy, start_block
            ) VALUES ($1, 84532, 'base-sepolia', $2, 'test-v2',
                      $3, 'Deposited(bytes32,address,address,uint256)', $4,
                      'rpc_finalized_two_provider', 0)
            "#,
        )
        .bind(deployment_id)
        .bind(VAULT)
        .bind(format!("0x{}", "11".repeat(32)))
        .bind(format!("0x{}", "22".repeat(32)))
        .execute(self.ledger.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_contract_capabilities (
                deployment_id, authorization_version, execution_event_signature,
                execution_event_topic, configured_by, evidence
            ) VALUES ($1, 'eip712_v2',
                      'WithdrawalExecuted(bytes16,address,address,address,uint256,uint256,uint256,bytes32,bytes32)',
                      $2, 'risk:configurer', '{"ticket":"EX-026-test"}')
            "#,
        )
        .bind(deployment_id)
        .bind(format!("0x{}", "33".repeat(32)))
        .execute(self.ledger.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_confirmation_policies (
                deployment_id, policy_version, min_confirmations, require_finalized,
                required_provider_count, effective_at, configured_by, evidence
            ) VALUES ($1, 'confirmation.test-v1', 2, true, 2,
                      transaction_timestamp() - interval '1 second',
                      'risk:configurer', '{"ticket":"EX-026-test"}')
            "#,
        )
        .bind(deployment_id)
        .execute(self.ledger.pool())
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_policy_limits (
                policy_version, asset_id, max_single_amount,
                max_customer_window_amount, max_customer_window_count,
                customer_window_seconds, max_platform_window_amount,
                platform_window_seconds, effective_at, configured_by, evidence
            ) VALUES ($1, $2, $3, $4, $5, 86400, $6, 86400,
                      transaction_timestamp() - interval '1 second',
                      'risk:configurer', '{"ticket":"EX-025-test"}')
            "#,
        )
        .bind(version)
        .bind(self.asset_id)
        .bind(BigDecimal::from(single))
        .bind(BigDecimal::from(customer_amount))
        .bind(customer_count)
        .bind(BigDecimal::from(platform_amount))
        .execute(self.ledger.pool())
        .await
        .unwrap();
        deployment_id
    }

    fn evaluation(
        &self,
        withdrawal_id: Uuid,
        disposition: ComplianceDisposition,
    ) -> EvaluateWithdrawalPolicy {
        EvaluateWithdrawalPolicy {
            decision_id: Uuid::new_v4(),
            withdrawal_id,
            evaluated_by: "policy:test".to_owned(),
            compliance_provider: "test-compliance-adapter".to_owned(),
            compliance_reference: format!("screening:{withdrawal_id}"),
            compliance_disposition: disposition,
        }
    }

    async fn sign_withdrawal(
        &self,
        nonce: u64,
        amount: i64,
        policy_version: &str,
        lifetime_seconds: i64,
    ) -> (ReserveWithdrawal, WithdrawalAuthorization, Uuid) {
        let deployment_id = self
            .configure_policy(policy_version, amount * 2, amount * 3, 10, amount * 10)
            .await;
        let reservation = self.reservation(nonce, amount);
        self.ledger.reserve_withdrawal(&reservation).await.unwrap();
        self.ledger
            .evaluate_withdrawal_policy(
                &self.evaluation(reservation.withdrawal_id, ComplianceDisposition::Clear),
            )
            .await
            .unwrap();
        let approved_at = Utc::now();
        let authorization = self
            .ledger
            .authorize_withdrawal(&ApproveWithdrawal {
                authorization_id: Uuid::new_v4(),
                withdrawal_id: reservation.withdrawal_id,
                policy_version: policy_version.to_owned(),
                approved_by: "policy:test".to_owned(),
                approved_at,
                expires_at: chrono::DateTime::from_timestamp(
                    approved_at.timestamp() + lifetime_seconds,
                    0,
                )
                .unwrap(),
            })
            .await
            .unwrap();
        self.ledger
            .begin_withdrawal_signing(reservation.withdrawal_id, authorization.id)
            .await
            .unwrap();
        let mut signature = [7_u8; 65];
        signature[64] = 1;
        self.ledger
            .record_withdrawal_signature(&RecordWithdrawalSignature {
                signature_id: Uuid::new_v4(),
                withdrawal_id: reservation.withdrawal_id,
                authorization_id: authorization.id,
                authorization_digest: authorization.digest,
                signer_key_id: "test-only-key".to_owned(),
                signer_public_key: vec![2; 33],
                signature,
                signed_at: Utc::now(),
            })
            .await
            .unwrap();
        (reservation, authorization, deployment_id)
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
    fixture
        .configure_policy("withdrawal-policy.v1", 500_000, 750_000, 3, 900_000)
        .await;
    let reservation = fixture.reservation(41, 250_000);
    fixture
        .ledger
        .reserve_withdrawal(&reservation)
        .await
        .unwrap();

    let evaluation = fixture.evaluation(reservation.withdrawal_id, ComplianceDisposition::Clear);
    let decision = fixture
        .ledger
        .evaluate_withdrawal_policy(&evaluation)
        .await
        .unwrap();
    assert_eq!(decision.disposition, WithdrawalPolicyDisposition::Approved);
    assert!(decision.reason_codes.is_empty());

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

#[sqlx::test(migrations = "./migrations")]
async fn platform_policy_limit_fails_closed(pool: PgPool) {
    let fixture = Fixture::new(pool, 3_000_000).await;
    fixture
        .configure_policy("withdrawal-policy.limits", 800_000, 2_000_000, 2, 600_000)
        .await;

    let over_platform = fixture.reservation(61, 700_000);
    fixture
        .ledger
        .reserve_withdrawal(&over_platform)
        .await
        .unwrap();
    let rejected = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(over_platform.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    assert_eq!(rejected.disposition, WithdrawalPolicyDisposition::Rejected);
    assert_eq!(
        rejected.reason_codes,
        vec!["platform_velocity_amount_limit"]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unavailable_compliance_requires_review_and_blocks_authorization(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    fixture
        .configure_policy("withdrawal-policy.compliance", 800_000, 900_000, 3, 900_000)
        .await;
    let review = fixture.reservation(62, 100_000);
    fixture.ledger.reserve_withdrawal(&review).await.unwrap();
    let review_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(review.withdrawal_id, ComplianceDisposition::Unavailable),
        )
        .await
        .unwrap();
    assert_eq!(
        review_decision.disposition,
        WithdrawalPolicyDisposition::Review
    );
    assert!(review_decision
        .reason_codes
        .contains(&"compliance_provider_unavailable".to_owned()));

    let approval = ApproveWithdrawal {
        authorization_id: Uuid::new_v4(),
        withdrawal_id: review.withdrawal_id,
        policy_version: "withdrawal-policy.compliance".to_owned(),
        approved_by: "policy:test".to_owned(),
        approved_at: Utc::now(),
        expires_at: chrono::DateTime::from_timestamp(Utc::now().timestamp() + 600, 0).unwrap(),
    };
    assert!(fixture
        .ledger
        .authorize_withdrawal(&approval)
        .await
        .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn customer_amount_count_and_single_limits_are_durable(pool: PgPool) {
    let fixture = Fixture::new(pool, 3_000_000).await;
    fixture
        .configure_policy(
            "withdrawal-policy.velocity",
            800_000,
            1_000_000,
            2,
            5_000_000,
        )
        .await;
    assert!(sqlx::query(
        "UPDATE ledger_withdrawal_policy_limits SET max_single_amount = 1 WHERE policy_version = 'withdrawal-policy.velocity' AND asset_id = $1",
    )
    .bind(fixture.asset_id)
    .execute(fixture.ledger.pool())
    .await
    .is_err());

    let over_single = fixture.reservation(71, 900_000);
    fixture
        .ledger
        .reserve_withdrawal(&over_single)
        .await
        .unwrap();
    let single_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(over_single.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    assert!(single_decision
        .reason_codes
        .contains(&"asset_single_amount_limit".to_owned()));

    let second = fixture.reservation(72, 200_000);
    fixture.ledger.reserve_withdrawal(&second).await.unwrap();
    let second_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(second.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    assert!(second_decision
        .reason_codes
        .contains(&"customer_velocity_amount_limit".to_owned()));

    let third = fixture.reservation(73, 100_000);
    fixture.ledger.reserve_withdrawal(&third).await.unwrap();
    let evaluation = fixture.evaluation(third.withdrawal_id, ComplianceDisposition::Clear);
    let third_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(&evaluation)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .evaluate_withdrawal_policy(&evaluation)
        .await
        .unwrap();
    assert_eq!(third_decision, replay);
    assert!(third_decision
        .reason_codes
        .contains(&"customer_velocity_count_limit".to_owned()));
}

#[sqlx::test(migrations = "./migrations")]
async fn global_halt_blocks_reservation_and_policy_until_three_party_resolution(pool: PgPool) {
    let fixture = Fixture::new(pool, 2_000_000).await;
    fixture
        .configure_policy(
            "withdrawal-policy.halt",
            1_000_000,
            2_000_000,
            10,
            3_000_000,
        )
        .await;
    let before_halt = fixture.reservation(81, 100_000);
    fixture
        .ledger
        .reserve_withdrawal(&before_halt)
        .await
        .unwrap();
    let approved_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(before_halt.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    assert_eq!(
        approved_decision.disposition,
        WithdrawalPolicyDisposition::Approved
    );
    let approved_at = Utc::now();
    let authorization = fixture
        .ledger
        .authorize_withdrawal(&ApproveWithdrawal {
            authorization_id: Uuid::new_v4(),
            withdrawal_id: before_halt.withdrawal_id,
            policy_version: "withdrawal-policy.halt".to_owned(),
            approved_by: "policy:test".to_owned(),
            approved_at,
            expires_at: chrono::DateTime::from_timestamp(approved_at.timestamp() + 600, 0).unwrap(),
        })
        .await
        .unwrap();
    let policy_candidate = fixture.reservation(82, 100_000);
    fixture
        .ledger
        .reserve_withdrawal(&policy_candidate)
        .await
        .unwrap();

    let halt = ActivateGlobalWithdrawalHalt {
        halt_id: Uuid::new_v4(),
        reason: "operator emergency stop".to_owned(),
        evidence: json!({ "incident": "INC-025" }),
        activated_by: "operator:alice".to_owned(),
    };
    fixture
        .ledger
        .activate_global_withdrawal_halt(&halt)
        .await
        .unwrap();
    fixture
        .ledger
        .activate_global_withdrawal_halt(&halt)
        .await
        .unwrap();
    let halted_decision = fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(policy_candidate.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    assert_eq!(halted_decision.reason_codes, vec!["global_withdrawal_halt"]);
    assert!(fixture
        .ledger
        .begin_withdrawal_signing(before_halt.withdrawal_id, authorization.id)
        .await
        .is_err());
    assert!(fixture
        .ledger
        .reserve_withdrawal(&fixture.reservation(83, 100_000))
        .await
        .is_err());

    assert!(fixture
        .ledger
        .resolve_global_withdrawal_halt(&ResolveGlobalWithdrawalHalt {
            halt_id: halt.halt_id,
            resolution_evidence: json!({ "ticket": "invalid-self-resolution" }),
            resolution_initiated_by: "OPERATOR:ALICE".to_owned(),
            resolved_by: "operator:bob".to_owned(),
        })
        .await
        .is_err());

    fixture
        .ledger
        .resolve_global_withdrawal_halt(&ResolveGlobalWithdrawalHalt {
            halt_id: halt.halt_id,
            resolution_evidence: json!({ "ticket": "INC-025-resolved" }),
            resolution_initiated_by: "operator:bob".to_owned(),
            resolved_by: "operator:carol".to_owned(),
        })
        .await
        .unwrap();
    fixture
        .ledger
        .reserve_withdrawal(&fixture.reservation(84, 100_000))
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn policy_rotation_invalidates_an_unsigned_authorization(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    fixture
        .configure_policy(
            "withdrawal-policy.rotation-v1",
            800_000,
            900_000,
            3,
            900_000,
        )
        .await;
    let reservation = fixture.reservation(91, 100_000);
    fixture
        .ledger
        .reserve_withdrawal(&reservation)
        .await
        .unwrap();
    fixture
        .ledger
        .evaluate_withdrawal_policy(
            &fixture.evaluation(reservation.withdrawal_id, ComplianceDisposition::Clear),
        )
        .await
        .unwrap();
    let approved_at = Utc::now();
    let authorization = fixture
        .ledger
        .authorize_withdrawal(&ApproveWithdrawal {
            authorization_id: Uuid::new_v4(),
            withdrawal_id: reservation.withdrawal_id,
            policy_version: "withdrawal-policy.rotation-v1".to_owned(),
            approved_by: "policy:test".to_owned(),
            approved_at,
            expires_at: chrono::DateTime::from_timestamp(approved_at.timestamp() + 600, 0).unwrap(),
        })
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO ledger_withdrawal_policy_limits (
            policy_version, asset_id, max_single_amount,
            max_customer_window_amount, max_customer_window_count,
            customer_window_seconds, max_platform_window_amount,
            platform_window_seconds, effective_at, configured_by, evidence
        ) VALUES ('withdrawal-policy.rotation-v2', $1, 800000, 900000, 3,
                  86400, 900000, 86400, transaction_timestamp(),
                  'risk:configurer', '{"ticket":"rotate"}')
        "#,
    )
    .bind(fixture.asset_id)
    .execute(fixture.ledger.pool())
    .await
    .unwrap();

    assert!(fixture
        .ledger
        .begin_withdrawal_signing(reservation.withdrawal_id, authorization.id)
        .await
        .is_err());
}

fn broadcast_attempt(
    withdrawal_id: Uuid,
    attempt_id: Uuid,
    transaction_byte: &str,
    replacement_of: Option<Uuid>,
) -> RecordWithdrawalBroadcastAttempt {
    RecordWithdrawalBroadcastAttempt {
        attempt_id,
        withdrawal_id,
        transaction_hash: format!("0x{}", transaction_byte.repeat(32)),
        sender_address: CUSTOMER.to_owned(),
        sender_nonce: 7,
        calldata_hash: [9; 32],
        replacement_of,
        provider_name: "rpc-primary".to_owned(),
        provider_reference: format!("submission:{attempt_id}"),
        submitted_by: "withdrawal-broadcaster".to_owned(),
        submitted_at: Utc::now(),
    }
}

fn observed_replacement(
    withdrawal_id: Uuid,
    attempt_id: Uuid,
    transaction_byte: &str,
    replacement_of: Uuid,
) -> RecordObservedWithdrawalReplacement {
    RecordObservedWithdrawalReplacement {
        attempt_id,
        withdrawal_id,
        transaction_hash: format!("0x{}", transaction_byte.repeat(32)),
        sender_address: CUSTOMER.to_owned(),
        sender_nonce: 7,
        calldata_hash: [9; 32],
        replacement_of,
        primary_provider: "rpc-primary".to_owned(),
        primary_reference: format!("primary:{attempt_id}"),
        secondary_provider: "rpc-secondary".to_owned(),
        secondary_reference: format!("secondary:{attempt_id}"),
        evidence: json!({"primary":"exact_transaction", "secondary":"exact_transaction"}),
        submitted_at: Utc::now(),
        observed_at: Utc::now(),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn broadcast_attempts_are_immutable_replay_safe_and_replacement_ordered(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let (reservation, _, deployment_id) = fixture
        .sign_withdrawal(101, 200_000, "withdrawal-policy.broadcast", 600)
        .await;
    let first = broadcast_attempt(reservation.withdrawal_id, Uuid::new_v4(), "44", None);
    let stored = fixture
        .ledger
        .record_withdrawal_broadcast_attempt(&first)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .record_withdrawal_broadcast_attempt(&first)
        .await
        .unwrap();
    assert_eq!(stored, replay);
    assert_eq!(stored.attempt_number, 1);

    let mut bypass = fixture.ledger.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO ledger_withdrawal_broadcast_attempts (
            id, withdrawal_id, attempt_number, transaction_hash, sender_address,
            sender_nonce, vault_address, calldata_hash, replacement_of,
            provider_name, provider_reference, submitted_by, submitted_at
        ) VALUES ($1, $2, 2, $3, $4, 7, $5, $6, $7,
                  'bypass', 'bypass', 'bypass', transaction_timestamp())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(reservation.withdrawal_id)
    .bind(format!("0x{}", "53".repeat(32)))
    .bind(CUSTOMER)
    .bind(VAULT)
    .bind([9_u8; 32].as_slice())
    .bind(first.attempt_id)
    .execute(&mut *bypass)
    .await
    .unwrap();
    assert!(bypass.commit().await.is_err());

    let mut wrong_nonce = observed_replacement(
        reservation.withdrawal_id,
        Uuid::new_v4(),
        "54",
        first.attempt_id,
    );
    wrong_nonce.sender_nonce = 8;
    assert!(fixture
        .ledger
        .record_observed_withdrawal_replacement(&wrong_nonce)
        .await
        .is_err());

    let replacement_command = observed_replacement(
        reservation.withdrawal_id,
        Uuid::new_v4(),
        "55",
        first.attempt_id,
    );
    let replacement = fixture
        .ledger
        .record_observed_withdrawal_replacement(&replacement_command)
        .await
        .unwrap();
    let replacement_replay = fixture
        .ledger
        .record_observed_withdrawal_replacement(&replacement_command)
        .await
        .unwrap();
    assert_eq!(replacement, replacement_replay);
    let mut conflicting_replay = replacement_command.clone();
    conflicting_replay.secondary_reference = "secondary:conflict".to_owned();
    assert!(fixture
        .ledger
        .record_observed_withdrawal_replacement(&conflicting_replay)
        .await
        .is_err());
    assert_eq!(replacement.attempt_number, 2);
    assert_eq!(replacement.replacement_of, Some(first.attempt_id));
    let dropped = fixture
        .ledger
        .record_dropped_withdrawal_attempt(&RecordDroppedWithdrawalAttempt {
            observation_id: Uuid::new_v4(),
            withdrawal_id: reservation.withdrawal_id,
            deployment_id,
            transaction_hash: replacement.transaction_hash.clone(),
            primary_provider: "rpc-primary".to_owned(),
            secondary_provider: "rpc-secondary".to_owned(),
            evidence: json!({ "primary": "not_found", "secondary": "not_found" }),
        })
        .await
        .unwrap();
    assert_eq!(dropped.state, WithdrawalState::Dropped);
    let retry = observed_replacement(
        reservation.withdrawal_id,
        Uuid::new_v4(),
        "aa",
        replacement.id,
    );
    let retry = fixture
        .ledger
        .record_observed_withdrawal_replacement(&retry)
        .await
        .unwrap();
    assert_eq!(retry.attempt_number, 3);
    assert!(fixture
        .ledger
        .record_dropped_withdrawal_attempt(&RecordDroppedWithdrawalAttempt {
            observation_id: Uuid::new_v4(),
            withdrawal_id: reservation.withdrawal_id,
            deployment_id,
            transaction_hash: replacement.transaction_hash.clone(),
            primary_provider: "rpc-primary".to_owned(),
            secondary_provider: "rpc-secondary".to_owned(),
            evidence: json!({ "stale": true }),
        })
        .await
        .is_err());
    assert_eq!(
        fixture
            .ledger
            .customer_withdrawals(CUSTOMER, 1)
            .await
            .unwrap()[0]
            .state,
        WithdrawalState::Replaced
    );
    assert!(sqlx::query(
        "UPDATE ledger_withdrawal_broadcast_attempts SET provider_reference = 'tampered' WHERE id = $1",
    )
    .bind(first.attempt_id)
    .execute(fixture.ledger.pool())
    .await
    .is_err());
    assert!(sqlx::query(
        "DELETE FROM ledger_withdrawal_replacement_evidence WHERE attempt_id = $1",
    )
    .bind(replacement.id)
    .execute(fixture.ledger.pool())
    .await
    .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn finalized_execution_completes_reserved_funds_even_during_a_halt(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let (reservation, _, deployment_id) = fixture
        .sign_withdrawal(102, 300_000, "withdrawal-policy.execution", 600)
        .await;
    let attempt = broadcast_attempt(reservation.withdrawal_id, Uuid::new_v4(), "66", None);
    fixture
        .ledger
        .record_withdrawal_broadcast_attempt(&attempt)
        .await
        .unwrap();
    fixture
        .ledger
        .activate_global_withdrawal_halt(&ActivateGlobalWithdrawalHalt {
            halt_id: Uuid::new_v4(),
            reason: "execution must still reconcile".to_owned(),
            evidence: json!({ "incident": "INC-026" }),
            activated_by: "operator:alice".to_owned(),
        })
        .await
        .unwrap();
    let command = FinalizedWithdrawalExecution {
        observation_id: Uuid::new_v4(),
        completion_transaction_id: Uuid::new_v4(),
        withdrawal_id: reservation.withdrawal_id,
        deployment_id,
        transaction_hash: attempt.transaction_hash,
        block_number: 1234,
        block_hash: format!("0x{}", "77".repeat(32)),
        block_timestamp: Utc::now(),
        transaction_index: 2,
        log_index: 3,
        confirmations: 2,
        primary_provider: "rpc-primary".to_owned(),
        secondary_provider: "rpc-secondary".to_owned(),
        confirmation_policy_version: "confirmation.test-v1".to_owned(),
        evidence: json!({ "receipt": "exact-event" }),
    };
    let confirmed = fixture
        .ledger
        .confirm_withdrawal_execution(&command)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .confirm_withdrawal_execution(&command)
        .await
        .unwrap();
    assert_eq!(confirmed.state, WithdrawalState::Confirmed);
    assert_eq!(confirmed, replay);
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(0)
    );
    assert_eq!(
        fixture
            .ledger
            .balance(fixture.custody_withdrawn_id)
            .await
            .unwrap(),
        BigDecimal::from(300_000)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn finalized_revert_is_immutable_terminal_evidence_without_releasing_funds(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let (reservation, _, deployment_id) = fixture
        .sign_withdrawal(104, 250_000, "withdrawal-policy.revert", 600)
        .await;
    let attempt = broadcast_attempt(reservation.withdrawal_id, Uuid::new_v4(), "67", None);
    fixture
        .ledger
        .record_withdrawal_broadcast_attempt(&attempt)
        .await
        .unwrap();
    let command = FinalizedWithdrawalRevert {
        observation_id: Uuid::new_v4(),
        withdrawal_id: reservation.withdrawal_id,
        deployment_id,
        transaction_hash: attempt.transaction_hash.clone(),
        block_number: 1234,
        block_hash: format!("0x{}", "78".repeat(32)),
        block_timestamp: Utc::now(),
        confirmations: 2,
        primary_provider: "rpc-primary".to_owned(),
        secondary_provider: "rpc-secondary".to_owned(),
        confirmation_policy_version: "confirmation.test-v1".to_owned(),
        evidence: json!({"primary_receipt_status":"0x0", "secondary_receipt_status":"0x0"}),
    };
    let recorded = fixture
        .ledger
        .record_finalized_withdrawal_revert(&command)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .record_finalized_withdrawal_revert(&command)
        .await
        .unwrap();
    assert_eq!(recorded, replay);
    assert_eq!(recorded.state, WithdrawalState::Broadcast);
    assert_eq!(
        fixture.ledger.balance(fixture.available_id).await.unwrap(),
        BigDecimal::from(750_000)
    );
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(250_000)
    );
    let mut conflict = command;
    conflict.block_hash = format!("0x{}", "79".repeat(32));
    assert!(fixture
        .ledger
        .record_finalized_withdrawal_revert(&conflict)
        .await
        .is_err());
    assert!(
        sqlx::query("DELETE FROM ledger_withdrawal_attempt_reverts WHERE attempt_id = $1",)
            .bind(attempt.attempt_id)
            .execute(fixture.ledger.pool())
            .await
            .is_err()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn reservation_releases_only_after_finalized_two_provider_non_execution(pool: PgPool) {
    let fixture = Fixture::new(pool, 1_000_000).await;
    let (reservation, authorization, deployment_id) = fixture
        .sign_withdrawal(103, 400_000, "withdrawal-policy.expiry", 30)
        .await;
    let attempt = broadcast_attempt(reservation.withdrawal_id, Uuid::new_v4(), "88", None);
    fixture
        .ledger
        .record_withdrawal_broadcast_attempt(&attempt)
        .await
        .unwrap();
    let mut proof = FinalizedWithdrawalNonExecution {
        observation_id: Uuid::new_v4(),
        release_transaction_id: Uuid::new_v4(),
        withdrawal_id: reservation.withdrawal_id,
        deployment_id,
        last_transaction_hash: Some(attempt.transaction_hash.clone()),
        block_number: 1235,
        block_hash: format!("0x{}", "99".repeat(32)),
        block_timestamp: authorization.expires_at + chrono::Duration::seconds(1),
        confirmations: 2,
        primary_provider: "rpc-primary".to_owned(),
        secondary_provider: "rpc-secondary".to_owned(),
        confirmation_policy_version: "confirmation.test-v1".to_owned(),
        request_id_unused: true,
        withdrawal_nonce_unused: true,
        no_execution_event: true,
        all_attempts_terminal: true,
        evidence: json!({ "request_id": "unused", "nonce": "unused" }),
    };
    assert!(fixture
        .ledger
        .release_expired_withdrawal(&proof)
        .await
        .is_err());
    fixture
        .ledger
        .record_dropped_withdrawal_attempt(&RecordDroppedWithdrawalAttempt {
            observation_id: Uuid::new_v4(),
            withdrawal_id: reservation.withdrawal_id,
            deployment_id,
            transaction_hash: attempt.transaction_hash.clone(),
            primary_provider: "rpc-primary".to_owned(),
            secondary_provider: "rpc-secondary".to_owned(),
            evidence: json!({"primary":"not_found", "secondary":"not_found"}),
        })
        .await
        .unwrap();
    proof.secondary_provider = "RPC-PRIMARY".to_owned();
    assert!(fixture
        .ledger
        .release_expired_withdrawal(&proof)
        .await
        .is_err());
    proof.secondary_provider = "rpc-secondary".to_owned();
    let released = fixture
        .ledger
        .release_expired_withdrawal(&proof)
        .await
        .unwrap();
    let replay = fixture
        .ledger
        .release_expired_withdrawal(&proof)
        .await
        .unwrap();
    assert_eq!(released.state, WithdrawalState::BroadcastFailed);
    assert_eq!(released, replay);
    assert_eq!(
        fixture.ledger.balance(fixture.available_id).await.unwrap(),
        BigDecimal::from(1_000_000)
    );
    assert_eq!(
        fixture.ledger.balance(fixture.reserved_id).await.unwrap(),
        BigDecimal::from(0)
    );
}

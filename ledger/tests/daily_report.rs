use bigdecimal::BigDecimal;
use chrono::{Days, Utc};
use ledger::{JournalLegInput, Ledger, NewAccount, NewAsset, PostTransaction};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../ledger/migrations")]
async fn reports_reconstruct_journal_without_trusting_balance_projection(pool: PgPool) {
    let ledger = Ledger::from_pool(pool.clone());
    let asset_id = Uuid::new_v4();
    ledger
        .create_asset(&NewAsset {
            id: asset_id,
            code: "USDC".into(),
            decimals: 6,
            chain_id: Some(84532),
            contract_address: None,
        })
        .await
        .unwrap();
    let customer = Uuid::new_v4();
    let custody = Uuid::new_v4();
    let fees = Uuid::new_v4();
    for (id, owner_type, account_type, allow_negative) in [
        (customer, "customer", "customer_available", false),
        (custody, "custody", "custody_vault", true),
        (fees, "platform", "fee_revenue", false),
    ] {
        ledger
            .create_account(&NewAccount {
                id,
                asset_id,
                owner_type: owner_type.into(),
                owner_reference: format!("test-{owner_type}"),
                account_type: account_type.into(),
                allow_negative,
            })
            .await
            .unwrap();
    }
    for (operation, debit, credit, amount) in [
        ("deposit_credit", custody, customer, 100),
        ("fee", customer, fees, 2),
    ] {
        ledger
            .post(&PostTransaction {
                id: Uuid::new_v4(),
                operation_type: operation.into(),
                idempotency_key: format!("daily:{operation}"),
                request_hash: [1; 32],
                external_reference: None,
                metadata: json!({"test":true}),
                legs: vec![
                    JournalLegInput {
                        account_id: debit,
                        amount: BigDecimal::from(-amount),
                    },
                    JournalLegInput {
                        account_id: credit,
                        amount: BigDecimal::from(amount),
                    },
                ],
            })
            .await
            .unwrap();
    }
    sqlx::query("UPDATE ledger_account_balances SET balance=999 WHERE account_id=$1")
        .bind(customer)
        .execute(&pool)
        .await
        .unwrap();
    let report = ledger
        .daily_reconciliation_report(Utc::now().date_naive())
        .await
        .unwrap();
    assert!(!report.complete_day);
    assert_eq!(report.operations.len(), 2);
    assert_eq!(report.assets[0].closing_customer_liability_atomic, "98");
    assert_eq!(report.assets[0].closing_custody_journal_atomic, "-100");
    assert_eq!(report.assets[0].daily_fee_revenue_atomic, "2");
    assert!(report.assets[0].journal_balanced);
    assert!(report.latest_custody_evidence.is_empty());

    let yesterday = Utc::now()
        .date_naive()
        .checked_sub_days(Days::new(1))
        .unwrap();
    let report = ledger.daily_reconciliation_report(yesterday).await.unwrap();
    assert!(report.complete_day);
    assert!(report.operations.is_empty());
    assert_eq!(report.assets[0].closing_customer_liability_atomic, "0");
}

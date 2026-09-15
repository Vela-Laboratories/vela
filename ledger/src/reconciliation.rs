use super::{Ledger, LedgerError};
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationDeployment {
    pub id: Uuid,
    pub chain_id: i64,
    pub vault_address: String,
    pub deployment_version: String,
    pub runtime_code_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationAsset {
    pub id: Uuid,
    pub code: String,
    pub contract_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCustodyBalance {
    pub asset_id: Uuid,
    pub amount: BigDecimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordReconciliationCycle {
    pub deployment_id: Uuid,
    pub lease_owner: Uuid,
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub vault_address: String,
    pub deployment_version: String,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub input_hash: [u8; 32],
    pub evidence: Value,
    pub balances: Vec<ObservedCustodyBalance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationStatus {
    Balanced,
    Surplus,
    Deficit,
    Indeterminate,
}

impl ReconciliationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Surplus => "surplus",
            Self::Deficit => "deficit",
            Self::Indeterminate => "indeterminate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResult {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub custody_amount: BigDecimal,
    pub liability_amount: BigDecimal,
    pub difference: BigDecimal,
    pub status: ReconciliationStatus,
    pub halt_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReconciliation {
    pub snapshot_id: Uuid,
    pub replayed: bool,
    pub results: Vec<ReconciliationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordReconciliationFailure {
    pub failure_id: Uuid,
    pub deployment_id: Uuid,
    pub reason: String,
    pub evidence: Value,
    pub evidence_hash: [u8; 32],
}

#[derive(Debug, FromRow)]
struct ProjectionRow {
    asset_id: Uuid,
    non_custody: BigDecimal,
    custody_derived: BigDecimal,
    missing_balances: i64,
    negative_customer_balances: i64,
}

impl Ledger {
    pub async fn reconciliation_deployment(
        &self,
        deployment_id: Uuid,
    ) -> Result<(ReconciliationDeployment, Vec<ReconciliationAsset>), LedgerError> {
        let deployment = sqlx::query_as::<_, (Uuid, i64, String, String, String)>(
            r#"
            SELECT id, chain_id, vault_address, deployment_version, runtime_code_hash
            FROM ledger_chain_deployments
            WHERE id = $1 AND enabled
            "#,
        )
        .bind(deployment_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            LedgerError::InvalidReconciliation("deployment is not enabled".to_owned())
        })?;
        let assets = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
            r#"
            SELECT id, code, contract_address
            FROM ledger_assets
            WHERE chain_id = $1
            ORDER BY code
            "#,
        )
        .bind(deployment.1)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| ReconciliationAsset {
            id: row.0,
            code: row.1,
            contract_address: row.2,
        })
        .collect::<Vec<_>>();
        if assets.is_empty() {
            return Err(LedgerError::InvalidReconciliation(
                "deployment has no registered ledger assets".to_owned(),
            ));
        }
        Ok((
            ReconciliationDeployment {
                id: deployment.0,
                chain_id: deployment.1,
                vault_address: deployment.2,
                deployment_version: deployment.3,
                runtime_code_hash: deployment.4,
            },
            assets,
        ))
    }

    pub async fn acquire_reconciliation_lease(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
        duration: Duration,
    ) -> Result<bool, LedgerError> {
        let seconds = i64::try_from(duration.as_secs()).map_err(|_| {
            LedgerError::InvalidReconciliation("lease duration exceeds bigint".to_owned())
        })?;
        let acquired = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO ledger_reconciliation_worker_checkpoints
                (deployment_id, lease_owner, lease_expires_at)
            VALUES ($1, $2, transaction_timestamp() + ($3 * interval '1 second'))
            ON CONFLICT (deployment_id) DO UPDATE
            SET lease_owner = EXCLUDED.lease_owner,
                lease_expires_at = EXCLUDED.lease_expires_at,
                updated_at = transaction_timestamp()
            WHERE ledger_reconciliation_worker_checkpoints.lease_owner = EXCLUDED.lease_owner
               OR ledger_reconciliation_worker_checkpoints.lease_expires_at <= transaction_timestamp()
            RETURNING deployment_id
            "#,
        )
        .bind(deployment_id)
        .bind(owner)
        .bind(seconds)
        .fetch_optional(&self.pool)
        .await?;
        Ok(acquired.is_some())
    }

    pub async fn release_reconciliation_lease(
        &self,
        deployment_id: Uuid,
        owner: Uuid,
    ) -> Result<(), LedgerError> {
        sqlx::query(
            r#"
            UPDATE ledger_reconciliation_worker_checkpoints
            SET lease_owner = NULL, lease_expires_at = NULL,
                updated_at = transaction_timestamp()
            WHERE deployment_id = $1 AND lease_owner = $2
            "#,
        )
        .bind(deployment_id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn reconciliation_checkpoint(
        &self,
        deployment_id: Uuid,
    ) -> Result<Option<(i64, String)>, LedgerError> {
        sqlx::query_as::<_, (i64, String)>(
            r#"
            SELECT last_finalized_block_number, last_finalized_block_hash
            FROM ledger_reconciliation_worker_checkpoints
            WHERE deployment_id = $1 AND last_finalized_block_number IS NOT NULL
            "#,
        )
        .bind(deployment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(LedgerError::from)
    }

    pub async fn record_reconciliation_cycle(
        &self,
        command: &RecordReconciliationCycle,
    ) -> Result<RecordedReconciliation, LedgerError> {
        validate_cycle(command)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;

        let lease_valid: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM ledger_reconciliation_worker_checkpoints
                WHERE deployment_id = $1 AND lease_owner = $2
                  AND lease_expires_at > transaction_timestamp()
            )
            "#,
        )
        .bind(command.deployment_id)
        .bind(command.lease_owner)
        .fetch_one(&mut *tx)
        .await?;
        if !lease_valid {
            return Err(LedgerError::InvalidReconciliation(
                "reconciliation lease is absent or expired".to_owned(),
            ));
        }

        let deployment_matches: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM ledger_chain_deployments
                WHERE id = $1 AND enabled AND chain_id = $2
                  AND lower(vault_address) = lower($3)
                  AND deployment_version = $4
            )
            "#,
        )
        .bind(command.deployment_id)
        .bind(command.chain_id)
        .bind(&command.vault_address)
        .bind(&command.deployment_version)
        .fetch_one(&mut *tx)
        .await?;
        if !deployment_matches {
            return Err(LedgerError::InvalidReconciliation(
                "snapshot does not match the enabled deployment".to_owned(),
            ));
        }

        let projections = sqlx::query_as::<_, ProjectionRow>(
            r#"
            SELECT asset.id AS asset_id,
                   COALESCE(sum(balance.balance) FILTER (
                       WHERE account.owner_type <> 'custody'
                   ), 0) AS non_custody,
                   -COALESCE(sum(balance.balance) FILTER (
                       WHERE account.owner_type = 'custody'
                   ), 0) AS custody_derived,
                   count(*) FILTER (WHERE balance.account_id IS NULL) AS missing_balances,
                   count(*) FILTER (
                       WHERE account.owner_type = 'customer' AND balance.balance < 0
                   ) AS negative_customer_balances
            FROM ledger_assets asset
            LEFT JOIN ledger_accounts account ON account.asset_id = asset.id
            LEFT JOIN ledger_account_balances balance ON balance.account_id = account.id
            WHERE asset.chain_id = $1
            GROUP BY asset.id
            ORDER BY asset.id
            "#,
        )
        .bind(command.chain_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut cycle_hasher = Sha256::new();
        cycle_hasher.update(command.deployment_id.as_bytes());
        cycle_hasher.update(command.chain_id.to_be_bytes());
        cycle_hasher.update(command.block_number.to_be_bytes());
        cycle_hasher.update(command.block_hash.as_bytes());
        cycle_hasher.update(command.block_timestamp.timestamp().to_be_bytes());
        cycle_hasher.update(
            command
                .block_timestamp
                .timestamp_subsec_nanos()
                .to_be_bytes(),
        );
        cycle_hasher.update(command.vault_address.to_ascii_lowercase().as_bytes());
        cycle_hasher.update(command.deployment_version.as_bytes());
        cycle_hasher.update(command.primary_provider.as_bytes());
        cycle_hasher.update(command.secondary_provider.as_bytes());
        cycle_hasher.update(command.input_hash);
        let mut observed_balances = command.balances.iter().collect::<Vec<_>>();
        observed_balances.sort_by_key(|balance| balance.asset_id);
        for balance in observed_balances {
            cycle_hasher.update(balance.asset_id.as_bytes());
            cycle_hasher.update(balance.amount.to_string().as_bytes());
            cycle_hasher.update([0]);
        }
        for projection in &projections {
            cycle_hasher.update(projection.asset_id.as_bytes());
            cycle_hasher.update(projection.non_custody.to_string().as_bytes());
            cycle_hasher.update([0]);
            cycle_hasher.update(projection.custody_derived.to_string().as_bytes());
            cycle_hasher.update([0]);
            cycle_hasher.update(projection.missing_balances.to_be_bytes());
            cycle_hasher.update(projection.negative_customer_balances.to_be_bytes());
        }
        let cycle_hash: [u8; 32] = cycle_hasher.finalize().into();
        let snapshot_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, &cycle_hash);
        if let Some(stored_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM ledger_custody_snapshots WHERE input_hash = $1",
        )
        .bind(cycle_hash.as_slice())
        .fetch_optional(&mut *tx)
        .await?
        {
            if stored_id != snapshot_id {
                return Err(LedgerError::InvalidReconciliation(
                    "reconciliation cycle hash conflicts with stored identity".to_owned(),
                ));
            }
            let results = load_results(&mut tx, snapshot_id).await?;
            tx.commit().await?;
            return Ok(RecordedReconciliation {
                snapshot_id,
                replayed: true,
                results,
            });
        }
        let supplied = command
            .balances
            .iter()
            .map(|balance| (balance.asset_id, balance.amount.clone()))
            .collect::<HashMap<_, _>>();
        let expected = projections
            .iter()
            .map(|row| row.asset_id)
            .collect::<HashSet<_>>();
        if expected.len() != supplied.len() || supplied.keys().any(|id| !expected.contains(id)) {
            return Err(LedgerError::InvalidReconciliation(
                "custody balances must cover every registered chain asset exactly once".to_owned(),
            ));
        }

        sqlx::query(
            r#"
            INSERT INTO ledger_custody_snapshots (
                id, chain_id, block_number, block_hash, vault_address,
                deployment_version, input_hash, captured_at, deployment_id,
                primary_provider, secondary_provider, block_timestamp, evidence
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, transaction_timestamp(), $8,
                $9, $10, $11, $12
            )
            "#,
        )
        .bind(snapshot_id)
        .bind(command.chain_id)
        .bind(command.block_number)
        .bind(&command.block_hash)
        .bind(command.vault_address.to_ascii_lowercase())
        .bind(&command.deployment_version)
        .bind(cycle_hash.as_slice())
        .bind(command.deployment_id)
        .bind(&command.primary_provider)
        .bind(&command.secondary_provider)
        .bind(command.block_timestamp)
        .bind(&command.evidence)
        .execute(&mut *tx)
        .await?;

        let mut results = Vec::with_capacity(projections.len());
        for projection in projections {
            let custody = supplied.get(&projection.asset_id).cloned().ok_or_else(|| {
                LedgerError::InvalidReconciliation(
                    "custody balances omitted a registered chain asset".to_owned(),
                )
            })?;
            let accounting_valid = projection.missing_balances == 0
                && projection.negative_customer_balances == 0
                && projection.non_custody >= BigDecimal::zero()
                && projection.custody_derived >= BigDecimal::zero()
                && projection.non_custody == projection.custody_derived;
            let liability = if projection.non_custody >= BigDecimal::zero() {
                projection.non_custody.clone()
            } else {
                BigDecimal::zero()
            };
            let difference = &custody - &liability;
            let status = if !accounting_valid {
                ReconciliationStatus::Indeterminate
            } else if difference.is_zero() {
                ReconciliationStatus::Balanced
            } else if difference > BigDecimal::zero() {
                ReconciliationStatus::Surplus
            } else {
                ReconciliationStatus::Deficit
            };
            let result_id = deterministic_id(
                "vela.reconciliation.result.v1",
                &[cycle_hash.as_slice(), projection.asset_id.as_bytes()],
            );
            let evidence = json!({
                "asset_id": projection.asset_id,
                "custody_amount": custody.to_string(),
                "liability_amount": liability.to_string(),
                "difference": difference.to_string(),
                "status": status.as_str(),
                "formula": "sum(non-custody account projections) = -sum(custody_vault + custody_withdrawn projections)",
                "non_custody_projection": projection.non_custody.to_string(),
                "custody_side_projection": projection.custody_derived.to_string(),
                "missing_balance_rows": projection.missing_balances,
                "negative_customer_balances": projection.negative_customer_balances,
                "accounting_cross_check_valid": accounting_valid,
                "custody_evidence_hash": hex::encode(command.input_hash),
                "reconciliation_cycle_hash": hex::encode(cycle_hash),
            });
            let evidence_hash = hash_json(&evidence)?;

            sqlx::query(
                "INSERT INTO ledger_custody_snapshot_balances (snapshot_id, asset_id, amount) VALUES ($1, $2, $3)",
            )
            .bind(snapshot_id)
            .bind(projection.asset_id)
            .bind(&custody)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO ledger_reconciliation_results (
                    id, snapshot_id, asset_id, custody_amount, liability_amount,
                    difference, status, evidence, evidence_hash
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(result_id)
            .bind(snapshot_id)
            .bind(projection.asset_id)
            .bind(&custody)
            .bind(&liability)
            .bind(&difference)
            .bind(status.as_str())
            .bind(&evidence)
            .bind(evidence_hash.as_slice())
            .execute(&mut *tx)
            .await?;

            let halt_id = if matches!(
                status,
                ReconciliationStatus::Deficit | ReconciliationStatus::Indeterminate
            ) {
                Some(
                    ensure_reconciliation_halt(
                        &mut tx,
                        command.deployment_id,
                        projection.asset_id,
                        status,
                        result_id,
                        &evidence,
                    )
                    .await?,
                )
            } else {
                None
            };
            if let Some(halt_id) = halt_id {
                sqlx::query(
                    "INSERT INTO ledger_reconciliation_halt_links (result_id, halt_id) VALUES ($1, $2)",
                )
                .bind(result_id)
                .bind(halt_id)
                .execute(&mut *tx)
                .await?;
            }
            results.push(ReconciliationResult {
                id: result_id,
                asset_id: projection.asset_id,
                custody_amount: custody,
                liability_amount: liability,
                difference,
                status,
                halt_id,
            });
        }

        let checkpoint = sqlx::query(
            r#"
            UPDATE ledger_reconciliation_worker_checkpoints
            SET cycle = cycle + 1,
                last_finalized_block_number = $3,
                last_finalized_block_hash = $4,
                last_snapshot_id = $5,
                last_completed_at = transaction_timestamp(),
                updated_at = transaction_timestamp()
            WHERE deployment_id = $1 AND lease_owner = $2
              AND lease_expires_at > transaction_timestamp()
            "#,
        )
        .bind(command.deployment_id)
        .bind(command.lease_owner)
        .bind(command.block_number)
        .bind(&command.block_hash)
        .bind(snapshot_id)
        .execute(&mut *tx)
        .await?;
        if checkpoint.rows_affected() != 1 {
            return Err(LedgerError::InvalidReconciliation(
                "reconciliation lease expired before atomic checkpoint".to_owned(),
            ));
        }
        tx.commit().await?;
        Ok(RecordedReconciliation {
            snapshot_id,
            replayed: false,
            results,
        })
    }

    pub async fn record_reconciliation_failure(
        &self,
        command: &RecordReconciliationFailure,
    ) -> Result<(), LedgerError> {
        if command.failure_id.is_nil()
            || command.deployment_id.is_nil()
            || !command.evidence.is_object()
            || !matches!(
                command.reason.as_str(),
                "provider_chain_mismatch"
                    | "provider_disagreement"
                    | "provider_stale"
                    | "finalized_regression"
                    | "vault_code_mismatch"
                    | "malformed_rpc"
                    | "ledger_indeterminate"
                    | "lease_lost"
            )
        {
            return Err(LedgerError::InvalidReconciliation(
                "reconciliation failure evidence is malformed".to_owned(),
            ));
        }
        if hash_json(&command.evidence)? != command.evidence_hash {
            return Err(LedgerError::InvalidReconciliation(
                "reconciliation failure evidence hash does not match its evidence".to_owned(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO ledger_reconciliation_failures
                (id, deployment_id, reason, evidence, evidence_hash)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (evidence_hash) DO NOTHING
            "#,
        )
        .bind(command.failure_id)
        .bind(command.deployment_id)
        .bind(&command.reason)
        .bind(&command.evidence)
        .bind(command.evidence_hash.as_slice())
        .execute(&mut *tx)
        .await?;
        let asset_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT asset.id FROM ledger_assets asset
            JOIN ledger_chain_deployments deployment ON deployment.chain_id = asset.chain_id
            WHERE deployment.id = $1 AND deployment.enabled
            ORDER BY asset.id
            "#,
        )
        .bind(command.deployment_id)
        .fetch_all(&mut *tx)
        .await?;
        if asset_ids.is_empty() {
            return Err(LedgerError::InvalidReconciliation(
                "reconciliation failure has no registered assets to halt".to_owned(),
            ));
        }
        for asset_id in asset_ids {
            let halt_id = ensure_reconciliation_halt(
                &mut tx,
                command.deployment_id,
                asset_id,
                ReconciliationStatus::Indeterminate,
                command.failure_id,
                &command.evidence,
            )
            .await?;
            sqlx::query(
                r#"
                INSERT INTO ledger_reconciliation_failure_halts (failure_id, asset_id, halt_id)
                VALUES ($1, $2, $3)
                ON CONFLICT (failure_id, asset_id) DO NOTHING
                "#,
            )
            .bind(command.failure_id)
            .bind(asset_id)
            .bind(halt_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn ensure_reconciliation_halt(
    tx: &mut Transaction<'_, Postgres>,
    deployment_id: Uuid,
    asset_id: Uuid,
    status: ReconciliationStatus,
    evidence_id: Uuid,
    evidence: &Value,
) -> Result<Uuid, LedgerError> {
    let reason = if status == ReconciliationStatus::Deficit {
        "reconciliation_deficit"
    } else {
        "reconciliation_indeterminate"
    };
    let halt_id = deterministic_id(
        "vela.reconciliation.halt.v1",
        &[asset_id.as_bytes(), evidence_id.as_bytes()],
    );
    sqlx::query(
        r#"
        INSERT INTO ledger_asset_halts (
            id, asset_id, deployment_id, reason, evidence,
            deposit_credits_halted, trading_halted, withdrawals_halted
        ) VALUES ($1, $2, $3, $4, $5, true, true, true)
        ON CONFLICT (asset_id) WHERE resolved_at IS NULL DO NOTHING
        "#,
    )
    .bind(halt_id)
    .bind(asset_id)
    .bind(deployment_id)
    .bind(reason)
    .bind(json!({"reconciliation_evidence_id": evidence_id, "evidence": evidence}))
    .execute(&mut **tx)
    .await?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM ledger_asset_halts WHERE asset_id = $1 AND resolved_at IS NULL",
    )
    .bind(asset_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(LedgerError::from)
}

async fn load_results(
    tx: &mut Transaction<'_, Postgres>,
    snapshot_id: Uuid,
) -> Result<Vec<ReconciliationResult>, LedgerError> {
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            BigDecimal,
            BigDecimal,
            BigDecimal,
            String,
            Option<Uuid>,
        ),
    >(
        r#"
        SELECT result.id, result.asset_id, result.custody_amount,
               result.liability_amount, result.difference, result.status, link.halt_id
        FROM ledger_reconciliation_results result
        LEFT JOIN ledger_reconciliation_halt_links link ON link.result_id = result.id
        WHERE result.snapshot_id = $1
        ORDER BY result.asset_id
        "#,
    )
    .bind(snapshot_id)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            let status = match row.5.as_str() {
                "balanced" => ReconciliationStatus::Balanced,
                "surplus" => ReconciliationStatus::Surplus,
                "deficit" => ReconciliationStatus::Deficit,
                "indeterminate" => ReconciliationStatus::Indeterminate,
                other => {
                    return Err(LedgerError::InvalidReconciliation(format!(
                        "unknown stored reconciliation status {other}"
                    )))
                }
            };
            Ok(ReconciliationResult {
                id: row.0,
                asset_id: row.1,
                custody_amount: row.2,
                liability_amount: row.3,
                difference: row.4,
                status,
                halt_id: row.6,
            })
        })
        .collect()
}

fn validate_cycle(command: &RecordReconciliationCycle) -> Result<(), LedgerError> {
    let ids = command
        .balances
        .iter()
        .map(|balance| balance.asset_id)
        .collect::<HashSet<_>>();
    if command.deployment_id.is_nil()
        || command.lease_owner.is_nil()
        || command.chain_id <= 0
        || command.block_number < 0
        || !is_hash(&command.block_hash)
        || command.vault_address.len() != 42
        || !command.vault_address.starts_with("0x")
        || command.deployment_version.trim().is_empty()
        || command.primary_provider.trim().is_empty()
        || command.secondary_provider.trim().is_empty()
        || command
            .primary_provider
            .eq_ignore_ascii_case(&command.secondary_provider)
        || !command.evidence.is_object()
        || command.balances.is_empty()
        || ids.len() != command.balances.len()
        || command
            .balances
            .iter()
            .any(|balance| balance.amount < BigDecimal::zero())
    {
        return Err(LedgerError::InvalidReconciliation(
            "reconciliation cycle identity or evidence is malformed".to_owned(),
        ));
    }
    if hash_json(&command.evidence)? != command.input_hash {
        return Err(LedgerError::InvalidReconciliation(
            "custody input hash must derive from its canonical evidence".to_owned(),
        ));
    }
    Ok(())
}

fn is_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn hash_json(value: &Value) -> Result<[u8; 32], LedgerError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| LedgerError::InvalidReconciliation("could not encode evidence".to_owned()))?;
    Ok(Sha256::digest(bytes).into())
}

fn deterministic_id(namespace: &str, parts: &[&[u8]]) -> Uuid {
    let mut bytes = namespace.as_bytes().to_vec();
    for part in parts {
        bytes.extend_from_slice(part);
    }
    Uuid::new_v5(&Uuid::NAMESPACE_OID, &bytes)
}

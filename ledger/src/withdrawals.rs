use std::str::FromStr;
use std::time::Duration;

use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::{
    is_retryable_transaction_error, post_command_in_transaction, validate_command, JournalLegInput,
    Ledger, LedgerError, PostTransaction, PostedTransaction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalState {
    Requested,
    Reserved,
    Approved,
    Signing,
    Signed,
    Broadcast,
    Confirmed,
    Rejected,
    Cancelled,
    SigningFailed,
    BroadcastFailed,
    Replaced,
    Dropped,
    Reversed,
}

impl WithdrawalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Reserved => "reserved",
            Self::Approved => "approved",
            Self::Signing => "signing",
            Self::Signed => "signed",
            Self::Broadcast => "broadcast",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::SigningFailed => "signing_failed",
            Self::BroadcastFailed => "broadcast_failed",
            Self::Replaced => "replaced",
            Self::Dropped => "dropped",
            Self::Reversed => "reversed",
        }
    }
}

impl FromStr for WithdrawalState {
    type Err = LedgerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "requested" => Ok(Self::Requested),
            "reserved" => Ok(Self::Reserved),
            "approved" => Ok(Self::Approved),
            "signing" => Ok(Self::Signing),
            "signed" => Ok(Self::Signed),
            "broadcast" => Ok(Self::Broadcast),
            "confirmed" => Ok(Self::Confirmed),
            "rejected" => Ok(Self::Rejected),
            "cancelled" => Ok(Self::Cancelled),
            "signing_failed" => Ok(Self::SigningFailed),
            "broadcast_failed" => Ok(Self::BroadcastFailed),
            "replaced" => Ok(Self::Replaced),
            "dropped" => Ok(Self::Dropped),
            "reversed" => Ok(Self::Reversed),
            other => Err(LedgerError::InvalidWithdrawal(format!(
                "unknown withdrawal state {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReserveWithdrawal {
    pub withdrawal_id: Uuid,
    pub reservation_transaction_id: Uuid,
    pub customer_reference: String,
    pub asset_code: String,
    pub destination_address: String,
    pub amount: BigDecimal,
    pub nonce: u64,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalRecord {
    pub id: Uuid,
    pub customer_reference: String,
    pub source_account_id: Uuid,
    pub asset_id: Uuid,
    pub asset_code: String,
    pub asset_decimals: i16,
    pub destination_address: String,
    pub amount: BigDecimal,
    pub state: WithdrawalState,
    pub reservation_transaction_id: Option<Uuid>,
    pub completion_transaction_id: Option<Uuid>,
    pub release_transaction_id: Option<Uuid>,
    pub chain_id: i64,
    pub vault_address: String,
    pub nonce: Option<BigDecimal>,
    pub expires_at: Option<DateTime<Utc>>,
    pub transaction_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedWithdrawal {
    pub withdrawal: WithdrawalRecord,
    pub transaction: PostedTransaction,
}

#[derive(Debug, Clone)]
pub struct ApproveWithdrawal {
    pub authorization_id: Uuid,
    pub withdrawal_id: Uuid,
    pub policy_version: String,
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceDisposition {
    Clear,
    Reject,
    Review,
    Unavailable,
}

impl ComplianceDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Reject => "reject",
            Self::Review => "review",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalPolicyDisposition {
    Approved,
    Rejected,
    Review,
}

impl WithdrawalPolicyDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Review => "review",
        }
    }
}

impl FromStr for WithdrawalPolicyDisposition {
    type Err = LedgerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "review" => Ok(Self::Review),
            other => Err(LedgerError::InvalidWithdrawal(format!(
                "unknown withdrawal policy disposition {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvaluateWithdrawalPolicy {
    pub decision_id: Uuid,
    pub withdrawal_id: Uuid,
    pub evaluated_by: String,
    pub compliance_provider: String,
    pub compliance_reference: String,
    pub compliance_disposition: ComplianceDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalPolicyDecision {
    pub id: Uuid,
    pub withdrawal_id: Uuid,
    pub policy_version: String,
    pub disposition: WithdrawalPolicyDisposition,
    pub compliance_provider: String,
    pub compliance_reference: String,
    pub compliance_disposition: ComplianceDisposition,
    pub reason_codes: Vec<String>,
    pub evidence: serde_json::Value,
    pub evidence_hash: [u8; 32],
    pub evaluated_by: String,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ActivateGlobalWithdrawalHalt {
    pub halt_id: Uuid,
    pub reason: String,
    pub evidence: serde_json::Value,
    pub activated_by: String,
}

#[derive(Debug, Clone)]
pub struct ResolveGlobalWithdrawalHalt {
    pub halt_id: Uuid,
    pub resolution_evidence: serde_json::Value,
    pub resolution_initiated_by: String,
    pub resolved_by: String,
}

#[derive(Debug, Clone)]
pub struct RecordWithdrawalBroadcastAttempt {
    pub attempt_id: Uuid,
    pub withdrawal_id: Uuid,
    pub transaction_hash: String,
    pub sender_address: String,
    pub sender_nonce: u64,
    pub calldata_hash: [u8; 32],
    pub replacement_of: Option<Uuid>,
    pub provider_name: String,
    pub provider_reference: String,
    pub submitted_by: String,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalBroadcastAttempt {
    pub id: Uuid,
    pub withdrawal_id: Uuid,
    pub attempt_number: i64,
    pub transaction_hash: String,
    pub sender_address: String,
    pub sender_nonce: BigDecimal,
    pub vault_address: String,
    pub calldata_hash: [u8; 32],
    pub replacement_of: Option<Uuid>,
    pub provider_name: String,
    pub provider_reference: String,
    pub submitted_by: String,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RecordDroppedWithdrawalAttempt {
    pub observation_id: Uuid,
    pub withdrawal_id: Uuid,
    pub deployment_id: Uuid,
    pub transaction_hash: String,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct FinalizedWithdrawalExecution {
    pub observation_id: Uuid,
    pub completion_transaction_id: Uuid,
    pub withdrawal_id: Uuid,
    pub deployment_id: Uuid,
    pub transaction_hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub transaction_index: i32,
    pub log_index: i32,
    pub confirmations: i64,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub confirmation_policy_version: String,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct FinalizedWithdrawalNonExecution {
    pub observation_id: Uuid,
    pub release_transaction_id: Uuid,
    pub withdrawal_id: Uuid,
    pub deployment_id: Uuid,
    pub last_transaction_hash: Option<String>,
    pub block_number: i64,
    pub block_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub confirmations: i64,
    pub primary_provider: String,
    pub secondary_provider: String,
    pub confirmation_policy_version: String,
    pub request_id_unused: bool,
    pub withdrawal_nonce_unused: bool,
    pub no_execution_event: bool,
    pub all_attempts_terminal: bool,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalAuthorization {
    pub id: Uuid,
    pub withdrawal_id: Uuid,
    pub envelope_version: String,
    pub chain_id: i64,
    pub vault_address: String,
    pub customer_reference: String,
    pub destination_address: String,
    pub asset_id: Uuid,
    pub asset_code: String,
    pub asset_address: String,
    pub amount: BigDecimal,
    pub nonce: BigDecimal,
    pub expires_at: DateTime<Utc>,
    pub canonical_payload: String,
    pub digest: [u8; 32],
    pub domain_separator: [u8; 32],
    pub struct_hash: [u8; 32],
    pub policy_hash: [u8; 32],
    pub approver_hash: [u8; 32],
    pub policy_version: String,
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RecordWithdrawalSignature {
    pub signature_id: Uuid,
    pub withdrawal_id: Uuid,
    pub authorization_id: Uuid,
    pub authorization_digest: [u8; 32],
    pub signer_key_id: String,
    pub signer_public_key: Vec<u8>,
    pub signature: [u8; 65],
    pub signed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalSignatureRecord {
    pub id: Uuid,
    pub withdrawal_id: Uuid,
    pub authorization_id: Uuid,
    pub authorization_digest: [u8; 32],
    pub signer_key_id: String,
    pub signer_public_key: Vec<u8>,
    pub algorithm: String,
    pub signature: [u8; 65],
    pub signed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawal {
    id: Uuid,
    customer_reference: String,
    source_account_id: Uuid,
    asset_id: Uuid,
    asset_code: String,
    asset_decimals: i16,
    asset_contract_address: Option<String>,
    destination_address: String,
    amount: BigDecimal,
    state: String,
    reservation_transaction_id: Option<Uuid>,
    completion_transaction_id: Option<Uuid>,
    release_transaction_id: Option<Uuid>,
    chain_id: i64,
    vault_address: String,
    nonce: Option<BigDecimal>,
    expires_at: Option<DateTime<Utc>>,
    transaction_hash: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawalAuthorization {
    id: Uuid,
    withdrawal_id: Uuid,
    envelope_version: String,
    chain_id: i64,
    vault_address: String,
    customer_reference: String,
    destination_address: String,
    asset_id: Uuid,
    asset_code: String,
    asset_address: String,
    amount: BigDecimal,
    nonce: BigDecimal,
    expires_at: DateTime<Utc>,
    canonical_payload: String,
    digest: Vec<u8>,
    domain_separator: Option<Vec<u8>>,
    struct_hash: Option<Vec<u8>>,
    policy_hash: Option<Vec<u8>>,
    approver_hash: Option<Vec<u8>>,
    policy_version: String,
    approved_by: String,
    approved_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawalSignature {
    id: Uuid,
    withdrawal_id: Uuid,
    authorization_id: Uuid,
    authorization_digest: Vec<u8>,
    signer_key_id: String,
    signer_public_key: Vec<u8>,
    algorithm: String,
    signature: Vec<u8>,
    signed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawalPolicyLimit {
    policy_version: String,
    max_single_amount: BigDecimal,
    max_customer_window_amount: BigDecimal,
    max_customer_window_count: i64,
    customer_window_seconds: i64,
    max_platform_window_amount: BigDecimal,
    platform_window_seconds: i64,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawalPolicyDecision {
    id: Uuid,
    withdrawal_id: Uuid,
    policy_version: String,
    disposition: String,
    compliance_provider: String,
    compliance_reference: String,
    compliance_disposition: String,
    reason_codes: serde_json::Value,
    evidence: serde_json::Value,
    evidence_hash: Vec<u8>,
    evaluated_by: String,
    evaluated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct StoredWithdrawalBroadcastAttempt {
    id: Uuid,
    withdrawal_id: Uuid,
    attempt_number: i64,
    transaction_hash: String,
    sender_address: String,
    sender_nonce: BigDecimal,
    vault_address: String,
    calldata_hash: Vec<u8>,
    replacement_of: Option<Uuid>,
    provider_name: String,
    provider_reference: String,
    submitted_by: String,
    submitted_at: DateTime<Utc>,
}

impl TryFrom<StoredWithdrawalAuthorization> for WithdrawalAuthorization {
    type Error = LedgerError;

    fn try_from(value: StoredWithdrawalAuthorization) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            withdrawal_id: value.withdrawal_id,
            envelope_version: value.envelope_version,
            chain_id: value.chain_id,
            vault_address: value.vault_address,
            customer_reference: value.customer_reference,
            destination_address: value.destination_address,
            asset_id: value.asset_id,
            asset_code: value.asset_code,
            asset_address: value.asset_address,
            amount: value.amount,
            nonce: value.nonce,
            expires_at: value.expires_at,
            canonical_payload: value.canonical_payload,
            digest: fixed_bytes(value.digest, "withdrawal authorization digest")?,
            domain_separator: fixed_bytes(
                value.domain_separator.ok_or_else(|| {
                    LedgerError::InvalidWithdrawal(
                        "withdrawal authorization has no EIP-712 domain separator".to_owned(),
                    )
                })?,
                "withdrawal authorization domain separator",
            )?,
            struct_hash: fixed_bytes(
                value.struct_hash.ok_or_else(|| {
                    LedgerError::InvalidWithdrawal(
                        "withdrawal authorization has no EIP-712 struct hash".to_owned(),
                    )
                })?,
                "withdrawal authorization struct hash",
            )?,
            policy_hash: fixed_bytes(
                value.policy_hash.ok_or_else(|| {
                    LedgerError::InvalidWithdrawal(
                        "withdrawal authorization has no policy hash".to_owned(),
                    )
                })?,
                "withdrawal authorization policy hash",
            )?,
            approver_hash: fixed_bytes(
                value.approver_hash.ok_or_else(|| {
                    LedgerError::InvalidWithdrawal(
                        "withdrawal authorization has no approver hash".to_owned(),
                    )
                })?,
                "withdrawal authorization approver hash",
            )?,
            policy_version: value.policy_version,
            approved_by: value.approved_by,
            approved_at: value.approved_at,
        })
    }
}

impl TryFrom<StoredWithdrawalSignature> for WithdrawalSignatureRecord {
    type Error = LedgerError;

    fn try_from(value: StoredWithdrawalSignature) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            withdrawal_id: value.withdrawal_id,
            authorization_id: value.authorization_id,
            authorization_digest: fixed_bytes(
                value.authorization_digest,
                "withdrawal signature digest",
            )?,
            signer_key_id: value.signer_key_id,
            signer_public_key: value.signer_public_key,
            algorithm: value.algorithm,
            signature: fixed_bytes(value.signature, "withdrawal signature")?,
            signed_at: value.signed_at,
        })
    }
}

impl TryFrom<StoredWithdrawalPolicyDecision> for WithdrawalPolicyDecision {
    type Error = LedgerError;

    fn try_from(value: StoredWithdrawalPolicyDecision) -> Result<Self, Self::Error> {
        let reason_codes =
            serde_json::from_value::<Vec<String>>(value.reason_codes).map_err(|_| {
                LedgerError::InvalidWithdrawal(
                    "stored withdrawal policy reason codes are malformed".to_owned(),
                )
            })?;
        let compliance_disposition = match value.compliance_disposition.as_str() {
            "clear" => ComplianceDisposition::Clear,
            "reject" => ComplianceDisposition::Reject,
            "review" => ComplianceDisposition::Review,
            "unavailable" => ComplianceDisposition::Unavailable,
            other => {
                return Err(LedgerError::InvalidWithdrawal(format!(
                    "unknown compliance disposition {other}"
                )))
            }
        };
        Ok(Self {
            id: value.id,
            withdrawal_id: value.withdrawal_id,
            policy_version: value.policy_version,
            disposition: WithdrawalPolicyDisposition::from_str(&value.disposition)?,
            compliance_provider: value.compliance_provider,
            compliance_reference: value.compliance_reference,
            compliance_disposition,
            reason_codes,
            evidence: value.evidence,
            evidence_hash: fixed_bytes(value.evidence_hash, "withdrawal policy evidence hash")?,
            evaluated_by: value.evaluated_by,
            evaluated_at: value.evaluated_at,
        })
    }
}

impl TryFrom<StoredWithdrawalBroadcastAttempt> for WithdrawalBroadcastAttempt {
    type Error = LedgerError;

    fn try_from(value: StoredWithdrawalBroadcastAttempt) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            withdrawal_id: value.withdrawal_id,
            attempt_number: value.attempt_number,
            transaction_hash: value.transaction_hash,
            sender_address: value.sender_address,
            sender_nonce: value.sender_nonce,
            vault_address: value.vault_address,
            calldata_hash: fixed_bytes(value.calldata_hash, "withdrawal calldata hash")?,
            replacement_of: value.replacement_of,
            provider_name: value.provider_name,
            provider_reference: value.provider_reference,
            submitted_by: value.submitted_by,
            submitted_at: value.submitted_at,
        })
    }
}

impl TryFrom<StoredWithdrawal> for WithdrawalRecord {
    type Error = LedgerError;

    fn try_from(value: StoredWithdrawal) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            customer_reference: value.customer_reference,
            source_account_id: value.source_account_id,
            asset_id: value.asset_id,
            asset_code: value.asset_code,
            asset_decimals: value.asset_decimals,
            destination_address: value.destination_address,
            amount: value.amount,
            state: WithdrawalState::from_str(&value.state)?,
            reservation_transaction_id: value.reservation_transaction_id,
            completion_transaction_id: value.completion_transaction_id,
            release_transaction_id: value.release_transaction_id,
            chain_id: value.chain_id,
            vault_address: value.vault_address,
            nonce: value.nonce,
            expires_at: value.expires_at,
            transaction_hash: value.transaction_hash,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl Ledger {
    pub async fn reserve_withdrawal(
        &self,
        command: &ReserveWithdrawal,
    ) -> Result<ReservedWithdrawal, LedgerError> {
        validate_reservation(command)?;
        let mut attempt = 0_u32;
        loop {
            match self.reserve_withdrawal_once(command).await {
                Err(error)
                    if (is_retryable_transaction_error(&error)
                        || is_withdrawal_nonce_race(&error))
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn reserve_withdrawal_once(
        &self,
        command: &ReserveWithdrawal,
    ) -> Result<ReservedWithdrawal, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;

        if let Some(existing) = load_existing_reservation(&mut tx, command).await? {
            let request_hash = reservation_hash(command);
            ensure_same_reservation(&existing, command)?;
            let transaction_id = existing.reservation_transaction_id.ok_or_else(|| {
                LedgerError::InvalidWithdrawal(
                    "reserved withdrawal has no journal transaction".to_owned(),
                )
            })?;
            let (transaction, stored_hash) =
                load_posted_transaction(&mut tx, transaction_id).await?;
            if stored_hash != request_hash {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return Ok(ReservedWithdrawal {
                withdrawal: existing.try_into()?,
                transaction,
            });
        }

        let asset = sqlx::query_as::<_, (Uuid, i16, Option<i64>)>(
            "SELECT id, decimals, chain_id FROM ledger_assets WHERE code = $1",
        )
        .bind(&command.asset_code)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| LedgerError::MissingAsset(command.asset_code.clone()))?;
        let expected_decimals = match command.asset_code.as_str() {
            "ETH" => 18,
            "USDC" => 6,
            _ => {
                return Err(LedgerError::InvalidWithdrawal(
                    "only ETH and USDC withdrawals are supported".to_owned(),
                ))
            }
        };
        if asset.1 != expected_decimals {
            return Err(LedgerError::InvalidWithdrawal(format!(
                "{} ledger decimals do not match the withdrawal policy",
                command.asset_code
            )));
        }
        let chain_id = asset.2.ok_or_else(|| {
            LedgerError::InvalidWithdrawal("withdrawal asset has no configured chain".to_owned())
        })?;
        let source_account_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id FROM ledger_accounts
            WHERE asset_id = $1
              AND owner_type = 'customer'
              AND lower(owner_reference) = lower($2)
              AND account_type = 'customer_available'
            "#,
        )
        .bind(asset.0)
        .bind(&command.customer_reference)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| LedgerError::MissingAccount {
            asset_code: command.asset_code.clone(),
            owner_reference: command.customer_reference.clone(),
            account_type: "customer_available".to_owned(),
        })?;
        let vault_addresses = sqlx::query_scalar::<_, String>(
            r#"
            SELECT owner_reference FROM ledger_accounts
            WHERE asset_id = $1 AND owner_type = 'custody' AND account_type = 'custody_vault'
            "#,
        )
        .bind(asset.0)
        .fetch_all(&mut *tx)
        .await?;
        let vault_address = match vault_addresses.as_slice() {
            [vault_address] => vault_address.clone(),
            [] => {
                return Err(LedgerError::MissingAccount {
                    asset_code: command.asset_code.clone(),
                    owner_reference: "configured custody vault".to_owned(),
                    account_type: "custody_vault".to_owned(),
                })
            }
            _ => {
                return Err(LedgerError::InvalidWithdrawal(
                    "withdrawal asset has multiple custody vault accounts".to_owned(),
                ))
            }
        };
        let reserved_account_id =
            ensure_reserved_account(&mut tx, asset.0, &command.customer_reference).await?;

        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawals (
                id, customer_reference, source_account_id, asset_id,
                destination_address, amount, state, chain_id, vault_address,
                nonce, expires_at
            )
            VALUES ($1, lower($2), $3, $4, lower($5), $6, 'requested', $7, lower($8), $9, $10)
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(&command.customer_reference)
        .bind(source_account_id)
        .bind(asset.0)
        .bind(&command.destination_address)
        .bind(&command.amount)
        .bind(chain_id)
        .bind(&vault_address)
        .bind(BigDecimal::from(command.nonce))
        .bind(command.expires_at)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            None,
            WithdrawalState::Requested,
            "authenticated customer withdrawal requested",
            json!({ "destination_address": command.destination_address }),
            Some(&command.customer_reference),
        )
        .await?;

        let request_hash = reservation_hash(command);
        let post = PostTransaction {
            id: command.reservation_transaction_id,
            operation_type: "withdrawal_reservation".to_owned(),
            idempotency_key: format!(
                "withdrawal-reservation:{}:{}",
                command.customer_reference.to_ascii_lowercase(),
                command.nonce
            ),
            request_hash,
            external_reference: Some(json!({ "withdrawal_id": command.withdrawal_id })),
            metadata: json!({
                "source": "customer_withdrawal_api",
                "destination_address": command.destination_address,
                "chain_id": chain_id,
                "vault_address": vault_address,
            }),
            legs: vec![
                JournalLegInput {
                    account_id: source_account_id,
                    amount: -command.amount.clone(),
                },
                JournalLegInput {
                    account_id: reserved_account_id,
                    amount: command.amount.clone(),
                },
            ],
        };
        validate_command(&post)?;
        let posted = post_command_in_transaction(&mut tx, &post)
            .await?
            .transaction;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(WithdrawalState::Requested),
            WithdrawalState::Reserved,
            "customer funds durably reserved before approval or signing",
            json!({ "reservation_transaction_id": posted.id }),
            Some("ledger"),
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE ledger_withdrawals
            SET state = 'reserved', reservation_transaction_id = $2,
                updated_at = transaction_timestamp()
            WHERE id = $1 AND state = 'requested'
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;

        let withdrawal = load_withdrawal(&mut tx, command.withdrawal_id).await?;
        tx.commit().await?;
        Ok(ReservedWithdrawal {
            withdrawal: withdrawal.try_into()?,
            transaction: posted,
        })
    }

    pub async fn cancel_reserved_withdrawal(
        &self,
        withdrawal_id: Uuid,
        customer_reference: &str,
        release_transaction_id: Uuid,
    ) -> Result<WithdrawalRecord, LedgerError> {
        if customer_reference.trim().is_empty() || release_transaction_id == Uuid::nil() {
            return Err(LedgerError::InvalidWithdrawal(
                "withdrawal cancellation identity is invalid".to_owned(),
            ));
        }
        let mut attempt = 0_u32;
        loop {
            match self
                .cancel_reserved_withdrawal_once(
                    withdrawal_id,
                    customer_reference,
                    release_transaction_id,
                )
                .await
            {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn cancel_reserved_withdrawal_once(
        &self,
        withdrawal_id: Uuid,
        customer_reference: &str,
        release_transaction_id: Uuid,
    ) -> Result<WithdrawalRecord, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let stored = load_withdrawal_for_update(&mut tx, withdrawal_id).await?;
        if !stored
            .customer_reference
            .eq_ignore_ascii_case(customer_reference)
        {
            return Err(LedgerError::WithdrawalNotFound(withdrawal_id));
        }
        let state = WithdrawalState::from_str(&stored.state)?;
        let request_hash = release_hash(&stored);
        if state == WithdrawalState::Cancelled {
            let transaction_id = stored.release_transaction_id.ok_or_else(|| {
                LedgerError::InvalidWithdrawal(
                    "cancelled withdrawal has no release transaction".to_owned(),
                )
            })?;
            let (_, stored_hash) = load_posted_transaction(&mut tx, transaction_id).await?;
            if stored_hash != request_hash {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return stored.try_into();
        }
        if state != WithdrawalState::Reserved {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Cancelled.as_str().to_owned(),
            });
        }

        let reserved_account_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id FROM ledger_accounts
            WHERE asset_id = $1 AND owner_type = 'customer'
              AND lower(owner_reference) = lower($2)
              AND account_type = 'customer_withdrawal_reserved'
            "#,
        )
        .bind(stored.asset_id)
        .bind(&stored.customer_reference)
        .fetch_one(&mut *tx)
        .await?;
        let post = PostTransaction {
            id: release_transaction_id,
            operation_type: "withdrawal_release".to_owned(),
            idempotency_key: format!("withdrawal-release:{}:cancelled", withdrawal_id),
            request_hash,
            external_reference: Some(json!({ "withdrawal_id": withdrawal_id })),
            metadata: json!({ "reason": "customer_cancelled_before_signing" }),
            legs: vec![
                JournalLegInput {
                    account_id: reserved_account_id,
                    amount: -stored.amount.clone(),
                },
                JournalLegInput {
                    account_id: stored.source_account_id,
                    amount: stored.amount.clone(),
                },
            ],
        };
        validate_command(&post)?;
        let posted = post_command_in_transaction(&mut tx, &post)
            .await?
            .transaction;
        insert_history(
            &mut tx,
            withdrawal_id,
            Some(state),
            WithdrawalState::Cancelled,
            "customer cancelled before signing; reservation released exactly once",
            json!({ "release_transaction_id": posted.id }),
            Some(customer_reference),
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE ledger_withdrawals
            SET state = 'cancelled', release_transaction_id = $2,
                updated_at = transaction_timestamp()
            WHERE id = $1 AND state = 'reserved'
            "#,
        )
        .bind(withdrawal_id)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;
        let withdrawal = load_withdrawal(&mut tx, withdrawal_id).await?;
        tx.commit().await?;
        withdrawal.try_into()
    }

    pub async fn activate_global_withdrawal_halt(
        &self,
        command: &ActivateGlobalWithdrawalHalt,
    ) -> Result<(), LedgerError> {
        validate_global_halt(command)?;
        sqlx::query(
            r#"
            INSERT INTO ledger_global_withdrawal_halts (id, reason, evidence, activated_by)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(command.halt_id)
        .bind(&command.reason)
        .bind(&command.evidence)
        .bind(&command.activated_by)
        .execute(&self.pool)
        .await?;

        let stored = sqlx::query_as::<_, (String, serde_json::Value, String)>(
            "SELECT reason, evidence, activated_by FROM ledger_global_withdrawal_halts WHERE id = $1",
        )
        .bind(command.halt_id)
        .fetch_one(&self.pool)
        .await?;
        if stored.0 != command.reason
            || stored.1 != command.evidence
            || stored.2 != command.activated_by
        {
            return Err(LedgerError::IdempotencyConflict);
        }
        Ok(())
    }

    pub async fn resolve_global_withdrawal_halt(
        &self,
        command: &ResolveGlobalWithdrawalHalt,
    ) -> Result<(), LedgerError> {
        validate_global_halt_resolution(command)?;
        let result = sqlx::query(
            r#"
            UPDATE ledger_global_withdrawal_halts
            SET resolution_evidence = $2,
                resolution_initiated_by = $3,
                resolved_by = $4,
                resolved_at = transaction_timestamp()
            WHERE id = $1 AND resolved_at IS NULL
            "#,
        )
        .bind(command.halt_id)
        .bind(&command.resolution_evidence)
        .bind(&command.resolution_initiated_by)
        .bind(&command.resolved_by)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(LedgerError::InvalidWithdrawal(
                "global withdrawal halt is missing or already resolved".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn evaluate_withdrawal_policy(
        &self,
        command: &EvaluateWithdrawalPolicy,
    ) -> Result<WithdrawalPolicyDecision, LedgerError> {
        validate_policy_evaluation(command)?;
        let mut attempt = 0_u32;
        loop {
            match self.evaluate_withdrawal_policy_once(command).await {
                Err(error)
                    if (is_retryable_transaction_error(&error)
                        || is_withdrawal_evidence_race(&error))
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn evaluate_withdrawal_policy_once(
        &self,
        command: &EvaluateWithdrawalPolicy,
    ) -> Result<WithdrawalPolicyDecision, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;

        if let Some(existing) = load_policy_decision(&mut tx, command.withdrawal_id).await? {
            let existing: WithdrawalPolicyDecision = existing.try_into()?;
            ensure_same_policy_evaluation(&existing, command)?;
            tx.commit().await?;
            return Ok(existing);
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if state != WithdrawalState::Reserved {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Approved.as_str().to_owned(),
            });
        }

        let limit = sqlx::query_as::<_, StoredWithdrawalPolicyLimit>(
            r#"
            SELECT policy_version, max_single_amount, max_customer_window_amount,
                   max_customer_window_count, customer_window_seconds,
                   max_platform_window_amount, platform_window_seconds
            FROM ledger_withdrawal_policy_limits
            WHERE asset_id = $1 AND effective_at <= transaction_timestamp()
            ORDER BY effective_at DESC
            LIMIT 1
            FOR SHARE
            "#,
        )
        .bind(withdrawal.asset_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            LedgerError::InvalidWithdrawal(
                "withdrawal has no active per-asset policy limit; approval fails closed".to_owned(),
            )
        })?;

        let (customer_count, customer_amount, platform_amount) =
            sqlx::query_as::<_, (i64, BigDecimal, BigDecimal)>(
                r#"
            SELECT
                count(*) FILTER (
                    WHERE lower(customer_reference) = lower($2)
                      AND created_at >= transaction_timestamp()
                          - make_interval(secs => $3::double precision)
                )::BIGINT,
                COALESCE(sum(amount) FILTER (
                    WHERE lower(customer_reference) = lower($2)
                      AND created_at >= transaction_timestamp()
                          - make_interval(secs => $3::double precision)
                ), 0),
                COALESCE(sum(amount) FILTER (
                    WHERE created_at >= transaction_timestamp()
                          - make_interval(secs => $4::double precision)
                ), 0)
            FROM ledger_withdrawals
            WHERE asset_id = $1
              AND state NOT IN ('rejected', 'cancelled', 'signing_failed', 'reversed')
            "#,
            )
            .bind(withdrawal.asset_id)
            .bind(&withdrawal.customer_reference)
            .bind(limit.customer_window_seconds as f64)
            .bind(limit.platform_window_seconds as f64)
            .fetch_one(&mut *tx)
            .await?;

        let global_halt_active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM ledger_global_withdrawal_halts WHERE resolved_at IS NULL)",
        )
        .fetch_one(&mut *tx)
        .await?;
        let asset_halt_active = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM ledger_asset_halts
                WHERE asset_id = $1 AND withdrawals_halted AND resolved_at IS NULL
            )
            "#,
        )
        .bind(withdrawal.asset_id)
        .fetch_one(&mut *tx)
        .await?;

        let mut reason_codes = Vec::new();
        if global_halt_active {
            reason_codes.push("global_withdrawal_halt".to_owned());
        }
        if asset_halt_active {
            reason_codes.push("asset_withdrawal_halt".to_owned());
        }
        if withdrawal.amount > limit.max_single_amount {
            reason_codes.push("asset_single_amount_limit".to_owned());
        }
        if customer_amount > limit.max_customer_window_amount {
            reason_codes.push("customer_velocity_amount_limit".to_owned());
        }
        if customer_count > limit.max_customer_window_count {
            reason_codes.push("customer_velocity_count_limit".to_owned());
        }
        if platform_amount > limit.max_platform_window_amount {
            reason_codes.push("platform_velocity_amount_limit".to_owned());
        }
        if command.compliance_disposition == ComplianceDisposition::Reject {
            reason_codes.push("compliance_rejected".to_owned());
        } else if command.compliance_disposition == ComplianceDisposition::Review {
            reason_codes.push("compliance_review_required".to_owned());
        } else if command.compliance_disposition == ComplianceDisposition::Unavailable {
            reason_codes.push("compliance_provider_unavailable".to_owned());
        }

        let hard_rejection = global_halt_active
            || asset_halt_active
            || withdrawal.amount > limit.max_single_amount
            || customer_amount > limit.max_customer_window_amount
            || customer_count > limit.max_customer_window_count
            || platform_amount > limit.max_platform_window_amount
            || command.compliance_disposition == ComplianceDisposition::Reject;
        let disposition = if hard_rejection {
            WithdrawalPolicyDisposition::Rejected
        } else if matches!(
            command.compliance_disposition,
            ComplianceDisposition::Review | ComplianceDisposition::Unavailable
        ) {
            WithdrawalPolicyDisposition::Review
        } else {
            WithdrawalPolicyDisposition::Approved
        };
        let evidence = json!({
            "withdrawal": {
                "id": withdrawal.id,
                "customer_reference": withdrawal.customer_reference,
                "asset_id": withdrawal.asset_id,
                "asset_code": withdrawal.asset_code,
                "amount_atomic": withdrawal.amount.normalized().to_plain_string(),
            },
            "limits": {
                "max_single_amount": limit.max_single_amount.normalized().to_plain_string(),
                "max_customer_window_amount": limit.max_customer_window_amount.normalized().to_plain_string(),
                "max_customer_window_count": limit.max_customer_window_count,
                "customer_window_seconds": limit.customer_window_seconds,
                "max_platform_window_amount": limit.max_platform_window_amount.normalized().to_plain_string(),
                "platform_window_seconds": limit.platform_window_seconds,
            },
            "observed": {
                "customer_window_amount": customer_amount.normalized().to_plain_string(),
                "customer_window_count": customer_count,
                "platform_window_amount": platform_amount.normalized().to_plain_string(),
                "global_halt_active": global_halt_active,
                "asset_halt_active": asset_halt_active,
            },
            "compliance": {
                "provider": command.compliance_provider,
                "reference": command.compliance_reference,
                "disposition": command.compliance_disposition.as_str(),
            },
            "reason_codes": reason_codes,
        });
        let canonical_evidence = serde_json::to_vec(&evidence).map_err(|error| {
            LedgerError::InvalidWithdrawal(format!("policy evidence serialization failed: {error}"))
        })?;
        let evidence_hash: [u8; 32] = Sha256::digest(canonical_evidence).into();

        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_policy_decisions (
                id, withdrawal_id, policy_version, disposition,
                compliance_provider, compliance_reference, compliance_disposition,
                reason_codes, evidence, evidence_hash, evaluated_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(command.decision_id)
        .bind(command.withdrawal_id)
        .bind(&limit.policy_version)
        .bind(disposition.as_str())
        .bind(&command.compliance_provider)
        .bind(&command.compliance_reference)
        .bind(command.compliance_disposition.as_str())
        .bind(json!(&reason_codes))
        .bind(&evidence)
        .bind(evidence_hash.as_slice())
        .bind(&command.evaluated_by)
        .execute(&mut *tx)
        .await?;

        let stored = load_policy_decision(&mut tx, command.withdrawal_id)
            .await?
            .ok_or_else(|| {
                LedgerError::InvalidWithdrawal(
                    "withdrawal policy decision was not stored".to_owned(),
                )
            })?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn authorize_withdrawal(
        &self,
        command: &ApproveWithdrawal,
    ) -> Result<WithdrawalAuthorization, LedgerError> {
        let mut attempt = 0_u32;
        loop {
            match self.authorize_withdrawal_once(command).await {
                Err(error)
                    if (is_retryable_transaction_error(&error)
                        || is_withdrawal_evidence_race(&error))
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn authorize_withdrawal_once(
        &self,
        command: &ApproveWithdrawal,
    ) -> Result<WithdrawalAuthorization, LedgerError> {
        validate_approval(command)?;
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;

        if let Some(existing) =
            load_authorization_by_withdrawal(&mut tx, command.withdrawal_id).await?
        {
            let existing: WithdrawalAuthorization = existing.try_into()?;
            ensure_same_authorization(&existing, &withdrawal, command)?;
            tx.commit().await?;
            return Ok(existing);
        }

        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if state != WithdrawalState::Reserved {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Approved.as_str().to_owned(),
            });
        }
        let nonce = withdrawal.nonce.clone().ok_or_else(|| {
            LedgerError::InvalidWithdrawal("withdrawal authorization requires a nonce".to_owned())
        })?;
        let canonical_payload = authorization_payload(&withdrawal, &nonce, command);
        let hashes = contract_authorization_hashes(&withdrawal, &nonce, command)?;
        let digest = hashes.digest;

        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_authorizations (
                id, withdrawal_id, envelope_version, chain_id, vault_address,
                customer_reference, destination_address, asset_id, asset_address, amount, nonce,
                expires_at, canonical_payload, digest,
                domain_separator, struct_hash, policy_hash, approver_hash,
                policy_version, approved_by, approved_at
            )
            VALUES ($1, $2, 'vela.withdrawal.authorization.eip712.v2', $3, lower($4),
                    lower($5), lower($6), $7, lower($8), $9, $10, $11, $12, $13,
                    $14, $15, $16, $17, $18, $19, $20)
            "#,
        )
        .bind(command.authorization_id)
        .bind(command.withdrawal_id)
        .bind(withdrawal.chain_id)
        .bind(&withdrawal.vault_address)
        .bind(&withdrawal.customer_reference)
        .bind(&withdrawal.destination_address)
        .bind(withdrawal.asset_id)
        .bind(asset_address(&withdrawal).to_ascii_lowercase())
        .bind(&withdrawal.amount)
        .bind(&nonce)
        .bind(command.expires_at)
        .bind(&canonical_payload)
        .bind(digest.as_slice())
        .bind(hashes.domain_separator.as_slice())
        .bind(hashes.struct_hash.as_slice())
        .bind(hashes.policy_hash.as_slice())
        .bind(hashes.approver_hash.as_slice())
        .bind(&command.policy_version)
        .bind(&command.approved_by)
        .bind(command.approved_at)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(WithdrawalState::Reserved),
            WithdrawalState::Approved,
            "withdrawal policy produced a durable contract-bound authorization",
            json!({
                "authorization_id": command.authorization_id,
                "authorization_digest": hex_digest(&digest),
                "policy_version": command.policy_version,
                "expires_at": command.expires_at,
            }),
            Some(&command.approved_by),
        )
        .await?;
        sqlx::query(
            "UPDATE ledger_withdrawals SET state = 'approved', updated_at = transaction_timestamp() WHERE id = $1 AND state = 'reserved'",
        )
        .bind(command.withdrawal_id)
        .execute(&mut *tx)
        .await?;

        let authorization = load_authorization_by_withdrawal(&mut tx, command.withdrawal_id)
            .await?
            .ok_or(LedgerError::WithdrawalAuthorizationNotFound(
                command.withdrawal_id,
            ))?;
        tx.commit().await?;
        authorization.try_into()
    }

    pub async fn begin_withdrawal_signing(
        &self,
        withdrawal_id: Uuid,
        authorization_id: Uuid,
    ) -> Result<WithdrawalAuthorization, LedgerError> {
        let mut attempt = 0_u32;
        loop {
            match self
                .begin_withdrawal_signing_once(withdrawal_id, authorization_id)
                .await
            {
                Err(error)
                    if is_retryable_transaction_error(&error)
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn begin_withdrawal_signing_once(
        &self,
        withdrawal_id: Uuid,
        authorization_id: Uuid,
    ) -> Result<WithdrawalAuthorization, LedgerError> {
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, withdrawal_id).await?;
        let authorization = load_authorization_by_withdrawal(&mut tx, withdrawal_id)
            .await?
            .ok_or(LedgerError::WithdrawalAuthorizationNotFound(withdrawal_id))?;
        if authorization.id != authorization_id {
            return Err(LedgerError::IdempotencyConflict);
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if state == WithdrawalState::Approved {
            insert_history(
                &mut tx,
                withdrawal_id,
                Some(state),
                WithdrawalState::Signing,
                "custody signer accepted the durable authorization",
                json!({ "authorization_id": authorization_id }),
                Some("custody-signer"),
            )
            .await?;
            sqlx::query("UPDATE ledger_withdrawals SET state = 'signing', updated_at = transaction_timestamp() WHERE id = $1 AND state = 'approved'")
                .bind(withdrawal_id)
                .execute(&mut *tx)
                .await?;
        } else if !matches!(state, WithdrawalState::Signing | WithdrawalState::Signed) {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Signing.as_str().to_owned(),
            });
        }
        tx.commit().await?;
        authorization.try_into()
    }

    pub async fn record_withdrawal_signature(
        &self,
        command: &RecordWithdrawalSignature,
    ) -> Result<WithdrawalSignatureRecord, LedgerError> {
        let mut attempt = 0_u32;
        loop {
            match self.record_withdrawal_signature_once(command).await {
                Err(error)
                    if (is_retryable_transaction_error(&error)
                        || is_withdrawal_evidence_race(&error))
                        && attempt < self.max_serializable_retries =>
                {
                    let backoff_ms = 5_u64.saturating_mul(1_u64 << attempt.min(6));
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn record_withdrawal_signature_once(
        &self,
        command: &RecordWithdrawalSignature,
    ) -> Result<WithdrawalSignatureRecord, LedgerError> {
        validate_signature_record(command)?;
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;
        if let Some(existing) = load_signature(&mut tx, command.withdrawal_id).await? {
            let existing: WithdrawalSignatureRecord = existing.try_into()?;
            ensure_same_signature(&existing, command)?;
            tx.commit().await?;
            return Ok(existing);
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if state != WithdrawalState::Signing {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Signed.as_str().to_owned(),
            });
        }
        let authorization = load_authorization_by_withdrawal(&mut tx, command.withdrawal_id)
            .await?
            .ok_or(LedgerError::WithdrawalAuthorizationNotFound(
                command.withdrawal_id,
            ))?;
        if authorization.id != command.authorization_id
            || authorization.digest.as_slice() != command.authorization_digest
        {
            return Err(LedgerError::IdempotencyConflict);
        }

        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_signatures (
                id, withdrawal_id, authorization_id, authorization_digest,
                signer_key_id, signer_public_key, algorithm, signature, signed_at
            ) VALUES ($1, $2, $3, $4, $5, $6, 'secp256k1_recoverable', $7, $8)
            "#,
        )
        .bind(command.signature_id)
        .bind(command.withdrawal_id)
        .bind(command.authorization_id)
        .bind(command.authorization_digest.as_slice())
        .bind(&command.signer_key_id)
        .bind(&command.signer_public_key)
        .bind(command.signature.as_slice())
        .bind(command.signed_at)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(state),
            WithdrawalState::Signed,
            "custody signer returned immutable recoverable signature evidence",
            json!({
                "authorization_id": command.authorization_id,
                "signature_id": command.signature_id,
                "signer_key_id": command.signer_key_id,
            }),
            Some("custody-signer"),
        )
        .await?;
        sqlx::query("UPDATE ledger_withdrawals SET state = 'signed', updated_at = transaction_timestamp() WHERE id = $1 AND state = 'signing'")
            .bind(command.withdrawal_id)
            .execute(&mut *tx)
            .await?;
        let signature = load_signature(&mut tx, command.withdrawal_id)
            .await?
            .ok_or_else(|| {
                LedgerError::InvalidWithdrawal("withdrawal signature was not stored".to_owned())
            })?;
        tx.commit().await?;
        signature.try_into()
    }

    pub async fn record_withdrawal_broadcast_attempt(
        &self,
        command: &RecordWithdrawalBroadcastAttempt,
    ) -> Result<WithdrawalBroadcastAttempt, LedgerError> {
        validate_broadcast_attempt(command)?;
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;

        if let Some(existing) = load_broadcast_attempt_by_identity(
            &mut tx,
            command.attempt_id,
            &command.transaction_hash,
        )
        .await?
        {
            let existing: WithdrawalBroadcastAttempt = existing.try_into()?;
            ensure_same_broadcast_attempt(&existing, command)?;
            tx.commit().await?;
            return Ok(existing);
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if !matches!(
            state,
            WithdrawalState::Signed
                | WithdrawalState::Broadcast
                | WithdrawalState::Replaced
                | WithdrawalState::Dropped
        ) {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Broadcast.as_str().to_owned(),
            });
        }
        let authorization = load_authorization_by_withdrawal(&mut tx, command.withdrawal_id)
            .await?
            .ok_or(LedgerError::WithdrawalAuthorizationNotFound(
                command.withdrawal_id,
            ))?;
        if command.submitted_at > authorization.expires_at {
            return Err(LedgerError::InvalidWithdrawal(
                "withdrawal was submitted after its on-chain deadline".to_owned(),
            ));
        }
        let attempt_number = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)::BIGINT + 1 FROM ledger_withdrawal_broadcast_attempts WHERE withdrawal_id = $1",
        )
        .bind(command.withdrawal_id)
        .fetch_one(&mut *tx)
        .await?;
        if let Some(predecessor_id) = command.replacement_of {
            let predecessor = sqlx::query_as::<_, (Uuid, i64, String, BigDecimal)>(
                "SELECT withdrawal_id, attempt_number, sender_address, sender_nonce FROM ledger_withdrawal_broadcast_attempts WHERE id = $1",
            )
            .bind(predecessor_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                LedgerError::InvalidWithdrawal(
                    "replacement references an unknown broadcast attempt".to_owned(),
                )
            })?;
            if predecessor.0 != command.withdrawal_id
                || predecessor.1 + 1 != attempt_number
                || !predecessor.2.eq_ignore_ascii_case(&command.sender_address)
                || predecessor.3 != command.sender_nonce
            {
                return Err(LedgerError::InvalidWithdrawal(
                    "replacement must reference the immediately preceding attempt with the same sender nonce"
                        .to_owned(),
                ));
            }
        } else if attempt_number > 1 && state != WithdrawalState::Dropped {
            return Err(LedgerError::InvalidWithdrawal(
                "a new live broadcast attempt must identify the attempt it replaces".to_owned(),
            ));
        }

        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_broadcast_attempts (
                id, withdrawal_id, attempt_number, transaction_hash, sender_address,
                sender_nonce, vault_address, calldata_hash, replacement_of,
                provider_name, provider_reference, submitted_by, submitted_at
            ) VALUES ($1, $2, $3, lower($4), lower($5), $6, lower($7), $8,
                      $9, $10, $11, $12, $13)
            "#,
        )
        .bind(command.attempt_id)
        .bind(command.withdrawal_id)
        .bind(attempt_number)
        .bind(&command.transaction_hash)
        .bind(&command.sender_address)
        .bind(BigDecimal::from(command.sender_nonce))
        .bind(&withdrawal.vault_address)
        .bind(command.calldata_hash.as_slice())
        .bind(command.replacement_of)
        .bind(&command.provider_name)
        .bind(&command.provider_reference)
        .bind(&command.submitted_by)
        .bind(command.submitted_at)
        .execute(&mut *tx)
        .await?;

        let next_state = if command.replacement_of.is_some() {
            WithdrawalState::Replaced
        } else {
            WithdrawalState::Broadcast
        };
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(state),
            next_state,
            "provider accepted an exact executable withdrawal transaction",
            json!({
                "attempt_id": command.attempt_id,
                "attempt_number": attempt_number,
                "transaction_hash": command.transaction_hash,
                "replacement_of": command.replacement_of,
            }),
            Some(&command.submitted_by),
        )
        .await?;
        sqlx::query(
            "UPDATE ledger_withdrawals SET state = $2, transaction_hash = lower($3), updated_at = transaction_timestamp() WHERE id = $1",
        )
        .bind(command.withdrawal_id)
        .bind(next_state.as_str())
        .bind(&command.transaction_hash)
        .execute(&mut *tx)
        .await?;

        let stored = load_broadcast_attempt_by_identity(
            &mut tx,
            command.attempt_id,
            &command.transaction_hash,
        )
        .await?
        .ok_or_else(|| {
            LedgerError::InvalidWithdrawal("broadcast attempt was not stored".to_owned())
        })?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn confirm_withdrawal_execution(
        &self,
        command: &FinalizedWithdrawalExecution,
    ) -> Result<WithdrawalRecord, LedgerError> {
        validate_finalized_execution(command)?;
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;
        let evidence = finalized_execution_evidence(&withdrawal, command);
        let evidence_hash = json_hash(&evidence)?;

        if let Some((observation_id, transaction_id, stored_hash)) =
            load_existing_execution(&mut tx, command.withdrawal_id).await?
        {
            if observation_id != command.observation_id
                || transaction_id != command.completion_transaction_id
                || stored_hash != evidence_hash
            {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return withdrawal.try_into();
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if !matches!(
            state,
            WithdrawalState::Signed
                | WithdrawalState::Broadcast
                | WithdrawalState::Replaced
                | WithdrawalState::Dropped
        ) {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Confirmed.as_str().to_owned(),
            });
        }
        require_broadcast_transaction(&mut tx, command.withdrawal_id, &command.transaction_hash)
            .await?;
        insert_chain_observation(
            &mut tx,
            command.observation_id,
            command.withdrawal_id,
            command.deployment_id,
            "finalized_execution",
            Some(&command.transaction_hash),
            command.block_number,
            &command.block_hash,
            command.block_timestamp,
            Some(command.transaction_index),
            Some(command.log_index),
            command.confirmations,
            &command.primary_provider,
            &command.secondary_provider,
            &command.confirmation_policy_version,
            &withdrawal,
            &evidence,
            &evidence_hash,
            None,
        )
        .await?;

        let reserved_account_id = reserved_account_id(&mut tx, &withdrawal).await?;
        let custody_withdrawn_id = custody_withdrawn_account_id(&mut tx, &withdrawal).await?;
        let posted = post_command_in_transaction(
            &mut tx,
            &PostTransaction {
                id: command.completion_transaction_id,
                operation_type: "withdrawal_completion".to_owned(),
                idempotency_key: format!("withdrawal-completion:{}", command.withdrawal_id),
                request_hash: evidence_hash,
                external_reference: Some(json!({
                    "withdrawal_id": command.withdrawal_id,
                    "transaction_hash": command.transaction_hash,
                })),
                metadata: json!({
                    "observation_id": command.observation_id,
                    "block_number": command.block_number,
                    "block_hash": command.block_hash,
                }),
                legs: vec![
                    JournalLegInput {
                        account_id: reserved_account_id,
                        amount: -withdrawal.amount.clone(),
                    },
                    JournalLegInput {
                        account_id: custody_withdrawn_id,
                        amount: withdrawal.amount.clone(),
                    },
                ],
            },
        )
        .await?
        .transaction;
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_executions (
                withdrawal_id, observation_id, completion_transaction_id, transaction_hash
            ) VALUES ($1, $2, $3, lower($4))
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(command.observation_id)
        .bind(posted.id)
        .bind(&command.transaction_hash)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(state),
            WithdrawalState::Confirmed,
            "two providers confirmed the exact withdrawal event at finalized depth",
            json!({
                "observation_id": command.observation_id,
                "completion_transaction_id": posted.id,
                "transaction_hash": command.transaction_hash,
            }),
            Some("withdrawal-chain-observer"),
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE ledger_withdrawals
            SET state = 'confirmed', completion_transaction_id = $2,
                transaction_hash = lower($3), updated_at = transaction_timestamp()
            WHERE id = $1
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(posted.id)
        .bind(&command.transaction_hash)
        .execute(&mut *tx)
        .await?;
        let stored = load_withdrawal(&mut tx, command.withdrawal_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn record_dropped_withdrawal_attempt(
        &self,
        command: &RecordDroppedWithdrawalAttempt,
    ) -> Result<WithdrawalRecord, LedgerError> {
        if command.observation_id == Uuid::nil()
            || command.withdrawal_id == Uuid::nil()
            || command.deployment_id == Uuid::nil()
            || !is_evm_hash(&command.transaction_hash)
            || !valid_two_provider_evidence(&command.primary_provider, &command.secondary_provider)
            || !command.evidence.is_object()
        {
            return Err(LedgerError::InvalidWithdrawal(
                "dropped attempt requires exact transaction evidence from two independent providers"
                    .to_owned(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;
        let evidence = json!({
            "kind": "vela.withdrawal.dropped.v1",
            "withdrawal_id": withdrawal.id,
            "deployment_id": command.deployment_id,
            "transaction_hash": command.transaction_hash.to_ascii_lowercase(),
            "primary_provider": command.primary_provider,
            "secondary_provider": command.secondary_provider,
            "provider_evidence": command.evidence,
        });
        let evidence_hash = json_hash(&evidence)?;
        if let Some((withdrawal_id, observation_type, transaction_hash, stored_hash)) =
            load_chain_observation(&mut tx, command.observation_id).await?
        {
            if withdrawal_id != command.withdrawal_id
                || observation_type != "dropped"
                || transaction_hash.as_deref() != Some(command.transaction_hash.as_str())
                || stored_hash != evidence_hash
            {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return withdrawal.try_into();
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if !matches!(
            state,
            WithdrawalState::Broadcast | WithdrawalState::Replaced
        ) {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::Dropped.as_str().to_owned(),
            });
        }
        if withdrawal.transaction_hash.as_deref() != Some(command.transaction_hash.as_str()) {
            return Err(LedgerError::InvalidWithdrawal(
                "only the currently tracked broadcast attempt can be marked dropped".to_owned(),
            ));
        }
        require_broadcast_transaction(&mut tx, command.withdrawal_id, &command.transaction_hash)
            .await?;
        let nonce = withdrawal.nonce.clone().ok_or_else(|| {
            LedgerError::InvalidWithdrawal("dropped attempt requires a withdrawal nonce".to_owned())
        })?;
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_chain_observations (
                id, withdrawal_id, deployment_id, observation_type, transaction_hash,
                chain_id, vault_address, request_id, customer_reference,
                destination_address, asset_address, amount, withdrawal_nonce,
                finalized, primary_provider, secondary_provider, evidence, evidence_hash
            ) VALUES ($1, $2, $3, 'dropped', lower($4), $5, lower($6), $7,
                      lower($8), lower($9), lower($10), $11, $12, FALSE, $13, $14, $15, $16)
            "#,
        )
        .bind(command.observation_id)
        .bind(command.withdrawal_id)
        .bind(command.deployment_id)
        .bind(&command.transaction_hash)
        .bind(withdrawal.chain_id)
        .bind(&withdrawal.vault_address)
        .bind(withdrawal.id)
        .bind(&withdrawal.customer_reference)
        .bind(&withdrawal.destination_address)
        .bind(asset_address(&withdrawal))
        .bind(&withdrawal.amount)
        .bind(nonce)
        .bind(&command.primary_provider)
        .bind(&command.secondary_provider)
        .bind(&evidence)
        .bind(evidence_hash.as_slice())
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(state),
            WithdrawalState::Dropped,
            "two providers no longer observe the previously submitted transaction",
            json!({
                "observation_id": command.observation_id,
                "transaction_hash": command.transaction_hash,
            }),
            Some("withdrawal-chain-observer"),
        )
        .await?;
        sqlx::query(
            "UPDATE ledger_withdrawals SET state = 'dropped', updated_at = transaction_timestamp() WHERE id = $1",
        )
        .bind(command.withdrawal_id)
        .execute(&mut *tx)
        .await?;
        let stored = load_withdrawal(&mut tx, command.withdrawal_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn release_expired_withdrawal(
        &self,
        command: &FinalizedWithdrawalNonExecution,
    ) -> Result<WithdrawalRecord, LedgerError> {
        validate_finalized_non_execution(command)?;
        let mut tx = self.pool.begin().await?;
        set_serializable(&mut tx).await?;
        let withdrawal = load_withdrawal_for_update(&mut tx, command.withdrawal_id).await?;
        let evidence = finalized_non_execution_evidence(&withdrawal, command);
        let evidence_hash = json_hash(&evidence)?;
        if let Some((observation_id, transaction_id, stored_hash)) =
            load_existing_failure_release(&mut tx, command.withdrawal_id).await?
        {
            if observation_id != command.observation_id
                || transaction_id != command.release_transaction_id
                || stored_hash != evidence_hash
            {
                return Err(LedgerError::IdempotencyConflict);
            }
            tx.commit().await?;
            return withdrawal.try_into();
        }
        let state = WithdrawalState::from_str(&withdrawal.state)?;
        if !matches!(
            state,
            WithdrawalState::Signed
                | WithdrawalState::Broadcast
                | WithdrawalState::Replaced
                | WithdrawalState::Dropped
        ) {
            return Err(LedgerError::InvalidWithdrawalTransition {
                from: state.as_str().to_owned(),
                to: WithdrawalState::BroadcastFailed.as_str().to_owned(),
            });
        }
        let authorization = load_authorization_by_withdrawal(&mut tx, command.withdrawal_id)
            .await?
            .ok_or(LedgerError::WithdrawalAuthorizationNotFound(
                command.withdrawal_id,
            ))?;
        if command.block_timestamp < authorization.expires_at {
            return Err(LedgerError::InvalidWithdrawal(
                "finalized non-execution proof predates the on-chain deadline".to_owned(),
            ));
        }
        if let Some(transaction_hash) = command.last_transaction_hash.as_deref() {
            require_broadcast_transaction(&mut tx, command.withdrawal_id, transaction_hash).await?;
        }
        insert_chain_observation(
            &mut tx,
            command.observation_id,
            command.withdrawal_id,
            command.deployment_id,
            "finalized_non_execution",
            command.last_transaction_hash.as_deref(),
            command.block_number,
            &command.block_hash,
            command.block_timestamp,
            None,
            None,
            command.confirmations,
            &command.primary_provider,
            &command.secondary_provider,
            &command.confirmation_policy_version,
            &withdrawal,
            &evidence,
            &evidence_hash,
            Some((
                command.request_id_unused,
                command.withdrawal_nonce_unused,
                command.no_execution_event,
                command.all_attempts_terminal,
            )),
        )
        .await?;
        let reserved_account_id = reserved_account_id(&mut tx, &withdrawal).await?;
        let posted = post_command_in_transaction(
            &mut tx,
            &PostTransaction {
                id: command.release_transaction_id,
                operation_type: "withdrawal_release".to_owned(),
                idempotency_key: format!(
                    "withdrawal-release:{}:permanent-failure",
                    command.withdrawal_id
                ),
                request_hash: evidence_hash,
                external_reference: Some(json!({ "withdrawal_id": command.withdrawal_id })),
                metadata: json!({
                    "reason": "eip712_authorization_expired_unused_at_finalized_block",
                    "observation_id": command.observation_id,
                }),
                legs: vec![
                    JournalLegInput {
                        account_id: reserved_account_id,
                        amount: -withdrawal.amount.clone(),
                    },
                    JournalLegInput {
                        account_id: withdrawal.source_account_id,
                        amount: withdrawal.amount.clone(),
                    },
                ],
            },
        )
        .await?
        .transaction;
        sqlx::query(
            r#"
            INSERT INTO ledger_withdrawal_failure_releases (
                withdrawal_id, observation_id, release_transaction_id
            ) VALUES ($1, $2, $3)
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(command.observation_id)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;
        insert_history(
            &mut tx,
            command.withdrawal_id,
            Some(state),
            WithdrawalState::BroadcastFailed,
            "finalized V2 authorization expiry proved non-execution; reservation released",
            json!({
                "observation_id": command.observation_id,
                "release_transaction_id": posted.id,
            }),
            Some("withdrawal-chain-observer"),
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE ledger_withdrawals
            SET state = 'broadcast_failed', release_transaction_id = $2,
                updated_at = transaction_timestamp()
            WHERE id = $1
            "#,
        )
        .bind(command.withdrawal_id)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;
        let stored = load_withdrawal(&mut tx, command.withdrawal_id).await?;
        tx.commit().await?;
        stored.try_into()
    }

    pub async fn customer_withdrawals(
        &self,
        customer_reference: &str,
        limit: u32,
    ) -> Result<Vec<WithdrawalRecord>, LedgerError> {
        if customer_reference.trim().is_empty()
            || customer_reference.len() > 255
            || !(1..=100).contains(&limit)
        {
            return Err(LedgerError::InvalidWithdrawal(
                "customer withdrawal query is invalid".to_owned(),
            ));
        }
        let rows = sqlx::query_as::<_, StoredWithdrawal>(&format!(
            "{} WHERE lower(withdrawal.customer_reference) = lower($1) ORDER BY withdrawal.created_at DESC, withdrawal.id LIMIT $2",
            withdrawal_select()
        ))
        .bind(customer_reference)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn withdrawal_signature(
        &self,
        withdrawal_id: Uuid,
    ) -> Result<Option<WithdrawalSignatureRecord>, LedgerError> {
        let stored = sqlx::query_as::<_, StoredWithdrawalSignature>(
            r#"
            SELECT id, withdrawal_id, authorization_id, authorization_digest,
                   signer_key_id, signer_public_key, algorithm, signature, signed_at
            FROM ledger_withdrawal_signatures
            WHERE withdrawal_id = $1
            "#,
        )
        .bind(withdrawal_id)
        .fetch_optional(&self.pool)
        .await?;
        stored.map(TryInto::try_into).transpose()
    }
}

fn withdrawal_select() -> &'static str {
    r#"
    SELECT withdrawal.id, withdrawal.customer_reference, withdrawal.source_account_id,
           withdrawal.asset_id, asset.code AS asset_code, asset.decimals AS asset_decimals,
           asset.contract_address AS asset_contract_address,
           withdrawal.destination_address, withdrawal.amount, withdrawal.state,
           withdrawal.reservation_transaction_id, withdrawal.completion_transaction_id,
           withdrawal.release_transaction_id, withdrawal.chain_id, withdrawal.vault_address,
           withdrawal.nonce, withdrawal.expires_at, withdrawal.transaction_hash,
           withdrawal.created_at, withdrawal.updated_at
    FROM ledger_withdrawals withdrawal
    JOIN ledger_assets asset ON asset.id = withdrawal.asset_id
    "#
}

fn authorization_select() -> &'static str {
    r#"
    SELECT authz.id, authz.withdrawal_id,
           authz.envelope_version, authz.chain_id,
           authz.vault_address, authz.customer_reference,
           authz.destination_address, authz.asset_id,
           asset.code AS asset_code, authz.asset_address, authz.amount, authz.nonce,
           authz.expires_at, authz.canonical_payload,
           authz.digest, authz.domain_separator, authz.struct_hash,
           authz.policy_hash, authz.approver_hash, authz.policy_version,
           authz.approved_by, authz.approved_at
    FROM ledger_withdrawal_authorizations authz
    JOIN ledger_assets asset ON asset.id = authz.asset_id
    "#
}

async fn load_authorization_by_withdrawal(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<Option<StoredWithdrawalAuthorization>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredWithdrawalAuthorization>(&format!(
        "{} WHERE authz.withdrawal_id = $1",
        authorization_select()
    ))
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn load_signature(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<Option<StoredWithdrawalSignature>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredWithdrawalSignature>(
        r#"
        SELECT id, withdrawal_id, authorization_id, authorization_digest,
               signer_key_id, signer_public_key, algorithm, signature, signed_at
        FROM ledger_withdrawal_signatures
        WHERE withdrawal_id = $1
        "#,
    )
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn load_broadcast_attempt_by_identity(
    tx: &mut Transaction<'_, Postgres>,
    attempt_id: Uuid,
    transaction_hash: &str,
) -> Result<Option<StoredWithdrawalBroadcastAttempt>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredWithdrawalBroadcastAttempt>(
        r#"
        SELECT id, withdrawal_id, attempt_number, transaction_hash, sender_address,
               sender_nonce, vault_address, calldata_hash, replacement_of,
               provider_name, provider_reference, submitted_by, submitted_at
        FROM ledger_withdrawal_broadcast_attempts
        WHERE id = $1 OR transaction_hash = lower($2)
        "#,
    )
    .bind(attempt_id)
    .bind(transaction_hash)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn require_broadcast_transaction(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
    transaction_hash: &str,
) -> Result<(), LedgerError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM ledger_withdrawal_broadcast_attempts
            WHERE withdrawal_id = $1 AND transaction_hash = lower($2)
        )
        "#,
    )
    .bind(withdrawal_id)
    .bind(transaction_hash)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        return Err(LedgerError::InvalidWithdrawal(
            "chain evidence does not identify a recorded broadcast attempt".to_owned(),
        ));
    }
    Ok(())
}

async fn load_existing_execution(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<Option<(Uuid, Uuid, [u8; 32])>, LedgerError> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, Vec<u8>)>(
        r#"
        SELECT execution.observation_id, execution.completion_transaction_id,
               observation.evidence_hash
        FROM ledger_withdrawal_executions execution
        JOIN ledger_withdrawal_chain_observations observation
          ON observation.id = execution.observation_id
        WHERE execution.withdrawal_id = $1
        "#,
    )
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|(observation_id, transaction_id, hash)| {
        Ok((
            observation_id,
            transaction_id,
            fixed_bytes(hash, "withdrawal execution evidence hash")?,
        ))
    })
    .transpose()
}

async fn load_chain_observation(
    tx: &mut Transaction<'_, Postgres>,
    observation_id: Uuid,
) -> Result<Option<(Uuid, String, Option<String>, [u8; 32])>, LedgerError> {
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>, Vec<u8>)>(
        r#"
        SELECT withdrawal_id, observation_type, transaction_hash, evidence_hash
        FROM ledger_withdrawal_chain_observations WHERE id = $1
        "#,
    )
    .bind(observation_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(
        |(withdrawal_id, observation_type, transaction_hash, hash)| {
            Ok((
                withdrawal_id,
                observation_type,
                transaction_hash,
                fixed_bytes(hash, "withdrawal chain observation evidence hash")?,
            ))
        },
    )
    .transpose()
}

async fn load_existing_failure_release(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<Option<(Uuid, Uuid, [u8; 32])>, LedgerError> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, Vec<u8>)>(
        r#"
        SELECT failure.observation_id, failure.release_transaction_id,
               observation.evidence_hash
        FROM ledger_withdrawal_failure_releases failure
        JOIN ledger_withdrawal_chain_observations observation
          ON observation.id = failure.observation_id
        WHERE failure.withdrawal_id = $1
        "#,
    )
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|(observation_id, transaction_id, hash)| {
        Ok((
            observation_id,
            transaction_id,
            fixed_bytes(hash, "withdrawal release evidence hash")?,
        ))
    })
    .transpose()
}

async fn reserved_account_id(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal: &StoredWithdrawal,
) -> Result<Uuid, LedgerError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id FROM ledger_accounts
        WHERE asset_id = $1 AND owner_type = 'customer'
          AND lower(owner_reference) = lower($2)
          AND account_type = 'customer_withdrawal_reserved'
        "#,
    )
    .bind(withdrawal.asset_id)
    .bind(&withdrawal.customer_reference)
    .fetch_one(&mut **tx)
    .await?)
}

async fn custody_withdrawn_account_id(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal: &StoredWithdrawal,
) -> Result<Uuid, LedgerError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id FROM ledger_accounts
        WHERE asset_id = $1 AND owner_type = 'custody'
          AND lower(owner_reference) = lower($2)
          AND account_type = 'custody_withdrawn'
        "#,
    )
    .bind(withdrawal.asset_id)
    .bind(&withdrawal.vault_address)
    .fetch_one(&mut **tx)
    .await?)
}

#[allow(clippy::too_many_arguments)]
async fn insert_chain_observation(
    tx: &mut Transaction<'_, Postgres>,
    observation_id: Uuid,
    withdrawal_id: Uuid,
    deployment_id: Uuid,
    observation_type: &str,
    transaction_hash: Option<&str>,
    block_number: i64,
    block_hash: &str,
    block_timestamp: DateTime<Utc>,
    transaction_index: Option<i32>,
    log_index: Option<i32>,
    confirmations: i64,
    primary_provider: &str,
    secondary_provider: &str,
    confirmation_policy_version: &str,
    withdrawal: &StoredWithdrawal,
    evidence: &serde_json::Value,
    evidence_hash: &[u8; 32],
    unused_proof: Option<(bool, bool, bool, bool)>,
) -> Result<(), LedgerError> {
    let nonce = withdrawal.nonce.clone().ok_or_else(|| {
        LedgerError::InvalidWithdrawal("withdrawal chain evidence requires a nonce".to_owned())
    })?;
    let (request_unused, nonce_unused, no_event, attempts_terminal) = unused_proof
        .map(|proof| (Some(proof.0), Some(proof.1), Some(proof.2), Some(proof.3)))
        .unwrap_or((None, None, None, None));
    sqlx::query(
        r#"
        INSERT INTO ledger_withdrawal_chain_observations (
            id, withdrawal_id, deployment_id, observation_type, transaction_hash,
            chain_id, vault_address, request_id, customer_reference,
            destination_address, asset_address, amount, withdrawal_nonce,
            block_number, block_hash, block_timestamp, transaction_index, log_index,
            confirmations, finalized, request_id_unused, withdrawal_nonce_unused,
            no_execution_event, all_attempts_terminal, primary_provider,
            secondary_provider, confirmation_policy_version, evidence, evidence_hash
        ) VALUES (
            $1, $2, $3, $4, lower($5), $6, lower($7), $8, lower($9),
            lower($10), lower($11), $12, $13, $14, lower($15), $16, $17, $18,
            $19, TRUE, $20, $21, $22, $23, $24, $25, $26, $27, $28
        )
        "#,
    )
    .bind(observation_id)
    .bind(withdrawal_id)
    .bind(deployment_id)
    .bind(observation_type)
    .bind(transaction_hash)
    .bind(withdrawal.chain_id)
    .bind(&withdrawal.vault_address)
    .bind(withdrawal.id)
    .bind(&withdrawal.customer_reference)
    .bind(&withdrawal.destination_address)
    .bind(asset_address(withdrawal))
    .bind(&withdrawal.amount)
    .bind(nonce)
    .bind(block_number)
    .bind(block_hash)
    .bind(block_timestamp)
    .bind(transaction_index)
    .bind(log_index)
    .bind(confirmations)
    .bind(request_unused)
    .bind(nonce_unused)
    .bind(no_event)
    .bind(attempts_terminal)
    .bind(primary_provider)
    .bind(secondary_provider)
    .bind(confirmation_policy_version)
    .bind(evidence)
    .bind(evidence_hash.as_slice())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_policy_decision(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<Option<StoredWithdrawalPolicyDecision>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredWithdrawalPolicyDecision>(
        r#"
        SELECT id, withdrawal_id, policy_version, disposition,
               compliance_provider, compliance_reference, compliance_disposition,
               reason_codes, evidence, evidence_hash, evaluated_by, evaluated_at
        FROM ledger_withdrawal_policy_decisions
        WHERE withdrawal_id = $1
        "#,
    )
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn load_existing_reservation(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReserveWithdrawal,
) -> Result<Option<StoredWithdrawal>, LedgerError> {
    Ok(sqlx::query_as::<_, StoredWithdrawal>(&format!(
        "{} WHERE withdrawal.id = $1 OR (lower(withdrawal.customer_reference) = lower($2) AND withdrawal.nonce = $3) FOR UPDATE OF withdrawal",
        withdrawal_select()
    ))
    .bind(command.withdrawal_id)
    .bind(&command.customer_reference)
    .bind(BigDecimal::from(command.nonce))
    .fetch_optional(&mut **tx)
    .await?)
}

async fn load_withdrawal(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<StoredWithdrawal, LedgerError> {
    sqlx::query_as::<_, StoredWithdrawal>(&format!(
        "{} WHERE withdrawal.id = $1",
        withdrawal_select()
    ))
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::WithdrawalNotFound(withdrawal_id))
}

async fn load_withdrawal_for_update(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
) -> Result<StoredWithdrawal, LedgerError> {
    sqlx::query_as::<_, StoredWithdrawal>(&format!(
        "{} WHERE withdrawal.id = $1 FOR UPDATE OF withdrawal",
        withdrawal_select()
    ))
    .bind(withdrawal_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(LedgerError::WithdrawalNotFound(withdrawal_id))
}

async fn ensure_reserved_account(
    tx: &mut Transaction<'_, Postgres>,
    asset_id: Uuid,
    customer_reference: &str,
) -> Result<Uuid, LedgerError> {
    let candidate = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO ledger_accounts
            (id, asset_id, owner_type, owner_reference, account_type, allow_negative)
        VALUES ($1, $2, 'customer', lower($3), 'customer_withdrawal_reserved', false)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(candidate)
    .bind(asset_id)
    .bind(customer_reference)
    .execute(&mut **tx)
    .await?;
    let account_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id FROM ledger_accounts
        WHERE asset_id = $1 AND owner_type = 'customer'
          AND lower(owner_reference) = lower($2)
          AND account_type = 'customer_withdrawal_reserved'
        "#,
    )
    .bind(asset_id)
    .bind(customer_reference)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO ledger_account_balances (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .execute(&mut **tx)
    .await?;
    Ok(account_id)
}

async fn insert_history(
    tx: &mut Transaction<'_, Postgres>,
    withdrawal_id: Uuid,
    from: Option<WithdrawalState>,
    to: WithdrawalState,
    reason: &str,
    evidence: serde_json::Value,
    actor_reference: Option<&str>,
) -> Result<(), LedgerError> {
    sqlx::query(
        r#"
        INSERT INTO ledger_withdrawal_state_history
            (withdrawal_id, from_state, to_state, reason, evidence, actor_reference)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(withdrawal_id)
    .bind(from.map(WithdrawalState::as_str))
    .bind(to.as_str())
    .bind(reason)
    .bind(evidence)
    .bind(actor_reference)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_posted_transaction(
    tx: &mut Transaction<'_, Postgres>,
    transaction_id: Uuid,
) -> Result<(PostedTransaction, [u8; 32]), LedgerError> {
    let row = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, Vec<u8>)>(
        r#"
        SELECT id, idempotency_key, created_at, request_hash
        FROM ledger_journal_transactions WHERE id = $1
        "#,
    )
    .bind(transaction_id)
    .fetch_one(&mut **tx)
    .await?;
    let hash: [u8; 32] = row.3.try_into().map_err(|_| {
        LedgerError::InvalidWithdrawal("journal request hash is malformed".to_owned())
    })?;
    Ok((
        PostedTransaction {
            id: row.0,
            idempotency_key: row.1,
            created_at: row.2,
        },
        hash,
    ))
}

fn validate_reservation(command: &ReserveWithdrawal) -> Result<(), LedgerError> {
    if command.withdrawal_id == Uuid::nil()
        || command.reservation_transaction_id == Uuid::nil()
        || command.customer_reference.trim().is_empty()
        || command.customer_reference.len() > 255
        || !is_asset_code(&command.asset_code)
        || !is_evm_address(&command.destination_address)
        || command.amount <= BigDecimal::zero()
        || !command.amount.is_integer()
    {
        return Err(LedgerError::InvalidWithdrawal(
            "withdrawal identity, destination, asset, and integer amount are required".to_owned(),
        ));
    }
    Ok(())
}

fn validate_approval(command: &ApproveWithdrawal) -> Result<(), LedgerError> {
    let now = Utc::now();
    let lifetime = command.expires_at - command.approved_at;
    if command.authorization_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || command.policy_version.is_empty()
        || command.policy_version.len() > 255
        || !command
            .policy_version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        || command.approved_by.trim().is_empty()
        || command.approved_by.len() > 255
        || !command.approved_by.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-' | b'@')
        })
        || command.approved_at > now
        || command.expires_at <= now
        || command.expires_at.timestamp_subsec_nanos() != 0
        || lifetime > chrono::Duration::hours(1)
    {
        return Err(LedgerError::InvalidWithdrawal(
            "withdrawal approval identity, policy, and one-hour maximum expiry are required"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_broadcast_attempt(
    command: &RecordWithdrawalBroadcastAttempt,
) -> Result<(), LedgerError> {
    if command.attempt_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || !is_evm_hash(&command.transaction_hash)
        || !is_evm_address(&command.sender_address)
        || command.calldata_hash == [0; 32]
        || !is_policy_reference(&command.provider_name, 255)
        || !is_policy_reference(&command.provider_reference, 1000)
        || !is_operator_reference(&command.submitted_by)
        || command.submitted_at > Utc::now()
    {
        return Err(LedgerError::InvalidWithdrawal(
            "broadcast attempt identity, exact transaction evidence, provider, and actor are required"
                .to_owned(),
        ));
    }
    Ok(())
}

fn ensure_same_broadcast_attempt(
    existing: &WithdrawalBroadcastAttempt,
    command: &RecordWithdrawalBroadcastAttempt,
) -> Result<(), LedgerError> {
    if existing.id != command.attempt_id
        || existing.withdrawal_id != command.withdrawal_id
        || !existing
            .transaction_hash
            .eq_ignore_ascii_case(&command.transaction_hash)
        || !existing
            .sender_address
            .eq_ignore_ascii_case(&command.sender_address)
        || existing.sender_nonce != command.sender_nonce
        || existing.calldata_hash != command.calldata_hash
        || existing.replacement_of != command.replacement_of
        || existing.provider_name != command.provider_name
        || existing.provider_reference != command.provider_reference
        || existing.submitted_by != command.submitted_by
        || existing.submitted_at.timestamp_micros() != command.submitted_at.timestamp_micros()
    {
        return Err(LedgerError::IdempotencyConflict);
    }
    Ok(())
}

fn validate_finalized_execution(command: &FinalizedWithdrawalExecution) -> Result<(), LedgerError> {
    if command.observation_id == Uuid::nil()
        || command.completion_transaction_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || command.deployment_id == Uuid::nil()
        || !is_evm_hash(&command.transaction_hash)
        || command.block_number < 0
        || !is_evm_hash(&command.block_hash)
        || command.transaction_index < 0
        || command.log_index < 0
        || command.confirmations <= 0
        || !valid_two_provider_evidence(&command.primary_provider, &command.secondary_provider)
        || !is_policy_reference(&command.confirmation_policy_version, 255)
        || !command.evidence.is_object()
    {
        return Err(LedgerError::InvalidWithdrawal(
            "finalized withdrawal execution requires exact block, log, policy, and independent provider evidence"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_finalized_non_execution(
    command: &FinalizedWithdrawalNonExecution,
) -> Result<(), LedgerError> {
    if command.observation_id == Uuid::nil()
        || command.release_transaction_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || command.deployment_id == Uuid::nil()
        || command
            .last_transaction_hash
            .as_deref()
            .is_some_and(|hash| !is_evm_hash(hash))
        || command.block_number < 0
        || !is_evm_hash(&command.block_hash)
        || command.confirmations <= 0
        || !valid_two_provider_evidence(&command.primary_provider, &command.secondary_provider)
        || !is_policy_reference(&command.confirmation_policy_version, 255)
        || !command.request_id_unused
        || !command.withdrawal_nonce_unused
        || !command.no_execution_event
        || !command.all_attempts_terminal
        || !command.evidence.is_object()
    {
        return Err(LedgerError::InvalidWithdrawal(
            "release requires finalized expiry, unused request and nonce, no event, terminal attempts, and two independent providers"
                .to_owned(),
        ));
    }
    Ok(())
}

fn valid_two_provider_evidence(primary: &str, secondary: &str) -> bool {
    is_policy_reference(primary, 255)
        && is_policy_reference(secondary, 255)
        && !primary.eq_ignore_ascii_case(secondary)
}

fn finalized_execution_evidence(
    withdrawal: &StoredWithdrawal,
    command: &FinalizedWithdrawalExecution,
) -> serde_json::Value {
    json!({
        "kind": "vela.withdrawal.finalized_execution.v1",
        "withdrawal_id": withdrawal.id,
        "deployment_id": command.deployment_id,
        "transaction_hash": command.transaction_hash.to_ascii_lowercase(),
        "block_number": command.block_number,
        "block_hash": command.block_hash.to_ascii_lowercase(),
        "block_timestamp": command.block_timestamp,
        "transaction_index": command.transaction_index,
        "log_index": command.log_index,
        "confirmations": command.confirmations,
        "primary_provider": command.primary_provider,
        "secondary_provider": command.secondary_provider,
        "confirmation_policy_version": command.confirmation_policy_version,
        "request_id": withdrawal.id,
        "customer": withdrawal.customer_reference.to_ascii_lowercase(),
        "destination": withdrawal.destination_address.to_ascii_lowercase(),
        "asset": asset_address(withdrawal).to_ascii_lowercase(),
        "amount": withdrawal.amount.normalized().to_plain_string(),
        "nonce": withdrawal.nonce.as_ref().map(|value| value.normalized().to_plain_string()),
        "provider_evidence": command.evidence,
    })
}

fn finalized_non_execution_evidence(
    withdrawal: &StoredWithdrawal,
    command: &FinalizedWithdrawalNonExecution,
) -> serde_json::Value {
    json!({
        "kind": "vela.withdrawal.finalized_non_execution.v1",
        "withdrawal_id": withdrawal.id,
        "deployment_id": command.deployment_id,
        "last_transaction_hash": command.last_transaction_hash.as_ref().map(|hash| hash.to_ascii_lowercase()),
        "block_number": command.block_number,
        "block_hash": command.block_hash.to_ascii_lowercase(),
        "block_timestamp": command.block_timestamp,
        "confirmations": command.confirmations,
        "primary_provider": command.primary_provider,
        "secondary_provider": command.secondary_provider,
        "confirmation_policy_version": command.confirmation_policy_version,
        "request_id_unused": command.request_id_unused,
        "withdrawal_nonce_unused": command.withdrawal_nonce_unused,
        "no_execution_event": command.no_execution_event,
        "all_attempts_terminal": command.all_attempts_terminal,
        "request_id": withdrawal.id,
        "customer": withdrawal.customer_reference.to_ascii_lowercase(),
        "destination": withdrawal.destination_address.to_ascii_lowercase(),
        "asset": asset_address(withdrawal).to_ascii_lowercase(),
        "amount": withdrawal.amount.normalized().to_plain_string(),
        "nonce": withdrawal.nonce.as_ref().map(|value| value.normalized().to_plain_string()),
        "provider_evidence": command.evidence,
    })
}

fn json_hash(value: &serde_json::Value) -> Result<[u8; 32], LedgerError> {
    let canonical = serde_json::to_vec(value).map_err(|error| {
        LedgerError::InvalidWithdrawal(format!("withdrawal evidence cannot be encoded: {error}"))
    })?;
    Ok(Sha256::digest(canonical).into())
}

fn validate_policy_evaluation(command: &EvaluateWithdrawalPolicy) -> Result<(), LedgerError> {
    if command.decision_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || !is_operator_reference(&command.evaluated_by)
        || !is_policy_reference(&command.compliance_provider, 255)
        || !is_policy_reference(&command.compliance_reference, 1000)
    {
        return Err(LedgerError::InvalidWithdrawal(
            "policy decision identity, evaluator, provider, and evidence reference are required"
                .to_owned(),
        ));
    }
    Ok(())
}

fn ensure_same_policy_evaluation(
    existing: &WithdrawalPolicyDecision,
    command: &EvaluateWithdrawalPolicy,
) -> Result<(), LedgerError> {
    if existing.id != command.decision_id
        || existing.withdrawal_id != command.withdrawal_id
        || existing.evaluated_by != command.evaluated_by
        || existing.compliance_provider != command.compliance_provider
        || existing.compliance_reference != command.compliance_reference
        || existing.compliance_disposition != command.compliance_disposition
    {
        return Err(LedgerError::IdempotencyConflict);
    }
    Ok(())
}

fn validate_global_halt(command: &ActivateGlobalWithdrawalHalt) -> Result<(), LedgerError> {
    if command.halt_id == Uuid::nil()
        || !is_policy_reference(&command.reason, 1000)
        || !command.evidence.is_object()
        || !is_operator_reference(&command.activated_by)
    {
        return Err(LedgerError::InvalidWithdrawal(
            "global withdrawal halt identity, reason, evidence, and actor are required".to_owned(),
        ));
    }
    Ok(())
}

fn validate_global_halt_resolution(
    command: &ResolveGlobalWithdrawalHalt,
) -> Result<(), LedgerError> {
    if command.halt_id == Uuid::nil()
        || !command.resolution_evidence.is_object()
        || !is_operator_reference(&command.resolution_initiated_by)
        || !is_operator_reference(&command.resolved_by)
        || command
            .resolution_initiated_by
            .eq_ignore_ascii_case(&command.resolved_by)
    {
        return Err(LedgerError::InvalidWithdrawal(
            "global halt resolution requires evidence and distinct valid actors".to_owned(),
        ));
    }
    Ok(())
}

fn is_operator_reference(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 255
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-' | b'@')
        })
}

fn is_policy_reference(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_len
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn authorization_payload(
    withdrawal: &StoredWithdrawal,
    nonce: &BigDecimal,
    command: &ApproveWithdrawal,
) -> String {
    format!(
        "vela.withdrawal.authorization.eip712.v2|request_id={}|chain_id={}|vault={}|customer={}|destination={}|asset={}|asset_address={}|amount_atomic={}|nonce={}|deadline={}|policy={}|approved_by={}",
        withdrawal.id,
        withdrawal.chain_id,
        withdrawal.vault_address.to_ascii_lowercase(),
        withdrawal.customer_reference.to_ascii_lowercase(),
        withdrawal.destination_address.to_ascii_lowercase(),
        withdrawal.asset_code,
        asset_address(withdrawal).to_ascii_lowercase(),
        withdrawal.amount.normalized().to_plain_string(),
        nonce.normalized().to_plain_string(),
        command.expires_at.timestamp(),
        command.policy_version,
        command.approved_by,
    )
}

struct ContractAuthorizationHashes {
    domain_separator: [u8; 32],
    struct_hash: [u8; 32],
    policy_hash: [u8; 32],
    approver_hash: [u8; 32],
    digest: [u8; 32],
}

fn contract_authorization_hashes(
    withdrawal: &StoredWithdrawal,
    nonce: &BigDecimal,
    command: &ApproveWithdrawal,
) -> Result<ContractAuthorizationHashes, LedgerError> {
    let domain_type_hash: [u8; 32] = Keccak256::digest(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    )
    .into();
    let withdrawal_type_hash: [u8; 32] = Keccak256::digest(
        b"Withdrawal(bytes16 requestId,address user,address destination,address asset,uint256 amount,uint256 nonce,uint256 deadline,bytes32 policyHash,bytes32 approverHash)",
    )
    .into();
    let name_hash: [u8; 32] = Keccak256::digest(b"VelaSettlement").into();
    let version_hash: [u8; 32] = Keccak256::digest(b"2").into();
    let policy_hash: [u8; 32] = Keccak256::digest(command.policy_version.as_bytes()).into();
    let approver_hash: [u8; 32] = Keccak256::digest(command.approved_by.as_bytes()).into();

    let mut domain = Vec::with_capacity(32 * 5);
    domain.extend_from_slice(&domain_type_hash);
    domain.extend_from_slice(&name_hash);
    domain.extend_from_slice(&version_hash);
    domain.extend_from_slice(&uint_word(&withdrawal.chain_id.to_string())?);
    domain.extend_from_slice(&address_word(&withdrawal.vault_address)?);
    let domain_separator: [u8; 32] = Keccak256::digest(domain).into();

    let mut request_id_word = [0_u8; 32];
    request_id_word[..16].copy_from_slice(withdrawal.id.as_bytes());
    let mut encoded = Vec::with_capacity(32 * 10);
    encoded.extend_from_slice(&withdrawal_type_hash);
    encoded.extend_from_slice(&request_id_word);
    encoded.extend_from_slice(&address_word(&withdrawal.customer_reference)?);
    encoded.extend_from_slice(&address_word(&withdrawal.destination_address)?);
    encoded.extend_from_slice(&address_word(asset_address(withdrawal))?);
    encoded.extend_from_slice(&uint_word(
        &withdrawal.amount.normalized().to_plain_string(),
    )?);
    encoded.extend_from_slice(&uint_word(&nonce.normalized().to_plain_string())?);
    encoded.extend_from_slice(&uint_word(&command.expires_at.timestamp().to_string())?);
    encoded.extend_from_slice(&policy_hash);
    encoded.extend_from_slice(&approver_hash);
    let struct_hash: [u8; 32] = Keccak256::digest(encoded).into();

    let mut envelope = Vec::with_capacity(66);
    envelope.extend_from_slice(b"\x19\x01");
    envelope.extend_from_slice(&domain_separator);
    envelope.extend_from_slice(&struct_hash);
    let digest: [u8; 32] = Keccak256::digest(envelope).into();
    Ok(ContractAuthorizationHashes {
        domain_separator,
        struct_hash,
        policy_hash,
        approver_hash,
        digest,
    })
}

fn address_word(value: &str) -> Result<[u8; 32], LedgerError> {
    if !is_evm_address_or_zero(value) {
        return Err(LedgerError::InvalidWithdrawal(
            "contract authorization contains an invalid EVM address".to_owned(),
        ));
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes()[2..].chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or_else(|| {
            LedgerError::InvalidWithdrawal("contract address hex is invalid".to_owned())
        })?;
        let low = hex_nibble(pair[1]).ok_or_else(|| {
            LedgerError::InvalidWithdrawal("contract address hex is invalid".to_owned())
        })?;
        output[12 + index] = (high << 4) | low;
    }
    Ok(output)
}

fn uint_word(value: &str) -> Result<[u8; 32], LedgerError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LedgerError::InvalidWithdrawal(
            "contract authorization integer is invalid".to_owned(),
        ));
    }
    let mut output = [0_u8; 32];
    for digit in value.bytes().map(|byte| byte - b'0') {
        let mut carry = u16::from(digit);
        for byte in output.iter_mut().rev() {
            let next = u16::from(*byte) * 10 + carry;
            *byte = next as u8;
            carry = next >> 8;
        }
        if carry != 0 {
            return Err(LedgerError::InvalidWithdrawal(
                "contract authorization integer exceeds uint256".to_owned(),
            ));
        }
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn asset_address(withdrawal: &StoredWithdrawal) -> &str {
    withdrawal
        .asset_contract_address
        .as_deref()
        .unwrap_or("0x0000000000000000000000000000000000000000")
}

fn ensure_same_authorization(
    existing: &WithdrawalAuthorization,
    withdrawal: &StoredWithdrawal,
    command: &ApproveWithdrawal,
) -> Result<(), LedgerError> {
    let nonce = withdrawal.nonce.as_ref().ok_or_else(|| {
        LedgerError::InvalidWithdrawal("withdrawal authorization requires a nonce".to_owned())
    })?;
    let expected_payload = authorization_payload(withdrawal, nonce, command);
    let expected_hashes = contract_authorization_hashes(withdrawal, nonce, command)?;
    if existing.id != command.authorization_id
        || existing.canonical_payload != expected_payload
        || existing.digest != expected_hashes.digest
        || existing.domain_separator != expected_hashes.domain_separator
        || existing.struct_hash != expected_hashes.struct_hash
        || existing.policy_hash != expected_hashes.policy_hash
        || existing.approver_hash != expected_hashes.approver_hash
        || existing.policy_version != command.policy_version
        || existing.approved_by != command.approved_by
        || existing.approved_at.timestamp_micros() != command.approved_at.timestamp_micros()
        || existing.expires_at.timestamp_micros() != command.expires_at.timestamp_micros()
    {
        return Err(LedgerError::IdempotencyConflict);
    }
    Ok(())
}

fn validate_signature_record(command: &RecordWithdrawalSignature) -> Result<(), LedgerError> {
    if command.signature_id == Uuid::nil()
        || command.withdrawal_id == Uuid::nil()
        || command.authorization_id == Uuid::nil()
        || command.signer_key_id.trim().is_empty()
        || command.signer_key_id.len() > 255
        || !matches!(command.signer_public_key.len(), 33 | 65)
        || command.signature[..64].iter().all(|byte| *byte == 0)
        || command.signature[64] > 1
        || command.signed_at > Utc::now()
    {
        return Err(LedgerError::InvalidWithdrawal(
            "canonical recoverable withdrawal signature evidence is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_same_signature(
    existing: &WithdrawalSignatureRecord,
    command: &RecordWithdrawalSignature,
) -> Result<(), LedgerError> {
    if existing.withdrawal_id != command.withdrawal_id
        || existing.authorization_id != command.authorization_id
        || existing.authorization_digest != command.authorization_digest
        || existing.signer_key_id != command.signer_key_id
        || existing.signer_public_key != command.signer_public_key
        || existing.algorithm != "secp256k1_recoverable"
    {
        return Err(LedgerError::IdempotencyConflict);
    }
    Ok(())
}

fn fixed_bytes<const N: usize>(value: Vec<u8>, label: &str) -> Result<[u8; N], LedgerError> {
    value.try_into().map_err(|_| {
        LedgerError::InvalidWithdrawal(format!("stored {label} has an invalid length"))
    })
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn ensure_same_reservation(
    existing: &StoredWithdrawal,
    command: &ReserveWithdrawal,
) -> Result<(), LedgerError> {
    if existing.id != command.withdrawal_id
        || !existing
            .customer_reference
            .eq_ignore_ascii_case(&command.customer_reference)
        || existing.asset_code != command.asset_code
        || !existing
            .destination_address
            .eq_ignore_ascii_case(&command.destination_address)
        || existing.amount != command.amount
        || existing.nonce != Some(BigDecimal::from(command.nonce))
        || existing.expires_at != command.expires_at
    {
        return Err(LedgerError::IdempotencyConflict);
    }
    Ok(())
}

fn reservation_hash(command: &ReserveWithdrawal) -> [u8; 32] {
    let mut hash = Sha256::new();
    for value in [
        command.withdrawal_id.to_string(),
        command.customer_reference.to_ascii_lowercase(),
        command.asset_code.clone(),
        command.destination_address.to_ascii_lowercase(),
        command.amount.normalized().to_plain_string(),
        command.nonce.to_string(),
        command
            .expires_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_default(),
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hash.finalize().into()
}

fn release_hash(withdrawal: &StoredWithdrawal) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"customer_cancelled_before_signing");
    hash.update(withdrawal.id.as_bytes());
    hash.update(withdrawal.amount.normalized().to_plain_string().as_bytes());
    hash.finalize().into()
}

fn is_asset_code(value: &str) -> bool {
    (2..=16).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn is_evm_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
        && value[2..].bytes().any(|byte| byte != b'0')
}

fn is_evm_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_evm_address_or_zero(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eip712_digest_matches_settlement_contract_vector() {
        let timestamp = DateTime::from_timestamp(2_000_000_000, 0).unwrap();
        let withdrawal = StoredWithdrawal {
            id: Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap(),
            customer_reference: "0x000000000000000000000000000000000000beef".to_owned(),
            source_account_id: Uuid::new_v4(),
            asset_id: Uuid::new_v4(),
            asset_code: "ETH".to_owned(),
            asset_decimals: 18,
            asset_contract_address: None,
            destination_address: "0x000000000000000000000000000000000000d357".to_owned(),
            amount: BigDecimal::from(500_000_000_000_000_000_i64),
            state: "approved".to_owned(),
            reservation_transaction_id: Some(Uuid::new_v4()),
            completion_transaction_id: None,
            release_transaction_id: None,
            chain_id: 84_532,
            vault_address: "0x111122223333444455556666777788889999aaaa".to_owned(),
            nonce: Some(BigDecimal::from(42)),
            expires_at: Some(timestamp),
            transaction_hash: None,
            created_at: timestamp,
            updated_at: timestamp,
        };
        let hashes = contract_authorization_hashes(
            &withdrawal,
            withdrawal.nonce.as_ref().unwrap(),
            &ApproveWithdrawal {
                authorization_id: Uuid::new_v4(),
                withdrawal_id: withdrawal.id,
                policy_version: "policy-vector".to_owned(),
                approved_by: "approver-vector".to_owned(),
                approved_at: timestamp,
                expires_at: timestamp,
            },
        )
        .unwrap();
        assert_eq!(
            hex_digest(&hashes.digest),
            "e4e370d5cee128859be93c6c83d94a246f2fb7996d6fc9499ea81e0277fc89a0"
        );
    }
}

fn is_withdrawal_nonce_race(error: &LedgerError) -> bool {
    matches!(
        error,
        LedgerError::Database(sqlx::Error::Database(database_error))
            if database_error.code().as_deref() == Some("23505")
                && database_error.constraint() == Some("ledger_withdrawals_customer_nonce")
    )
}

fn is_withdrawal_evidence_race(error: &LedgerError) -> bool {
    matches!(
        error,
        LedgerError::Database(sqlx::Error::Database(database_error))
            if database_error.code().as_deref() == Some("23505")
                && matches!(
                    database_error.constraint(),
                    Some(
                        "ledger_withdrawal_authorizations_withdrawal_id_key"
                            | "ledger_withdrawal_authorizations_digest_key"
                            | "ledger_withdrawal_signatures_withdrawal_id_key"
                            | "ledger_withdrawal_signatures_authorization_id_key"
                            | "ledger_withdrawal_policy_decisions_withdrawal_id_key"
                    )
                )
    )
}

async fn set_serializable(tx: &mut Transaction<'_, Postgres>) -> Result<(), LedgerError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

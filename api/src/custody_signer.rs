use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ledger::{
    Ledger, LedgerError, RecordWithdrawalSignature, WithdrawalAuthorization,
    WithdrawalSignatureRecord,
};
use sha3::{Digest, Keccak256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodySignRequest {
    pub authorization_id: Uuid,
    pub withdrawal_id: Uuid,
    pub digest: [u8; 32],
    pub chain_id: i64,
    pub vault_address: String,
    pub expires_at: DateTime<Utc>,
}

impl From<&WithdrawalAuthorization> for CustodySignRequest {
    fn from(value: &WithdrawalAuthorization) -> Self {
        Self {
            authorization_id: value.id,
            withdrawal_id: value.withdrawal_id,
            digest: value.digest,
            chain_id: value.chain_id,
            vault_address: value.vault_address.clone(),
            expires_at: value.expires_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodySignerIdentity {
    pub signer_key_id: String,
    pub signer_public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodySignature {
    pub signature: [u8; 65],
}

/// Check the on-chain deployment and operator before an opt-in testnet signer
/// is allowed to advance an approved authorization to Signing.
#[allow(clippy::too_many_arguments)]
pub fn verify_testnet_signer_preflight(
    authorization_chain: i64,
    authorization_vault: &str,
    configured_chain: i64,
    configured_vault: &str,
    rpc_chain: u64,
    pinned_runtime_hash: &str,
    runtime_code: &[u8],
    operator_word: &[u8],
    signer_public_key: &[u8],
) -> Result<(), CustodySignerError> {
    if configured_chain != 84532
        || authorization_chain != configured_chain
        || !authorization_vault.eq_ignore_ascii_case(configured_vault)
        || rpc_chain != configured_chain as u64
    {
        return Err(CustodySignerError::InvalidEvidence(
            "authorization, signer, and RPC deployment differ".into(),
        ));
    }
    if runtime_code.is_empty()
        || !format!("0x{}", hex::encode(Keccak256::digest(runtime_code)))
            .eq_ignore_ascii_case(pinned_runtime_hash)
    {
        return Err(CustodySignerError::InvalidEvidence(
            "vault bytecode differs from immutable deployment registry".into(),
        ));
    }
    let public_key = k256::ecdsa::VerifyingKey::from_sec1_bytes(signer_public_key)
        .map_err(|_| CustodySignerError::InvalidEvidence("signer public key is malformed".into()))?
        .to_encoded_point(false);
    let address = Keccak256::digest(&public_key.as_bytes()[1..]);
    if operator_word.len() != 32
        || operator_word[..12].iter().any(|byte| *byte != 0)
        || operator_word[12..] != address[12..]
    {
        return Err(CustodySignerError::InvalidEvidence(
            "signer does not match the vault operator".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CustodySignerError {
    #[error("custody signer is not configured")]
    Disabled,
    #[error("custody authorization expired before signing")]
    AuthorizationExpired,
    #[error("custody signer returned invalid evidence: {0}")]
    InvalidEvidence(String),
    #[error("custody signer provider failed: {0}")]
    Provider(String),
}

pub trait CustodySigner: Send + Sync {
    fn identity(&self) -> Result<CustodySignerIdentity, CustodySignerError>;

    fn sign<'a>(
        &'a self,
        request: &'a CustodySignRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CustodySignature, CustodySignerError>> + Send + 'a>>;
}

/// Explicit opt-in signer for isolated testnet rehearsals. Never wired into API startup.
/// A deployment-bound key cannot sign a request for another chain or vault.
pub struct TestnetLocalCustodySigner {
    key: k256::ecdsa::SigningKey,
    key_id: String,
    chain_id: i64,
    vault_address: String,
}
impl TestnetLocalCustodySigner {
    pub fn new(
        mode: types::RuntimeMode,
        chain_id: i64,
        vault_address: String,
        key_id: String,
        secret_hex: &str,
    ) -> Result<Self, CustodySignerError> {
        if mode != types::RuntimeMode::TestnetDemo
            || chain_id != 84532
            || types::UserId::from_hex(&vault_address).is_err()
            || vault_address.eq_ignore_ascii_case(&format!("0x{}", "00".repeat(20)))
            || key_id.trim().is_empty()
            || key_id.len() > 255
        {
            return Err(CustodySignerError::InvalidEvidence(
                "local signer requires an explicit supported testnet and vault".into(),
            ));
        }
        let decoded = zeroize::Zeroizing::new(
            hex::decode(secret_hex.strip_prefix("0x").unwrap_or(secret_hex)).map_err(|_| {
                CustodySignerError::InvalidEvidence("testnet key is malformed".into())
            })?,
        );
        let key = k256::ecdsa::SigningKey::from_slice(&decoded)
            .map_err(|_| CustodySignerError::InvalidEvidence("testnet key is malformed".into()))?;
        Ok(Self {
            key,
            key_id,
            chain_id,
            vault_address: vault_address.to_ascii_lowercase(),
        })
    }
}
impl CustodySigner for TestnetLocalCustodySigner {
    fn identity(&self) -> Result<CustodySignerIdentity, CustodySignerError> {
        Ok(CustodySignerIdentity {
            signer_key_id: self.key_id.clone(),
            signer_public_key: self
                .key
                .verifying_key()
                .to_encoded_point(true)
                .as_bytes()
                .to_vec(),
        })
    }
    fn sign<'a>(
        &'a self,
        request: &'a CustodySignRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CustodySignature, CustodySignerError>> + Send + 'a>>
    {
        Box::pin(async move {
            if request.chain_id != self.chain_id
                || !request
                    .vault_address
                    .eq_ignore_ascii_case(&self.vault_address)
            {
                return Err(CustodySignerError::InvalidEvidence(
                    "authorization is outside the signer deployment".into(),
                ));
            }
            if request.expires_at <= Utc::now() {
                return Err(CustodySignerError::AuthorizationExpired);
            }
            let (signature, recovery) = self
                .key
                .sign_prehash_recoverable(&request.digest)
                .map_err(|_| CustodySignerError::Provider("testnet signing failed".into()))?;
            let mut bytes = [0; 65];
            bytes[..64].copy_from_slice(&signature.to_bytes());
            bytes[64] = recovery.to_byte();
            Ok(CustodySignature { signature: bytes })
        })
    }
}

#[derive(Debug, Default)]
pub struct DisabledCustodySigner;

impl CustodySigner for DisabledCustodySigner {
    fn identity(&self) -> Result<CustodySignerIdentity, CustodySignerError> {
        Err(CustodySignerError::Disabled)
    }

    fn sign<'a>(
        &'a self,
        _request: &'a CustodySignRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CustodySignature, CustodySignerError>> + Send + 'a>>
    {
        Box::pin(async { Err(CustodySignerError::Disabled) })
    }
}

#[derive(Debug, Error)]
pub enum CustodySigningError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error(transparent)]
    Signer(#[from] CustodySignerError),
}

/// Provider-neutral orchestration boundary for a dedicated custody worker.
///
/// Production construction intentionally defaults to `DisabledCustodySigner`.
/// A future HSM or MPC adapter must implement `CustodySigner`; it never receives
/// database credentials or mutable withdrawal fields, only the immutable digest.
pub struct CustodySigningService {
    ledger: Arc<Ledger>,
    signer: Arc<dyn CustodySigner>,
}

impl CustodySigningService {
    pub fn disabled(ledger: Arc<Ledger>) -> Self {
        Self {
            ledger,
            signer: Arc::new(DisabledCustodySigner),
        }
    }

    pub fn with_signer(ledger: Arc<Ledger>, signer: Arc<dyn CustodySigner>) -> Self {
        Self { ledger, signer }
    }

    pub async fn sign_authorization(
        &self,
        withdrawal_id: Uuid,
        authorization_id: Uuid,
    ) -> Result<WithdrawalSignatureRecord, CustodySigningError> {
        if let Some(existing) = self.ledger.withdrawal_signature(withdrawal_id).await? {
            if existing.authorization_id != authorization_id {
                return Err(LedgerError::IdempotencyConflict.into());
            }
            return Ok(existing);
        }

        let signer_identity = self.signer.identity()?;
        let authorization = self
            .ledger
            .begin_withdrawal_signing(withdrawal_id, authorization_id)
            .await?;
        if authorization.expires_at <= Utc::now() {
            return Err(CustodySignerError::AuthorizationExpired.into());
        }
        let request = CustodySignRequest::from(&authorization);
        let signed = self.signer.sign(&request).await?;
        validate_provider_signature(&signer_identity, &signed, &authorization.digest)?;
        Ok(self
            .ledger
            .record_withdrawal_signature(&RecordWithdrawalSignature {
                signature_id: Uuid::new_v4(),
                withdrawal_id,
                authorization_id,
                authorization_digest: authorization.digest,
                signer_key_id: signer_identity.signer_key_id,
                signer_public_key: signer_identity.signer_public_key,
                signature: signed.signature,
                signed_at: Utc::now(),
            })
            .await?)
    }
}

fn validate_provider_signature(
    identity: &CustodySignerIdentity,
    signature: &CustodySignature,
    digest: &[u8; 32],
) -> Result<(), CustodySignerError> {
    if identity.signer_key_id.trim().is_empty()
        || identity.signer_key_id.len() > 255
        || !matches!(identity.signer_public_key.len(), 33 | 65)
    {
        return Err(CustodySignerError::InvalidEvidence(
            "expected a named secp256k1 recoverable signature with recovery ID 0 or 1".to_owned(),
        ));
    }
    let compact = k256::ecdsa::Signature::try_from(&signature.signature[..64]).map_err(|_| {
        CustodySignerError::InvalidEvidence("signature scalars are malformed".to_owned())
    })?;
    if compact.normalize_s().is_some() || signature.signature[64] > 1 {
        return Err(CustodySignerError::InvalidEvidence(
            "signature must use low-S scalars and recovery ID 0 or 1".to_owned(),
        ));
    }
    let recovery_id = k256::ecdsa::RecoveryId::try_from(signature.signature[64]).map_err(|_| {
        CustodySignerError::InvalidEvidence("recovery ID must be 0 or 1".to_owned())
    })?;
    let expected_key = k256::ecdsa::VerifyingKey::from_sec1_bytes(&identity.signer_public_key)
        .map_err(|_| {
            CustodySignerError::InvalidEvidence("signer public key is malformed".to_owned())
        })?;
    let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(digest, &compact, recovery_id)
        .map_err(|_| CustodySignerError::InvalidEvidence("signature recovery failed".to_owned()))?;
    if recovered != expected_key {
        return Err(CustodySignerError::InvalidEvidence(
            "signature does not recover to the declared signer key".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::{SigningKey, VerifyingKey};

    struct DeterministicTestSigner(SigningKey);

    impl CustodySigner for DeterministicTestSigner {
        fn identity(&self) -> Result<CustodySignerIdentity, CustodySignerError> {
            Ok(CustodySignerIdentity {
                signer_key_id: "test-only-key".to_owned(),
                signer_public_key: self
                    .0
                    .verifying_key()
                    .to_encoded_point(true)
                    .as_bytes()
                    .to_vec(),
            })
        }

        fn sign<'a>(
            &'a self,
            request: &'a CustodySignRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CustodySignature, CustodySignerError>> + Send + 'a>>
        {
            Box::pin(async move {
                let (signature, recovery_id) = self
                    .0
                    .sign_prehash_recoverable(&request.digest)
                    .map_err(|error| CustodySignerError::Provider(error.to_string()))?;
                let mut bytes = [0_u8; 65];
                bytes[..64].copy_from_slice(&signature.to_bytes());
                bytes[64] = recovery_id.to_byte();
                Ok(CustodySignature { signature: bytes })
            })
        }
    }

    fn request() -> CustodySignRequest {
        CustodySignRequest {
            authorization_id: Uuid::from_u128(1),
            withdrawal_id: Uuid::from_u128(2),
            digest: [7; 32],
            chain_id: 84532,
            vault_address: format!("0x{}", "11".repeat(20)),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        }
    }

    #[test]
    fn preflight_requires_registered_base_v3_code_and_matching_operator() {
        let key = SigningKey::from_slice(&[9; 32]).unwrap();
        let public_key = key.verifying_key().to_encoded_point(true);
        let uncompressed = key.verifying_key().to_encoded_point(false);
        let address = Keccak256::digest(&uncompressed.as_bytes()[1..]);
        let mut operator_word = [0_u8; 32];
        operator_word[12..].copy_from_slice(&address[12..]);
        let runtime_code = [0x60_u8, 0x00, 0x60, 0x00];
        let code_hash = format!("0x{}", hex::encode(Keccak256::digest(runtime_code)));
        let vault = "0x1111111111111111111111111111111111111111";
        let check = |authorization_chain,
                     authorization_vault: &str,
                     rpc_chain,
                     pinned_hash: &str,
                     code: &[u8],
                     operator: &[u8]| {
            verify_testnet_signer_preflight(
                authorization_chain,
                authorization_vault,
                84532,
                vault,
                rpc_chain,
                pinned_hash,
                code,
                operator,
                public_key.as_bytes(),
            )
        };
        assert!(check(
            84532,
            vault,
            84532,
            &code_hash,
            &runtime_code,
            &operator_word
        )
        .is_ok());
        assert!(check(
            11155111,
            vault,
            84532,
            &code_hash,
            &runtime_code,
            &operator_word
        )
        .is_err());
        assert!(check(
            84532,
            "0x2222222222222222222222222222222222222222",
            84532,
            &code_hash,
            &runtime_code,
            &operator_word
        )
        .is_err());
        assert!(check(
            84532,
            vault,
            8453,
            &code_hash,
            &runtime_code,
            &operator_word
        )
        .is_err());
        assert!(check(
            84532,
            vault,
            84532,
            &format!("0x{}", "00".repeat(32)),
            &runtime_code,
            &operator_word
        )
        .is_err());
        assert!(check(84532, vault, 84532, &code_hash, &[], &operator_word).is_err());
        operator_word[31] ^= 1;
        assert!(check(
            84532,
            vault,
            84532,
            &code_hash,
            &runtime_code,
            &operator_word
        )
        .is_err());
    }

    #[tokio::test]
    async fn local_signer_is_bound_to_testnet_vault_and_expiration() {
        let mut request = request();
        for mode in [
            types::RuntimeMode::Production,
            types::RuntimeMode::Development,
        ] {
            assert!(TestnetLocalCustodySigner::new(
                mode,
                request.chain_id,
                request.vault_address.clone(),
                "staging".into(),
                &"09".repeat(32)
            )
            .is_err());
        }
        assert!(TestnetLocalCustodySigner::new(
            types::RuntimeMode::TestnetDemo,
            8453,
            request.vault_address.clone(),
            "staging".into(),
            &"09".repeat(32)
        )
        .is_err());
        assert!(TestnetLocalCustodySigner::new(
            types::RuntimeMode::TestnetDemo,
            11155111,
            request.vault_address.clone(),
            "staging".into(),
            &"09".repeat(32)
        )
        .is_err());
        let signer = TestnetLocalCustodySigner::new(
            types::RuntimeMode::TestnetDemo,
            request.chain_id,
            request.vault_address.clone(),
            "staging".into(),
            &"09".repeat(32),
        )
        .unwrap();
        let signed = signer.sign(&request).await.unwrap();
        validate_provider_signature(&signer.identity().unwrap(), &signed, &request.digest).unwrap();
        request.chain_id = 11155111;
        assert!(signer.sign(&request).await.is_err());
        request.chain_id = 84532;
        request.vault_address = format!("0x{}", "22".repeat(20));
        assert!(signer.sign(&request).await.is_err());
        request.vault_address = format!("0x{}", "11".repeat(20));
        request.expires_at = Utc::now() - chrono::Duration::seconds(1);
        assert_eq!(
            signer.sign(&request).await,
            Err(CustodySignerError::AuthorizationExpired)
        );
    }

    #[tokio::test]
    async fn rejects_malleable_high_s_and_unsupported_recovery_ids() {
        let signer = DeterministicTestSigner(SigningKey::from_slice(&[9; 32]).unwrap());
        let request = request();
        let identity = signer.identity().unwrap();
        let signed = signer.sign(&request).await.unwrap();
        let mut wrong_recovery = signed.clone();
        wrong_recovery.signature[64] = 2;
        assert!(validate_provider_signature(&identity, &wrong_recovery, &request.digest).is_err());
        let compact = k256::ecdsa::Signature::try_from(&signed.signature[..64]).unwrap();
        let high_s =
            k256::ecdsa::Signature::from_scalars(compact.r().to_bytes(), (-compact.s()).to_bytes())
                .unwrap();
        assert!(high_s.normalize_s().is_some());
        let mut malleable = signed;
        malleable.signature[..64].copy_from_slice(&high_s.to_bytes());
        malleable.signature[64] ^= 1;
        assert!(validate_provider_signature(&identity, &malleable, &request.digest).is_err());
    }

    #[tokio::test]
    async fn disabled_signer_fails_closed() {
        assert_eq!(
            DisabledCustodySigner.identity(),
            Err(CustodySignerError::Disabled)
        );
    }

    #[tokio::test]
    async fn test_signer_returns_recoverable_signature_for_exact_digest() {
        let signer = DeterministicTestSigner(SigningKey::from_slice(&[9; 32]).unwrap());
        let request = request();
        let identity = signer.identity().unwrap();
        let signed = signer.sign(&request).await.unwrap();
        validate_provider_signature(&identity, &signed, &request.digest).unwrap();
        assert!(validate_provider_signature(&identity, &signed, &[8; 32]).is_err());

        let signature = k256::ecdsa::Signature::try_from(&signed.signature[..64]).unwrap();
        let recovery_id = k256::ecdsa::RecoveryId::try_from(signed.signature[64]).unwrap();
        let recovered =
            VerifyingKey::recover_from_prehash(&request.digest, &signature, recovery_id).unwrap();
        assert_eq!(recovered, *signer.0.verifying_key());
    }
}

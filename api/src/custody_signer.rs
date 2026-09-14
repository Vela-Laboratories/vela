use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ledger::{
    Ledger, LedgerError, RecordWithdrawalSignature, WithdrawalAuthorization,
    WithdrawalSignatureRecord,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodySignRequest {
    pub authorization_id: Uuid,
    pub withdrawal_id: Uuid,
    pub digest: [u8; 32],
    pub expires_at: DateTime<Utc>,
}

impl From<&WithdrawalAuthorization> for CustodySignRequest {
    fn from(value: &WithdrawalAuthorization) -> Self {
        Self {
            authorization_id: value.id,
            withdrawal_id: value.withdrawal_id,
            digest: value.digest,
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
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        }
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

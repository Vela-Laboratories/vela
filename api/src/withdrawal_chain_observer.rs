use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalObservationRequest {
    observation_id: Uuid,
    withdrawal_id: Uuid,
    chain_id: u64,
    transaction_hash: [u8; 32],
    expected_sender: Option<[u8; 20]>,
    expected_contract: [u8; 20],
    expected_calldata_hash: [u8; 32],
}

impl WithdrawalObservationRequest {
    pub fn new(
        observation_id: Uuid,
        withdrawal_id: Uuid,
        chain_id: u64,
        transaction_hash: [u8; 32],
        expected_sender: Option<[u8; 20]>,
        expected_contract: [u8; 20],
        expected_calldata_hash: [u8; 32],
    ) -> Result<Self, WithdrawalChainObserverError> {
        if observation_id.is_nil() || withdrawal_id.is_nil() {
            return Err(WithdrawalChainObserverError::InvalidRequest(
                "observation and withdrawal IDs must be non-nil".to_owned(),
            ));
        }
        if expected_sender.is_some_and(|sender| sender == [0; 20])
            || chain_id == 0
            || transaction_hash == [0; 32]
            || expected_contract == [0; 20]
            || expected_calldata_hash == [0; 32]
        {
            return Err(WithdrawalChainObserverError::InvalidRequest(
                "chain and expected transaction identity must be non-zero".to_owned(),
            ));
        }

        Ok(Self {
            observation_id,
            withdrawal_id,
            chain_id,
            transaction_hash,
            expected_sender,
            expected_contract,
            expected_calldata_hash,
        })
    }

    pub fn observation_id(&self) -> Uuid {
        self.observation_id
    }

    pub fn withdrawal_id(&self) -> Uuid {
        self.withdrawal_id
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn transaction_hash(&self) -> &[u8; 32] {
        &self.transaction_hash
    }

    pub fn expected_sender(&self) -> Option<&[u8; 20]> {
        self.expected_sender.as_ref()
    }

    pub fn expected_contract(&self) -> &[u8; 20] {
        &self.expected_contract
    }

    pub fn expected_calldata_hash(&self) -> &[u8; 32] {
        &self.expected_calldata_hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalChainObserverIdentity {
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithdrawalTransactionState {
    NotFound,
    Pending,
    Included {
        block_number: u64,
        block_hash: [u8; 32],
        confirmations: u64,
        succeeded: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedWithdrawalTransaction {
    pub transaction_hash: [u8; 32],
    pub sender: [u8; 20],
    pub destination: [u8; 20],
    pub calldata_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalTransactionObservation {
    /// `None` is valid only for `NotFound`; it avoids representing request
    /// expectations as if a provider had actually observed the transaction.
    pub transaction: Option<ObservedWithdrawalTransaction>,
    pub state: WithdrawalTransactionState,
    pub provider_reference: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WithdrawalChainObserverError {
    #[error("withdrawal chain observer is not configured")]
    Disabled,
    #[error("invalid withdrawal observation request: {0}")]
    InvalidRequest(String),
    #[error("withdrawal chain observer returned invalid evidence: {0}")]
    InvalidEvidence(String),
    #[error("withdrawal chain observer provider failed: {0}")]
    Provider(String),
}

pub trait WithdrawalChainObserver: Send + Sync {
    fn identity(&self) -> Result<WithdrawalChainObserverIdentity, WithdrawalChainObserverError>;

    fn observe<'a>(
        &'a self,
        request: &'a WithdrawalObservationRequest,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<WithdrawalTransactionObservation, WithdrawalChainObserverError>,
                > + Send
                + 'a,
        >,
    >;
}

#[derive(Debug, Default)]
pub struct DisabledWithdrawalChainObserver;

impl WithdrawalChainObserver for DisabledWithdrawalChainObserver {
    fn identity(&self) -> Result<WithdrawalChainObserverIdentity, WithdrawalChainObserverError> {
        Err(WithdrawalChainObserverError::Disabled)
    }

    fn observe<'a>(
        &'a self,
        _request: &'a WithdrawalObservationRequest,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<WithdrawalTransactionObservation, WithdrawalChainObserverError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(WithdrawalChainObserverError::Disabled) })
    }
}

/// Provider-neutral observation boundary for omnibus-vault withdrawals.
///
/// The observer receives only immutable transaction expectations and cannot
/// sign or broadcast a transaction. A relay sender is intentionally optional:
/// V3 commits the operator's signature and vault calldata, not a customer
/// address or per-user on-chain balance. Production construction deliberately
/// defaults to the disabled observer until an RPC provider is configured.
pub struct WithdrawalChainObservationService {
    observer: Arc<dyn WithdrawalChainObserver>,
}

impl WithdrawalChainObservationService {
    pub fn disabled() -> Self {
        Self {
            observer: Arc::new(DisabledWithdrawalChainObserver),
        }
    }

    pub fn with_observer(observer: Arc<dyn WithdrawalChainObserver>) -> Self {
        Self { observer }
    }

    pub async fn observe(
        &self,
        request: &WithdrawalObservationRequest,
    ) -> Result<WithdrawalTransactionObservation, WithdrawalChainObserverError> {
        let identity = self.observer.identity()?;
        validate_provider_identity(&identity)?;
        let observation = self.observer.observe(request).await?;
        validate_observation(request, &observation)?;
        Ok(observation)
    }
}

impl Default for WithdrawalChainObservationService {
    fn default() -> Self {
        Self::disabled()
    }
}

fn validate_provider_identity(
    identity: &WithdrawalChainObserverIdentity,
) -> Result<(), WithdrawalChainObserverError> {
    if identity.provider_id.trim().is_empty() || identity.provider_id.len() > 255 {
        return Err(WithdrawalChainObserverError::InvalidEvidence(
            "provider identity must be between 1 and 255 characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_observation(
    request: &WithdrawalObservationRequest,
    observation: &WithdrawalTransactionObservation,
) -> Result<(), WithdrawalChainObserverError> {
    match (&observation.state, &observation.transaction) {
        (WithdrawalTransactionState::NotFound, None) => {}
        (WithdrawalTransactionState::NotFound, Some(_))
        | (WithdrawalTransactionState::Pending, None)
        | (WithdrawalTransactionState::Included { .. }, None) => {
            return Err(WithdrawalChainObserverError::InvalidEvidence(
                "transaction evidence is inconsistent with its observed state".to_owned(),
            ));
        }
        (_, Some(transaction)) => {
            if transaction.transaction_hash != request.transaction_hash
                || request
                    .expected_sender
                    .is_some_and(|sender| transaction.sender != sender)
                || transaction.destination != request.expected_contract
                || transaction.calldata_hash != request.expected_calldata_hash
            {
                return Err(WithdrawalChainObserverError::InvalidEvidence(
                    "observed transaction does not match the immutable withdrawal request"
                        .to_owned(),
                ));
            }
        }
    }
    if observation.provider_reference.trim().is_empty()
        || observation.provider_reference.len() > 255
    {
        return Err(WithdrawalChainObserverError::InvalidEvidence(
            "provider reference must be between 1 and 255 characters".to_owned(),
        ));
    }
    if let WithdrawalTransactionState::Included {
        block_number,
        block_hash,
        confirmations,
        ..
    } = &observation.state
    {
        if *block_number == 0 || *block_hash == [0; 32] || *confirmations == 0 {
            return Err(WithdrawalChainObserverError::InvalidEvidence(
                "included transactions require block identity and at least one confirmation"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DeterministicTestObserver {
        observation: WithdrawalTransactionObservation,
    }

    impl WithdrawalChainObserver for DeterministicTestObserver {
        fn identity(
            &self,
        ) -> Result<WithdrawalChainObserverIdentity, WithdrawalChainObserverError> {
            Ok(WithdrawalChainObserverIdentity {
                provider_id: "test-only-observer".to_owned(),
            })
        }

        fn observe<'a>(
            &'a self,
            _request: &'a WithdrawalObservationRequest,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            WithdrawalTransactionObservation,
                            WithdrawalChainObserverError,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(self.observation.clone()) })
        }
    }

    fn request() -> WithdrawalObservationRequest {
        WithdrawalObservationRequest::new(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            8453,
            [3; 32],
            None,
            [5; 20],
            [6; 32],
        )
        .unwrap()
    }

    fn observation() -> WithdrawalTransactionObservation {
        WithdrawalTransactionObservation {
            transaction: Some(ObservedWithdrawalTransaction {
                transaction_hash: [3; 32],
                sender: [4; 20],
                destination: [5; 20],
                calldata_hash: [6; 32],
            }),
            state: WithdrawalTransactionState::Included {
                block_number: 123,
                block_hash: [7; 32],
                confirmations: 12,
                succeeded: true,
            },
            provider_reference: "test-observation-1".to_owned(),
            observed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn disabled_observer_fails_closed() {
        let error = WithdrawalChainObservationService::disabled()
            .observe(&request())
            .await
            .unwrap_err();
        assert_eq!(error, WithdrawalChainObserverError::Disabled);
    }

    #[tokio::test]
    async fn deterministic_observer_returns_matching_omnibus_vault_transaction() {
        let service =
            WithdrawalChainObservationService::with_observer(Arc::new(DeterministicTestObserver {
                observation: observation(),
            }));

        let observed = service.observe(&request()).await.unwrap();
        assert_eq!(
            observed.transaction.as_ref().unwrap().transaction_hash,
            [3; 32]
        );
        assert_eq!(observed.transaction.as_ref().unwrap().sender, [4; 20]);
        assert!(matches!(
            observed.state,
            WithdrawalTransactionState::Included {
                confirmations: 12,
                succeeded: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn relay_sender_is_not_treated_as_customer_identity() {
        let mut mismatched = observation();
        mismatched.transaction.as_mut().unwrap().sender = [9; 20];
        let service =
            WithdrawalChainObservationService::with_observer(Arc::new(DeterministicTestObserver {
                observation: mismatched,
            }));

        assert!(service.observe(&request()).await.is_ok());
    }

    #[tokio::test]
    async fn malformed_inclusion_evidence_is_rejected() {
        let mut malformed = observation();
        malformed.state = WithdrawalTransactionState::Included {
            block_number: 123,
            block_hash: [7; 32],
            confirmations: 0,
            succeeded: true,
        };
        let service =
            WithdrawalChainObservationService::with_observer(Arc::new(DeterministicTestObserver {
                observation: malformed,
            }));

        assert!(matches!(
            service.observe(&request()).await,
            Err(WithdrawalChainObserverError::InvalidEvidence(_))
        ));
    }

    #[tokio::test]
    async fn not_found_does_not_fabricate_transaction_evidence() {
        let mut not_found = observation();
        not_found.transaction = None;
        not_found.state = WithdrawalTransactionState::NotFound;
        let service =
            WithdrawalChainObservationService::with_observer(Arc::new(DeterministicTestObserver {
                observation: not_found,
            }));

        let observed = service.observe(&request()).await.unwrap();
        assert_eq!(observed.transaction, None);
        assert_eq!(observed.state, WithdrawalTransactionState::NotFound);
    }

    #[test]
    fn request_rejects_zero_identity_fields() {
        assert!(matches!(
            WithdrawalObservationRequest::new(
                Uuid::nil(),
                Uuid::from_u128(2),
                8453,
                [3; 32],
                Some([4; 20]),
                [5; 20],
                [6; 32],
            ),
            Err(WithdrawalChainObserverError::InvalidRequest(_))
        ));
    }
}

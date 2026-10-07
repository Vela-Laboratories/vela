//! Deterministic limits for a future, separately enabled V3 relayer.
//! A decision here does not reserve a nonce or authorize transaction submission.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuote {
    pub pending_nonce: u64,
    pub base_fee_wei: u128,
    pub priority_fee_wei: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayerLimits {
    pub max_gas_limit: u64,
    pub max_fee_per_gas_wei: u128,
    pub max_priority_fee_per_gas_wei: u128,
    pub max_transaction_cost_wei: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayerDecision {
    pub sender_nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas_wei: u128,
    pub max_priority_fee_per_gas_wei: u128,
    pub maximum_cost_wei: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayerPolicyError {
    InvalidLimits,
    ProviderDisagreement,
    InvalidQuote,
    FeeLimitExceeded,
    ArithmeticOverflow,
}

impl RelayerLimits {
    /// Require independent pending-nonce agreement. Use the higher fee quote
    /// so the transaction stays within the configured spend ceiling even when
    /// providers report different current base fees.
    pub fn decide(
        self,
        primary: ProviderQuote,
        secondary: ProviderQuote,
        estimated_gas: u64,
    ) -> Result<RelayerDecision, RelayerPolicyError> {
        if self.max_gas_limit == 0
            || self.max_fee_per_gas_wei == 0
            || self.max_priority_fee_per_gas_wei == 0
            || self.max_transaction_cost_wei == 0
        {
            return Err(RelayerPolicyError::InvalidLimits);
        }
        if primary.pending_nonce != secondary.pending_nonce {
            return Err(RelayerPolicyError::ProviderDisagreement);
        }
        if primary.base_fee_wei == 0
            || secondary.base_fee_wei == 0
            || primary.priority_fee_wei == 0
            || secondary.priority_fee_wei == 0
            || estimated_gas == 0
        {
            return Err(RelayerPolicyError::InvalidQuote);
        }
        if estimated_gas > self.max_gas_limit {
            return Err(RelayerPolicyError::FeeLimitExceeded);
        }
        let priority_fee = primary.priority_fee_wei.max(secondary.priority_fee_wei);
        let base_fee = primary.base_fee_wei.max(secondary.base_fee_wei);
        let max_fee = base_fee
            .checked_mul(2)
            .and_then(|value| value.checked_add(priority_fee))
            .ok_or(RelayerPolicyError::ArithmeticOverflow)?;
        let maximum_cost = max_fee
            .checked_mul(u128::from(estimated_gas))
            .ok_or(RelayerPolicyError::ArithmeticOverflow)?;
        if priority_fee > self.max_priority_fee_per_gas_wei
            || max_fee > self.max_fee_per_gas_wei
            || maximum_cost > self.max_transaction_cost_wei
        {
            return Err(RelayerPolicyError::FeeLimitExceeded);
        }
        Ok(RelayerDecision {
            sender_nonce: primary.pending_nonce,
            gas_limit: estimated_gas,
            max_fee_per_gas_wei: max_fee,
            max_priority_fee_per_gas_wei: priority_fee,
            maximum_cost_wei: maximum_cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RelayerLimits {
        RelayerLimits {
            max_gas_limit: 200_000,
            max_fee_per_gas_wei: 100,
            max_priority_fee_per_gas_wei: 10,
            max_transaction_cost_wei: 20_000_000,
        }
    }

    fn quote() -> ProviderQuote {
        ProviderQuote {
            pending_nonce: 7,
            base_fee_wei: 20,
            priority_fee_wei: 2,
        }
    }

    #[test]
    fn agrees_on_nonce_and_budgets_for_worst_quote() {
        let secondary = ProviderQuote {
            base_fee_wei: 25,
            priority_fee_wei: 3,
            ..quote()
        };
        let decision = limits().decide(quote(), secondary, 100_000).unwrap();
        assert_eq!(decision.sender_nonce, 7);
        assert_eq!(decision.max_fee_per_gas_wei, 53);
        assert_eq!(decision.maximum_cost_wei, 5_300_000);
    }

    #[test]
    fn refuses_nonce_disagreement_and_fee_overrun() {
        assert_eq!(
            limits().decide(
                quote(),
                ProviderQuote {
                    pending_nonce: 8,
                    ..quote()
                },
                100_000
            ),
            Err(RelayerPolicyError::ProviderDisagreement)
        );
        assert_eq!(
            limits().decide(quote(), quote(), 200_001),
            Err(RelayerPolicyError::FeeLimitExceeded)
        );
        assert_eq!(
            limits().decide(
                ProviderQuote {
                    base_fee_wei: 60,
                    ..quote()
                },
                quote(),
                100_000
            ),
            Err(RelayerPolicyError::FeeLimitExceeded)
        );
    }

    #[test]
    fn refuses_zero_quotes_and_overflow() {
        assert_eq!(
            limits().decide(
                ProviderQuote {
                    base_fee_wei: 0,
                    ..quote()
                },
                quote(),
                100_000
            ),
            Err(RelayerPolicyError::InvalidQuote)
        );
        assert_eq!(
            limits().decide(
                ProviderQuote {
                    base_fee_wei: u128::MAX,
                    ..quote()
                },
                quote(),
                100_000
            ),
            Err(RelayerPolicyError::ArithmeticOverflow)
        );
    }
}

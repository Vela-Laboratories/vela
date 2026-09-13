use bigdecimal::BigDecimal;
use engine::{FinancialCommitter, FinancialEffectSet};
use ledger::{AccountLookup, JournalLegInput, PostTransaction, ResolvedAccount};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

const ENGINE_DECIMALS: i16 = types::QUANTITY_DECIMALS as i16;
const PLATFORM_REFERENCE: &str = "platform:vela";

pub struct LedgerFinancialCommitter {
    ledger: Arc<ledger::Ledger>,
}

impl LedgerFinancialCommitter {
    pub fn new(ledger: Arc<ledger::Ledger>) -> Self {
        Self { ledger }
    }

    async fn commit_effects(&self, effects: &FinancialEffectSet) -> Result<(), String> {
        let encoded = serde_json::to_vec(effects)
            .map_err(|error| format!("could not encode financial effects: {error}"))?;
        let request_hash: [u8; 32] = Sha256::digest(&encoded).into();
        let idempotency_key = format!("matcher-effect:{}", hex::encode(request_hash));

        let mut pending_legs = Vec::new();
        let mut lookups = Vec::new();
        for delta in &effects.customer_balances {
            let owner_reference = delta.user.to_hex().to_ascii_lowercase();
            for (account_type, amount) in [
                ("customer_available", delta.available),
                ("customer_locked", delta.locked),
            ] {
                if amount == 0 {
                    continue;
                }
                let lookup = AccountLookup {
                    asset_code: delta.asset.as_str().to_owned(),
                    owner_type: "customer".to_owned(),
                    owner_reference: owner_reference.clone(),
                    account_type: account_type.to_owned(),
                };
                lookups.push(lookup.clone());
                pending_legs.push((lookup, amount));
            }
        }

        for delta in &effects.platform_balances {
            for (account_type, amount) in [
                ("fee_revenue", delta.fee_revenue),
                ("rebate_expense", delta.rebate_expense),
            ] {
                if amount == 0 {
                    continue;
                }
                let lookup = AccountLookup {
                    asset_code: delta.asset.as_str().to_owned(),
                    owner_type: "platform".to_owned(),
                    owner_reference: PLATFORM_REFERENCE.to_owned(),
                    account_type: account_type.to_owned(),
                };
                lookups.push(lookup.clone());
                pending_legs.push((lookup, amount));
            }
        }

        let resolved = self
            .ledger
            .resolve_accounts(&lookups)
            .await
            .map_err(|error| error.to_string())?;
        let legs = build_legs(pending_legs, &resolved)?;
        let metadata = serde_json::json!({
            "source": "matcher",
            "engine_decimals": ENGINE_DECIMALS,
            "effect_set": effects,
        });

        self.ledger
            .post(&PostTransaction {
                id: Uuid::new_v4(),
                operation_type: effects.operation_type.clone(),
                idempotency_key,
                request_hash,
                external_reference: Some(serde_json::json!({
                    "fills": effects.fills.iter().map(|fill| serde_json::json!({
                        "market": fill.market.0,
                        "maker_order_id": fill.maker_order_id,
                        "taker_order_id": fill.taker_order_id,
                        "timestamp": fill.timestamp,
                    })).collect::<Vec<_>>()
                })),
                metadata,
                legs,
            })
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

impl FinancialCommitter for LedgerFinancialCommitter {
    fn commit<'a>(
        &'a self,
        effects: &'a FinancialEffectSet,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { self.commit_effects(effects).await })
    }
}

fn build_legs(
    pending: Vec<(AccountLookup, i128)>,
    resolved: &HashMap<AccountLookup, ResolvedAccount>,
) -> Result<Vec<JournalLegInput>, String> {
    pending
        .into_iter()
        .map(|(lookup, amount)| {
            let account = resolved.get(&lookup).ok_or_else(|| {
                format!(
                    "resolved account disappeared for {} {}",
                    lookup.asset_code, lookup.account_type
                )
            })?;
            Ok(JournalLegInput {
                account_id: account.id,
                amount: scale_engine_amount(amount, account.asset_decimals)?,
            })
        })
        .collect()
}

fn scale_engine_amount(amount: i128, asset_decimals: i16) -> Result<BigDecimal, String> {
    if asset_decimals < 0 {
        return Err("asset decimals cannot be negative".to_owned());
    }
    if asset_decimals == ENGINE_DECIMALS {
        return Ok(BigDecimal::from(amount));
    }
    if asset_decimals > ENGINE_DECIMALS {
        let mut scaled = BigDecimal::from(amount);
        for _ in 0..(asset_decimals - ENGINE_DECIMALS) {
            scaled *= BigDecimal::from(10_u8);
        }
        return Ok(scaled);
    }

    let divisor = 10_i128.pow((ENGINE_DECIMALS - asset_decimals) as u32);
    if amount % divisor != 0 {
        return Err(format!(
            "engine amount {amount} is not representable with {asset_decimals} asset decimals"
        ));
    }
    Ok(BigDecimal::from(amount / divisor))
}

#[cfg(test)]
mod tests {
    use super::scale_engine_amount;
    use bigdecimal::BigDecimal;

    #[test]
    fn scales_engine_units_to_asset_base_units_exactly() {
        assert_eq!(
            scale_engine_amount(100_000_000, 18).unwrap(),
            BigDecimal::from(1_000_000_000_000_000_000_i128)
        );
        assert_eq!(
            scale_engine_amount(100_000_000, 6).unwrap(),
            BigDecimal::from(1_000_000_i64)
        );
        assert!(scale_engine_amount(1, 6).is_err());
    }
}

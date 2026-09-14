use bigdecimal::BigDecimal;
use engine::{FinancialCommitter, FinancialEffectSet};
use ledger::{
    AccountLookup, AccountProvisionRequest, JournalLegInput, PostTransaction, ResolvedAccount,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinancialReplayStats {
    pub replayed_accounts: usize,
    pub customer_balances: usize,
    pub platform_fee_assets: usize,
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
        let mut provisions = Vec::new();
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
                provisions.push(AccountProvisionRequest {
                    asset_code: lookup.asset_code.clone(),
                    owner_type: lookup.owner_type.clone(),
                    owner_reference: lookup.owner_reference.clone(),
                    account_type: lookup.account_type.clone(),
                    allow_negative: false,
                });
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
                provisions.push(AccountProvisionRequest {
                    asset_code: lookup.asset_code.clone(),
                    owner_type: lookup.owner_type.clone(),
                    owner_reference: lookup.owner_reference.clone(),
                    account_type: lookup.account_type.clone(),
                    allow_negative: true,
                });
                pending_legs.push((lookup, amount));
            }
        }

        let resolved = self
            .ledger
            .ensure_accounts(&provisions)
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

pub async fn restore_engine_financial_state(
    engine: &mut engine::MatchingEngine,
    ledger: &ledger::Ledger,
) -> Result<FinancialReplayStats, String> {
    let replayed = ledger
        .replay_account_balances()
        .await
        .map_err(|error| error.to_string())?;
    let mut balances: HashMap<(types::UserId, types::AssetId), types::Balance> = HashMap::new();
    let mut platform_totals: HashMap<String, i128> = HashMap::new();

    for account in &replayed {
        match (account.owner_type.as_str(), account.account_type.as_str()) {
            ("customer", "customer_available" | "customer_locked") => {
                let engine_amount = scale_ledger_amount(&account.balance, account.asset_decimals)?;
                if engine_amount < 0 {
                    return Err(format!(
                        "customer account {} replayed a negative balance",
                        account.account_id
                    ));
                }
                let amount = u64::try_from(engine_amount).map_err(|_| {
                    format!(
                        "customer account {} exceeds matching-engine capacity",
                        account.account_id
                    )
                })?;
                let user = types::UserId::from_hex(&account.owner_reference).map_err(|_| {
                    format!(
                        "customer account {} has an invalid owner reference",
                        account.account_id
                    )
                })?;
                let asset = types::AssetId::from_str(&account.asset_code);
                let balance = balances
                    .entry((user.clone(), asset))
                    .or_insert(types::Balance {
                        user,
                        asset,
                        available: 0,
                        locked: 0,
                    });
                if account.account_type == "customer_available" {
                    balance.available = amount;
                } else {
                    balance.locked = amount;
                }
            }
            ("platform", "fee_revenue" | "rebate_expense")
                if account.owner_reference == PLATFORM_REFERENCE =>
            {
                let engine_amount = scale_ledger_amount(&account.balance, account.asset_decimals)?;
                *platform_totals
                    .entry(account.asset_code.clone())
                    .or_insert(0) += engine_amount;
            }
            _ => {}
        }
    }

    let mut fee_balances = engine::EngineMap::default();
    for (asset, amount) in platform_totals {
        let amount = u64::try_from(amount).map_err(|_| {
            format!("platform fee balance for {asset} is negative or exceeds engine capacity")
        })?;
        if amount != 0 {
            fee_balances.insert(asset, amount);
        }
    }

    let customer_balance_count = balances.len();
    let platform_fee_assets = fee_balances.len();
    engine.balances = balances.into_iter().collect();
    engine.fee_balances = fee_balances;

    Ok(FinancialReplayStats {
        replayed_accounts: replayed.len(),
        customer_balances: customer_balance_count,
        platform_fee_assets,
    })
}

/// Load one customer's authoritative available/locked balances and convert
/// them into the matching engine's fixed-point units for API compatibility.
pub async fn load_customer_balances(
    ledger: &ledger::Ledger,
    user: &types::UserId,
) -> Result<Vec<types::Balance>, String> {
    let owner_reference = user.to_hex().to_ascii_lowercase();
    let accounts = ledger
        .account_balances_for_owner("customer", &owner_reference)
        .await
        .map_err(|error| error.to_string())?;
    let mut balances: HashMap<types::AssetId, types::Balance> = HashMap::new();

    for account in accounts {
        if !matches!(
            account.account_type.as_str(),
            "customer_available" | "customer_locked"
        ) {
            continue;
        }
        let engine_amount = scale_ledger_amount(&account.balance, account.asset_decimals)?;
        let amount = u64::try_from(engine_amount).map_err(|_| {
            format!(
                "customer account {} is negative or exceeds matching-engine capacity",
                account.account_id
            )
        })?;
        let asset = types::AssetId::from_str(&account.asset_code);
        let balance = balances.entry(asset).or_insert(types::Balance {
            user: user.clone(),
            asset,
            available: 0,
            locked: 0,
        });
        if account.account_type == "customer_available" {
            balance.available = amount;
        } else {
            balance.locked = amount;
        }
    }

    let mut balances: Vec<_> = balances.into_values().collect();
    balances.sort_by_key(|balance| balance.asset.0);
    Ok(balances)
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

pub(crate) fn scale_engine_amount(amount: i128, asset_decimals: i16) -> Result<BigDecimal, String> {
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

fn scale_ledger_amount(amount: &BigDecimal, asset_decimals: i16) -> Result<i128, String> {
    if asset_decimals < 0 {
        return Err("asset decimals cannot be negative".to_owned());
    }
    if !amount.is_integer() {
        return Err(format!(
            "ledger amount {amount} is not an integer base-unit value"
        ));
    }
    let raw = amount
        .normalized()
        .to_plain_string()
        .parse::<i128>()
        .map_err(|_| format!("ledger amount {amount} exceeds replay capacity"))?;
    if asset_decimals == ENGINE_DECIMALS {
        return Ok(raw);
    }
    if asset_decimals < ENGINE_DECIMALS {
        let multiplier = 10_i128.pow((ENGINE_DECIMALS - asset_decimals) as u32);
        return raw
            .checked_mul(multiplier)
            .ok_or_else(|| format!("ledger amount {amount} overflows matching-engine units"));
    }

    let divisor = 10_i128.pow((asset_decimals - ENGINE_DECIMALS) as u32);
    if raw % divisor != 0 {
        return Err(format!(
            "ledger amount {amount} is not representable with {ENGINE_DECIMALS} engine decimals"
        ));
    }
    Ok(raw / divisor)
}

#[cfg(test)]
mod tests {
    use super::{scale_engine_amount, scale_ledger_amount};
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

    #[test]
    fn scales_ledger_base_units_to_engine_units_exactly() {
        assert_eq!(
            scale_ledger_amount(&BigDecimal::from(1_000_000_000_000_000_000_i128), 18).unwrap(),
            100_000_000
        );
        assert_eq!(
            scale_ledger_amount(&BigDecimal::from(1_000_000_i64), 6).unwrap(),
            100_000_000
        );
        assert!(scale_ledger_amount(&BigDecimal::from(1_i64), 18).is_err());
    }
}

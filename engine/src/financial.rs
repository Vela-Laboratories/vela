use crate::{EngineMap, UserState};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use types::{AssetId, Fill, Request, Response, Timestamp, UserId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerBalanceDelta {
    pub user: UserId,
    pub asset: AssetId,
    pub available: i128,
    pub locked: i128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformBalanceDelta {
    pub asset: AssetId,
    pub fee_revenue: i128,
    pub rebate_expense: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialEffectSet {
    pub operation_type: String,
    pub customer_balances: Vec<CustomerBalanceDelta>,
    pub platform_balances: Vec<PlatformBalanceDelta>,
    /// Full engine responses bind the durable idempotency key to order and
    /// cancellation identities, even when two batches have equal net deltas.
    pub response_commitments: Vec<Response>,
    pub fills: Vec<Fill>,
    pub batch_size: usize,
}

impl FinancialEffectSet {
    pub fn between(
        before: &UserState,
        after: &UserState,
        responses: &[Response],
        batch_size: usize,
    ) -> Result<Option<Self>, String> {
        let mut keys: HashSet<(UserId, AssetId)> = before.balances.keys().cloned().collect();
        keys.extend(after.balances.keys().cloned());

        let mut customer_balances = Vec::new();
        for (user, asset) in keys {
            let before_balance = before.get_balance(&user, &asset);
            let after_balance = after.get_balance(&user, &asset);
            let available =
                i128::from(after_balance.available) - i128::from(before_balance.available);
            let locked = i128::from(after_balance.locked) - i128::from(before_balance.locked);
            if available != 0 || locked != 0 {
                customer_balances.push(CustomerBalanceDelta {
                    user,
                    asset,
                    available,
                    locked,
                });
            }
        }
        customer_balances.sort_by_key(|delta| (delta.user.0, delta.asset.0));

        let fills: Vec<Fill> = responses
            .iter()
            .filter_map(|response| match response {
                Response::OrderFilled(fill) => Some(fill.clone()),
                _ => None,
            })
            .collect();

        let mut platform_by_asset: EngineMap<AssetId, (i128, i128)> = EngineMap::default();
        for fill in &fills {
            let market = after.markets.get(&fill.market).ok_or_else(|| {
                format!("financial effect references unknown market {}", fill.market)
            })?;
            let entry = platform_by_asset.entry(market.quote).or_insert((0, 0));
            entry.0 += i128::from(fill.taker_fee.max(0));
            entry.0 += i128::from(fill.maker_fee.max(0));
            entry.0 -= i128::from(fill.referral_fee);
            if fill.maker_fee < 0 {
                entry.1 -= i128::from(fill.maker_fee.unsigned_abs());
            }
        }

        let mut fee_assets: HashSet<String> = before.fee_balances.keys().cloned().collect();
        fee_assets.extend(after.fee_balances.keys().cloned());
        for asset_code in fee_assets {
            let asset = AssetId::from_str(&asset_code);
            let before_fee = before.fee_balances.get(&asset_code).copied().unwrap_or(0);
            let after_fee = after.fee_balances.get(&asset_code).copied().unwrap_or(0);
            let actual_delta = i128::from(after_fee) - i128::from(before_fee);
            let expected_delta = platform_by_asset
                .get(&asset)
                .map(|(fees, rebates)| fees + rebates)
                .unwrap_or(0);
            if actual_delta != expected_delta {
                return Err(format!(
                    "platform fee delta mismatch for {asset_code}: expected {expected_delta}, found {actual_delta}"
                ));
            }
        }

        let mut platform_balances: Vec<PlatformBalanceDelta> = platform_by_asset
            .into_iter()
            .filter_map(|(asset, (fee_revenue, rebate_expense))| {
                (fee_revenue != 0 || rebate_expense != 0).then_some(PlatformBalanceDelta {
                    asset,
                    fee_revenue,
                    rebate_expense,
                })
            })
            .collect();
        platform_balances.sort_by_key(|delta| delta.asset.0);

        if customer_balances.is_empty() && platform_balances.is_empty() {
            return Ok(None);
        }

        let mut totals: EngineMap<AssetId, i128> = EngineMap::default();
        for delta in &customer_balances {
            *totals.entry(delta.asset).or_insert(0) += delta.available + delta.locked;
        }
        for delta in &platform_balances {
            *totals.entry(delta.asset).or_insert(0) += delta.fee_revenue + delta.rebate_expense;
        }
        if let Some((asset, total)) = totals.into_iter().find(|(_, total)| *total != 0) {
            return Err(format!(
                "financial effect is unbalanced for {asset}: delta {total}"
            ));
        }

        let operation_type = if !fills.is_empty() {
            "trade"
        } else if responses
            .iter()
            .any(|response| matches!(response, Response::OrderCanceled(_)))
        {
            "order_release"
        } else {
            "order_reservation"
        };

        Ok(Some(Self {
            operation_type: operation_type.to_owned(),
            customer_balances,
            platform_balances,
            response_commitments: responses.to_vec(),
            fills,
            batch_size,
        }))
    }
}

pub trait FinancialCommitter: Send + Sync {
    fn requires_financial_effects(&self) -> bool {
        true
    }

    fn commit<'a>(
        &'a self,
        effects: &'a FinancialEffectSet,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn commit_mutation<'a>(
        &'a self,
        _requests: &'a [(Request, Timestamp)],
        _responses: &'a [Vec<Response>],
        effects: Option<&'a FinancialEffectSet>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(effects) = effects {
                self.commit(effects).await
            } else {
                Ok(())
            }
        })
    }

    /// Latch any implementation-specific safety state when a proposed
    /// mutation cannot even be reduced to a valid financial effect set.
    fn reject_mutation(&self, _error: &str) {}
}

//! Canonical TWAP request shared by HTTP and MCP.
use serde::{Deserialize, Serialize};
use types::OrderSide;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TwapRequest {
    pub address: String,
    pub market: String,
    pub side: OrderSide,
    pub quantity: u64,
    pub duration_secs: u64,
    #[serde(default)]
    pub price_limit: Option<u64>,
    #[serde(default)]
    pub slices: Option<u32>,
    pub nonce: u64,
    pub signature: String,
    #[serde(default)]
    pub signature_version: u8,
}

impl TwapRequest {
    pub fn slice_count(&self) -> u32 {
        self.slices.unwrap_or(12)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=240).contains(&self.slice_count()) {
            return Err("slices must be between 1 and 240");
        }
        if self.quantity < u64::from(self.slice_count()) {
            return Err("quantity must cover every slice");
        }
        if !(1..=86_400).contains(&self.duration_secs) {
            return Err("duration_secs must be between 1 and 86400");
        }
        if !matches!(self.price_limit, Some(1..)) {
            return Err("a positive price_limit is required");
        }
        self.nonce
            .checked_add(u64::from(self.slice_count()))
            .ok_or("child nonce range overflows")?;
        Ok(())
    }
    pub async fn market_tick(&self, state: &crate::AppState) -> Result<u64, &'static str> {
        let engine = state.engine.lock().await;
        let market = engine
            .markets
            .get(&types::MarketId(self.market.clone()))
            .ok_or("unknown market")?;
        let tick = market.quantity_tick.max(1);
        let price_tick = market.price_tick.max(1);
        if !self.quantity.is_multiple_of(tick) {
            return Err("quantity must align to market lot size");
        }
        if !self.price_limit.unwrap_or(0).is_multiple_of(price_tick) {
            return Err("price_limit must align to market tick size");
        }
        let smallest_slice = self.quantity / tick / u64::from(self.slice_count()) * tick;
        if smallest_slice < market.min_order_size.max(tick) {
            return Err("every slice must meet minimum order size");
        }
        Ok(tick)
    }
    pub fn signing_message(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            "vela:algo:twap:v2",
            self.address.to_ascii_lowercase(),
            &self.market,
            self.side,
            self.quantity,
            self.duration_secs,
            self.price_limit,
            self.slice_count(),
            self.nonce,
        ))
        .expect("TWAP fields are serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signature_covers_execution_fields() {
        let req = TwapRequest {
            address: "0xABC".into(),
            market: "ETH-USDC".into(),
            side: OrderSide::Bid,
            quantity: 12,
            duration_secs: 60,
            price_limit: Some(100),
            slices: None,
            nonce: 1,
            signature: String::new(),
            signature_version: 2,
        };
        let original = req.signing_message();
        let mut changed = req.clone();
        changed.side = OrderSide::Ask;
        assert_ne!(original, changed.signing_message());
        changed = req.clone();
        changed.price_limit = Some(101);
        assert_ne!(original, changed.signing_message());
        changed = req.clone();
        changed.slices = Some(13);
        assert_ne!(original, changed.signing_message());
        changed = req.clone();
        changed.address = "0xabc".into();
        assert_eq!(original, changed.signing_message());
        assert_eq!(
            String::from_utf8(original).unwrap(),
            r#"["vela:algo:twap:v2","0xabc","ETH-USDC","Bid",12,60,100,12,1]"#
        );
    }
}

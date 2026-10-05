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

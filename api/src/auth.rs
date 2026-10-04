use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use rand::RngCore;
use sha3::{Digest, Keccak256};
use types::{OrderSide, OrderType, SelfTradePreventionMode, UserId, VelaError};

pub fn eth_message_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut h = Keccak256::new();
    h.update(prefix.as_bytes());
    h.update(message);
    h.finalize().into()
}

pub fn recover_signer(message: &[u8], signature_hex: &str) -> Result<UserId, VelaError> {
    let sig_bytes = hex::decode(signature_hex.strip_prefix("0x").unwrap_or(signature_hex))
        .map_err(|_| VelaError::InvalidSignature)?;
    if sig_bytes.len() != 65 {
        return Err(VelaError::InvalidSignature);
    }
    let hash = eth_message_hash(message);
    let v = sig_bytes[64];
    let recovery_id = RecoveryId::try_from(v % 27).map_err(|_| VelaError::InvalidSignature)?;
    let sig = Signature::try_from(&sig_bytes[..64]).map_err(|_| VelaError::InvalidSignature)?;
    let vk = VerifyingKey::recover_from_prehash(&hash, &sig, recovery_id)
        .map_err(|_| VelaError::InvalidSignature)?;
    let pubkey = vk.to_encoded_point(false);
    let pubkey_hash = Keccak256::digest(&pubkey.as_bytes()[1..]);
    let addr: [u8; 20] = pubkey_hash[12..]
        .try_into()
        .map_err(|_| VelaError::InvalidSignature)?;
    Ok(UserId(addr))
}

pub fn verify_matches(
    message: &[u8],
    signature_hex: &str,
    expected_hex: &str,
) -> Result<UserId, VelaError> {
    let expected = UserId::from_hex(expected_hex).map_err(|_| VelaError::InvalidSignature)?;
    let recovered = recover_signer(message, signature_hex)?;
    if recovered != expected {
        return Err(VelaError::InvalidSignature);
    }
    Ok(recovered)
}

pub fn order_signing_message(
    market: &str,
    side: &str,
    price: u64,
    quantity: u64,
    nonce: u64,
    client_order_id: Option<&str>,
) -> Vec<u8> {
    match client_order_id {
        None => format!(
            "vela:order:{}:{}:{}:{}:{}",
            market, side, price, quantity, nonce
        )
        .into_bytes(),
        Some(coid) => format!(
            "vela:order:{}:{}:{}:{}:{}:{}",
            market, side, price, quantity, nonce, coid
        )
        .into_bytes(),
    }
}

/// Canonical signed payload for order controls. The JSON array keeps each
/// field delimited and matches `JSON.stringify` in browser clients. V2 is
/// opt-in so existing plain orders retain their original signature format.
pub fn order_signing_message_v2(
    market: &str,
    side: OrderSide,
    order_type: OrderType,
    price: u64,
    quantity: u64,
    nonce: u64,
    client_order_id: Option<&str>,
    stp: SelfTradePreventionMode,
    min_quantity: Option<u64>,
    display_quantity: Option<u64>,
) -> Vec<u8> {
    serde_json::to_vec(&(
        "vela:order:v2",
        market,
        side,
        order_type,
        price,
        quantity,
        nonce,
        client_order_id,
        stp,
        min_quantity,
        display_quantity,
    ))
    .expect("order signing payload is serializable")
}

pub fn cancel_signing_message(
    order_id: Option<u64>,
    client_order_id: Option<&str>,
    nonce: u64,
) -> Vec<u8> {
    format!(
        "vela:cancel:{}:{}:{}",
        order_id.map(|i| i.to_string()).unwrap_or_default(),
        client_order_id.unwrap_or(""),
        nonce
    )
    .into_bytes()
}

pub fn withdrawal_signing_message(asset: &str, amount: u64, nonce: u64) -> Vec<u8> {
    format!("vela:withdraw:{}:{}:{}", asset, amount, nonce).into_bytes()
}

pub fn withdrawal_reservation_signing_message(
    withdrawal_id: &str,
    customer: &str,
    destination: &str,
    asset: &str,
    amount: &str,
    nonce: u64,
) -> Vec<u8> {
    format!(
        "vela:withdrawal-reservation:{}:{}:{}:{}:{}:{}",
        withdrawal_id,
        customer.to_ascii_lowercase(),
        destination.to_ascii_lowercase(),
        asset.to_ascii_uppercase(),
        amount,
        nonce
    )
    .into_bytes()
}

pub fn withdrawal_cancel_signing_message(withdrawal_id: &str, nonce: u64) -> Vec<u8> {
    format!("vela:withdrawal-cancel:{withdrawal_id}:{nonce}").into_bytes()
}

pub fn auth_signing_message(nonce: &str) -> Vec<u8> {
    format!("vela:auth:{}", nonce).into_bytes()
}

pub fn ws_auth_signing_message(address: &str, timestamp: u64) -> Vec<u8> {
    format!("vela:ws:{}:{}", address, timestamp).into_bytes()
}

/// Generates a server-side challenge nonce — 16 random bytes as a lowercase hex string.
/// Must be server-issued and single-use to prevent replay attacks.
pub fn generate_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub async fn verify_matches_async(
    message: Vec<u8>,
    signature: String,
    expected: String,
) -> Result<UserId, VelaError> {
    tokio::task::spawn_blocking(move || verify_matches(&message, &signature, &expected))
        .await
        .map_err(|_| VelaError::InvalidSignature)?
}

pub async fn verify_batch_async(
    items: Vec<(Vec<u8>, String, String)>,
) -> Vec<Result<UserId, VelaError>> {
    let handles: Vec<_> = items
        .into_iter()
        .map(|(msg, sig, addr)| {
            tokio::task::spawn_blocking(move || verify_matches(&msg, &sig, &addr))
        })
        .collect();
    let mut results = vec![];
    for h in handles {
        results.push(h.await.unwrap_or(Err(VelaError::InvalidSignature)));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_order_signature_binds_every_execution_control() {
        let message = |order_type, stp, min_quantity, display_quantity| {
            order_signing_message_v2(
                "BTC-USDC",
                OrderSide::Bid,
                order_type,
                100_000_000,
                5_000_000,
                42,
                Some("client:1"),
                stp,
                min_quantity,
                display_quantity,
            )
        };
        let base = message(
            OrderType::GoodTillCanceled,
            SelfTradePreventionMode::None,
            None,
            None,
        );
        assert_eq!(
            String::from_utf8(base.clone()).unwrap(),
            "[\"vela:order:v2\",\"BTC-USDC\",\"Bid\",\"GoodTillCanceled\",100000000,5000000,42,\"client:1\",\"None\",null,null]"
        );
        assert_ne!(
            base,
            message(
                OrderType::PostOnly,
                SelfTradePreventionMode::None,
                None,
                None
            )
        );
        assert_ne!(
            base,
            message(
                OrderType::GoodTillCanceled,
                SelfTradePreventionMode::CancelMaker,
                None,
                None
            )
        );
        assert_ne!(
            base,
            message(
                OrderType::GoodTillCanceled,
                SelfTradePreventionMode::None,
                Some(1_000_000),
                None
            )
        );
        assert_ne!(
            base,
            message(
                OrderType::GoodTillCanceled,
                SelfTradePreventionMode::None,
                None,
                Some(1_000_000)
            )
        );
    }
}

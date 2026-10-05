//! On-demand signing of an already approved authorization for an isolated testnet.
use api::custody_signer::{CustodySigner, CustodySigningService, TestnetLocalCustodySigner};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{env, sync::Arc, time::Duration};
use uuid::Uuid;

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{name} must be set").into())
}
async fn rpc(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let response = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .map_err(|_| "custody RPC request failed")?;
    let body: Value = response
        .error_for_status()
        .map_err(|_| "custody RPC HTTP failure")?
        .json()
        .await
        .map_err(|_| "custody RPC returned malformed JSON")?;
    if body.get("error").is_some() {
        return Err("custody RPC rejected the request".into());
    }
    body.get("result")
        .cloned()
        .ok_or_else(|| "custody RPC result is absent".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = types::RuntimeMode::from_env()?;
    let chain_id: i64 = required("VELA_CHAIN_ID")?.parse()?;
    let vault = required("VELA_CUSTODY_VAULT_ADDRESS")?;
    let secret = zeroize::Zeroizing::new(required("VELA_CUSTODY_SIGNER_KEY_HEX")?);
    let signer = Arc::new(TestnetLocalCustodySigner::new(
        mode,
        chain_id,
        vault.clone(),
        required("VELA_CUSTODY_SIGNER_KEY_ID")?,
        &secret,
    )?);
    let withdrawal_id: Uuid = required("VELA_WITHDRAWAL_ID")?.parse()?;
    let authorization_id: Uuid = required("VELA_AUTHORIZATION_ID")?.parse()?;
    let ledger = Arc::new(ledger::Ledger::connect(&required("DATABASE_URL")?, 1).await?);
    ledger.ensure_schema_current().await?;
    // Check immutable provenance before transitioning an authorization to Signing.
    let (stored_chain, stored_vault, code_hash): (i64,String,String) = sqlx::query_as(
        "SELECT a.chain_id,a.vault_address,d.runtime_code_hash FROM ledger_withdrawal_authorizations a JOIN ledger_chain_deployments d ON d.chain_id=a.chain_id AND lower(d.vault_address)=lower(a.vault_address) JOIN ledger_withdrawal_contract_capabilities c ON c.deployment_id=d.id WHERE a.id=$1 AND a.withdrawal_id=$2 AND d.enabled AND c.authorization_version='eip712_v3'"
    ).bind(authorization_id).bind(withdrawal_id).fetch_one(ledger.pool()).await?;
    if stored_chain != chain_id || !stored_vault.eq_ignore_ascii_case(&vault) {
        return Err("authorization deployment differs from signer configuration".into());
    }
    let url = required("VELA_CUSTODY_RPC_URL")?;
    if !reqwest::Url::parse(&url).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some()) {
        return Err("custody RPC must use HTTPS".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let rpc_chain = rpc(&client, &url, "eth_chainId", json!([])).await?;
    let observed_chain = u64::from_str_radix(
        rpc_chain
            .as_str()
            .ok_or("invalid chain ID")?
            .strip_prefix("0x")
            .ok_or("invalid chain ID")?,
        16,
    )?;
    if observed_chain != chain_id as u64 {
        return Err("custody RPC chain differs from authorization".into());
    }
    let code = rpc(&client, &url, "eth_getCode", json!([vault, "latest"])).await?;
    let bytes = hex::decode(
        code.as_str()
            .ok_or("invalid bytecode")?
            .strip_prefix("0x")
            .ok_or("invalid bytecode")?,
    )?;
    if bytes.is_empty()
        || !format!("0x{}", hex::encode(Keccak256::digest(&bytes))).eq_ignore_ascii_case(&code_hash)
    {
        return Err("vault bytecode differs from immutable deployment registry".into());
    }
    let selector = format!("0x{}", hex::encode(&Keccak256::digest(b"operator()")[..4]));
    let operator = rpc(
        &client,
        &url,
        "eth_call",
        json!([{"to":vault,"data":selector},"latest"]),
    )
    .await?;
    let operator = hex::decode(
        operator
            .as_str()
            .ok_or("invalid operator result")?
            .strip_prefix("0x")
            .ok_or("invalid operator result")?,
    )?;
    let identity = signer.identity()?;
    let public_key = k256::ecdsa::VerifyingKey::from_sec1_bytes(&identity.signer_public_key)?
        .to_encoded_point(false);
    let address = Keccak256::digest(&public_key.as_bytes()[1..]);
    if operator.len() != 32
        || operator[..12].iter().any(|v| *v != 0)
        || operator[12..] != address[12..]
    {
        return Err("signer does not match the vault operator".into());
    }
    let record = CustodySigningService::with_signer(ledger, signer)
        .sign_authorization(withdrawal_id, authorization_id)
        .await?;
    println!(
        "stored testnet signature {} for withdrawal {}",
        record.id, withdrawal_id
    );
    Ok(())
}

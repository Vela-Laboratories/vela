use ledger::{AccountProvisionRequest, Ledger, NewAsset};
use std::time::Duration;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_MIGRATOR_URL")
        .map_err(|_| "DATABASE_MIGRATOR_URL must be set for ledger bootstrap")?;
    let chain_id: i64 = std::env::var("VELA_CHAIN_ID")
        .map_err(|_| "VELA_CHAIN_ID must be set for ledger bootstrap")?
        .parse()
        .map_err(|_| "VELA_CHAIN_ID must be a positive integer")?;
    if chain_id <= 0 {
        return Err("VELA_CHAIN_ID must be a positive integer".into());
    }
    let usdc_address = std::env::var("VELA_USDC_ADDRESS")
        .map_err(|_| "VELA_USDC_ADDRESS must be set for ledger bootstrap")?;
    if !is_evm_address(&usdc_address) {
        return Err("VELA_USDC_ADDRESS must be a 20-byte 0x-prefixed hex address".into());
    }

    let ledger = Ledger::connect_with_options(
        &database_url,
        1,
        Duration::from_secs(10),
        Duration::from_secs(60),
        0,
    )
    .await?;
    ledger.ensure_schema_current().await?;

    for asset in [
        NewAsset {
            id: Uuid::new_v4(),
            code: "ETH".to_owned(),
            decimals: 18,
            chain_id: Some(chain_id),
            contract_address: None,
        },
        NewAsset {
            id: Uuid::new_v4(),
            code: "USDC".to_owned(),
            decimals: 6,
            chain_id: Some(chain_id),
            contract_address: Some(usdc_address.to_ascii_lowercase()),
        },
    ] {
        ledger.ensure_asset(&asset).await?;
    }

    let platform_accounts: Vec<AccountProvisionRequest> = ["ETH", "USDC"]
        .into_iter()
        .flat_map(|asset_code| {
            ["fee_revenue", "rebate_expense"].map(move |account_type| AccountProvisionRequest {
                asset_code: asset_code.to_owned(),
                owner_type: "platform".to_owned(),
                owner_reference: "platform:vela".to_owned(),
                account_type: account_type.to_owned(),
                allow_negative: true,
            })
        })
        .collect();
    ledger.ensure_accounts(&platform_accounts).await?;

    println!("ledger bootstrap complete for ETH/USDC on chain {chain_id}");
    Ok(())
}

fn is_evm_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::is_evm_address;

    #[test]
    fn validates_evm_address_shape() {
        assert!(is_evm_address("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"));
        assert!(!is_evm_address("0x1234"));
        assert!(!is_evm_address("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"));
    }
}

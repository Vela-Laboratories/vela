#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${ANVIL_RPC_URL:-}" || -z "${DATABASE_URL:-}" ]]; then
  printf 'run through scripts/with_local_custody_deps.sh\n' >&2
  exit 2
fi

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
accounts="$(cast rpc eth_accounts --rpc-url "$ANVIL_RPC_URL")"
deployer="$(jq -r '.[0]' <<<"$accounts")"
operator="$(jq -r '.[1]' <<<"$accounts")"
if [[ "$deployer" == null || "$operator" == null ]]; then
  printf 'Anvil test accounts unavailable\n' >&2
  exit 1
fi

deployment="$(cd "$repo_root/contracts" && forge create \
  src/VelaSettlement.sol:VelaSettlement \
  --rpc-url "$ANVIL_RPC_URL" --broadcast --unlocked --from "$deployer" \
  --constructor-args "$operator" "0x$(printf '11%.0s' {1..32})")"
export VELA_LOCAL_VAULT="$(sed -n 's/^Deployed to: //p' <<<"$deployment")"
deployment_tx="$(sed -n 's/^Transaction hash: //p' <<<"$deployment")"
if [[ -z "$VELA_LOCAL_VAULT" || -z "$deployment_tx" ]]; then
  printf 'local V3 deployment did not return an address and transaction\n' >&2
  exit 1
fi
export VELA_LOCAL_DEPLOYMENT_BLOCK="$(cast receipt "$deployment_tx" blockNumber --rpc-url "$ANVIL_RPC_URL")"
export VELA_LOCAL_ACCOUNT_ID="0x$(printf '33%.0s' {1..32})"

token_deployment="$(cd "$repo_root/contracts" && forge create \
  test/MockUSDC.sol:MockUSDC --rpc-url "$ANVIL_RPC_URL" \
  --broadcast --unlocked --from "$deployer")"
export VELA_LOCAL_USDC="$(sed -n 's/^Deployed to: //p' <<<"$token_deployment")"
if [[ -z "$VELA_LOCAL_USDC" ]]; then
  printf 'local six-decimal USDC fixture did not deploy\n' >&2
  exit 1
fi

cast send "$VELA_LOCAL_VAULT" 'setSupportedAsset(address,bool)' \
  "$VELA_LOCAL_USDC" true --rpc-url "$ANVIL_RPC_URL" \
  --unlocked --from "$deployer" --json >/dev/null
cast send "$VELA_LOCAL_USDC" 'approve(address,uint256)' \
  "$VELA_LOCAL_VAULT" 1500000 --rpc-url "$ANVIL_RPC_URL" \
  --unlocked --from "$deployer" --json >/dev/null

cast send "$VELA_LOCAL_VAULT" 'depositETH(bytes32)' "$VELA_LOCAL_ACCOUNT_ID" \
  --value 1ether --rpc-url "$ANVIL_RPC_URL" --unlocked --from "$operator" \
  --json >/dev/null
cast send "$VELA_LOCAL_VAULT" 'depositToken(bytes32,address,uint256)' \
  "$VELA_LOCAL_ACCOUNT_ID" "$VELA_LOCAL_USDC" 1500000 \
  --rpc-url "$ANVIL_RPC_URL" --unlocked --from "$deployer" --json >/dev/null

# Anvil's finalized tag trails the head. Mine past that window so both the
# vault deployment and the deposit are finalized before the indexer runs.
cast rpc anvil_mine 100 --rpc-url "$ANVIL_RPC_URL" >/dev/null
cargo test -p indexer --test live_anvil -- --nocapture

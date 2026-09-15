#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root/contracts"

source_commit=$(git rev-parse HEAD)
runtime_bytecode=$(forge inspect VelaSettlement deployedBytecode)
runtime_template_hash=$(cast keccak "$runtime_bytecode")
creation_bytecode=$(forge inspect VelaSettlement bytecode)
creation_bytecode_hash=$(cast keccak "$creation_bytecode")
compiler_version=$(forge config --json | jq -r '.solc')

jq -n \
  --arg contract "VelaSettlement" \
  --arg chain_id "${VELA_EXPECTED_CHAIN_ID:-unselected}" \
  --arg source_commit "$source_commit" \
  --arg compiler "$compiler_version" \
  --arg evm_version "cancun" \
  --arg optimizer_runs "200" \
  --arg creation_bytecode_hash "$creation_bytecode_hash" \
  --arg runtime_template_hash "$runtime_template_hash" \
  '{contract:$contract,chain_id:$chain_id,source_commit:$source_commit,compiler:$compiler,evm_version:$evm_version,optimizer_runs:$optimizer_runs,creation_bytecode_hash:$creation_bytecode_hash,runtime_template_hash:$runtime_template_hash}'

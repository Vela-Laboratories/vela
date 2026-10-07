# Base deposit indexer runbook

## Safety boundary

The `deposit-indexer` process is the only intended chain-to-ledger deposit writer. It supports Base mainnet (8453) and Base Sepolia (84532), but this repository remains testnet-only until every launch gate is complete. Do not point it at a mainnet vault or accept real funds.

The process credits only logs that satisfy all of these conditions:

- the PostgreSQL registry entry is enabled and matches the supported deposit ABI;
- both independently operated RPC endpoints report the configured chain ID;
- both latest heads are recent and within the configured block-lag limit;
- both providers agree on the hash at their highest common finalized height;
- both providers return runtime bytecode whose keccak256 hash matches the immutable registry entry;
- both providers return the exact same normalized logs for the scanned finalized range;
- each log has the pinned vault address, event topic, indexed deposit ID, beneficiary, asset, and exact uint256 amount;
- the asset and custody counter-account already exist in the authoritative ledger;
- the ledger accepts the idempotent observation, finality evidence, and atomic credit.

Any uncertainty stops credits. Safety failures are written to `ledger_indexer_halts`, and the monotonic PostgreSQL checkpoint advances only after the complete range is processed. A database lease prevents two workers from owning one deployment concurrently. A crash after a credit but before checkpoint advancement is safe because deposit and journal identities are deterministic and idempotent.

The same worker continuously cycles a second checkpoint from the registered deployment block through the already-processed finalized range. Both providers must continue returning identical canonical logs. A missing or changed credited event activates an asset-wide halt before the ledger moves every still-available unit into `customer_pending`; a complete isolation becomes `reversed`, while any shortfall becomes `quarantined`. A newly appeared historical event also halts the affected asset because it is behind the monotonic forward checkpoint.

Customer-facing status is served by `GET /account/{address}/deposits`. The route reads the ledger directly, can be filtered by the real transaction hash, and returns `503` rather than falling back to demo state when the ledger is unavailable. The frontend requires a successful status preflight before opening the wallet, then polls the route after the wallet confirms a Base transaction; transaction submission itself never creates a customer credit.

## Contract and registry requirement

The V3 source emits:

```text
Deposited(bytes32 indexed depositId, address indexed depositor, bytes32 indexed accountId, address asset, uint256 amount)
topic0: derive from the reviewed V3 ABI and deployment manifest; do not copy the legacy topic.
```

The older Ethereum Sepolia deployment documented in the project predates this event and is not compatible with the indexer. Registration accepts only Base Sepolia or Base network identifiers with deployment version `v3` and a nonzero UUID. Deploy and audit the updated contract on Base Sepolia before registering it. Never reuse the old address or invent a deployment block or bytecode hash.

After deployment, obtain the exact deployment block and hash the runtime bytecode returned by each provider. Keep RPC URLs out of shell history when practical.

```sh
cast code "$VELA_CUSTODY_VAULT_ADDRESS" --rpc-url "$VELA_BASE_RPC_PRIMARY_URL" | cast keccak
cast code "$VELA_CUSTODY_VAULT_ADDRESS" --rpc-url "$VELA_BASE_RPC_SECONDARY_URL" | cast keccak
```

The two hashes must match. Record the result as `VELA_CUSTODY_VAULT_CODE_HASH` in the protected staging environment.

## Controlled registration

Run all current migrations, reapply `ledger/sql/roles.sql`, bootstrap ETH/USDC accounts, publish reviewed withdrawal-policy limits, and register the deployment with the migrator credential. The GitHub `Ledger Migration` workflow performs the schema and role steps in that order; policy publication remains an explicit operator action.

Registration now reads one reviewed, nonsecret JSON manifest from
`VELA_CUSTODY_DEPLOYMENT_MANIFEST_PATH`. It requires schema version 1, a
nonzero deployment UUID, Base Sepolia or Base network identity, V3, an exact
deposit event ABI and topic, a positive deployment block, and nonzero source,
creation-bytecode, runtime-template, and deployed runtime-code hashes. The
manifest's `source_commit` is the 32-byte constructor identity; the
`creation_bytecode_hash` and `runtime_template_hash` come from
`contracts/scripts/reproducible_build.sh` at the reviewed source commit.
Compare the final `runtime_code_hash` with both RPC providers before approval.
The registration command also checks two distinct RPC origins and provider
names, matching chain IDs, the same common finalized block, and the pinned
runtime code hash before it opens the migrator database connection. The
manifest alone is not on-chain verification.

For a local Base Sepolia rehearsal:

```sh
DATABASE_MIGRATOR_URL=postgres://... \
VELA_CUSTODY_DEPLOYMENT_MANIFEST_PATH=/secure/ops/v3-base-sepolia.json \
VELA_BASE_RPC_PRIMARY_NAME=provider-a \
VELA_BASE_RPC_PRIMARY_URL=https://... \
VELA_BASE_RPC_SECONDARY_NAME=provider-b \
VELA_BASE_RPC_SECONDARY_URL=https://... \
cargo run --locked --release -p indexer --bin deposit-indexer-register
```

Generate the UUID once and retain it in protected environment configuration. Registration is idempotent only when every immutable value, including the UUID, matches.

## Run

Use the restricted writer credential, never the migrator credential:

```sh
DATABASE_URL=postgres://... \
VELA_INDEXER_DEPLOYMENT_ID=11111111-1111-4111-8111-111111111111 \
VELA_BASE_RPC_PRIMARY_NAME=provider-a \
VELA_BASE_RPC_PRIMARY_URL=https://... \
VELA_BASE_RPC_SECONDARY_NAME=provider-b \
VELA_BASE_RPC_SECONDARY_URL=https://... \
cargo run --locked --release -p indexer --bin deposit-indexer
```

The provider names are nonsecret evidence labels. The URLs are secrets. The process rejects identical names and identical URL origins, redacts URLs from its logs, uses bounded request timeouts, backs off after failure, and releases its lease on graceful shutdown.

## Halt response

When credits stop:

1. Leave the checkpoint and halt evidence intact.
2. Compare chain ID, latest height/time, common finalized hash, runtime code hash, and log results directly with both providers.
3. Treat bytecode mismatch, finalized regression, or conflicting logs as an incident. Do not resolve it by changing a registry row.
4. Disable a compromised deployment by changing only `enabled` from true to false with a reviewed migrator operation.
5. Register a new immutable deployment row for replacement bytecode or address.
6. Resolve a halt record only after the evidence is retained and an incident owner is named.

For `credited_deposit_contradiction` or `historical_deposit_appeared`, also inspect `ledger_asset_halts`, the deposit state history, the reversal journal transaction, customer pending balance, and recorded shortfall. Do not re-enable the asset merely because providers later converge. Resolution requires a fresh successful reconciliation plus distinct initiator and approver identities; the runtime writer has no resolution permission.

Provider recovery permits an idempotent retry from the unchanged checkpoint. Automatic reversal contains customer spendability but does not prove custody solvency, notify customers, page an operator, or replace the required reconciliation and incident process.

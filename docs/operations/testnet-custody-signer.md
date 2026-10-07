# Opt-in Base Sepolia custody signer

The `testnet-custody-sign` binary signs one previously approved V3 withdrawal
authorization on demand. It is not started by the API or by the withdrawal
observer. Its local key adapter accepts only `VELA_RUNTIME_MODE=testnet-demo`
and Base Sepolia chain ID `84532`; production and legacy Ethereum Sepolia are
rejected. Use only an isolated testnet key with no real-funds authority.

The process requires `DATABASE_URL`, `VELA_CHAIN_ID`,
`VELA_CUSTODY_VAULT_ADDRESS`, `VELA_CUSTODY_SIGNER_KEY_ID`,
`VELA_CUSTODY_SIGNER_KEY_HEX`, `VELA_WITHDRAWAL_ID`,
`VELA_AUTHORIZATION_ID`, and `VELA_CUSTODY_RPC_URL`. Keep the key in a secret
provider or ephemeral process environment, never in a command argument, shell
history, log, or repository file. The RPC URL must use HTTPS.

Before the ledger advances the authorization to signing, the binary checks the
stored authorization's chain and vault against its configuration, requires an
enabled V3 deployment capability, confirms RPC chain ID and pinned runtime
bytecode, and compares the key's derived address to `operator()` on the vault.
The signing service then checks authorization expiry and immutable digest,
recovers the produced secp256k1 signature against the configured public key,
and stores the signature under the ledger's withdrawal state machine.
The chain, vault, bytecode, and operator comparison is shared with a focused
unit test covering mismatches. The database also requires an enabled V3
capability before the binary reaches this comparison.

This binary does not choose a relayer, submit a transaction, watch finality, or
release a reservation. D-014 still requires an end-to-end preflight rehearsal
against a registered V3 Base Sepolia deployment before signing is treated as
operationally qualified.

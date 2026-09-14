# Authoritative withdrawal reservation boundary

## Scope

EX-023 establishes the first safe withdrawal boundary for the private testnet build. It does not claim that automated custody signing, broadcast, confirmation monitoring, or reconciliation is complete.

`POST /withdrawals` accepts an EIP-191 wallet signature over the request ID, customer, destination, asset, exact eight-decimal request amount, and nonce. ETH and USDC use the same endpoint and ledger command. The API converts the request amount to the configured ledger asset's base units without floating point.

When PostgreSQL is authoritative, the API never dispatches the withdrawal to the matching engine. `Ledger::reserve_withdrawal` instead performs one serializable transaction that:

- verifies the configured asset, customer available account, and single custody-vault account;
- inserts the immutable withdrawal identity and initial history;
- journals equal and opposite legs from `customer_available` to `customer_withdrawal_reserved`;
- updates the withdrawal from `requested` to `reserved` with its journal reference;
- writes the second immutable history entry and transactional outbox event;
- commits before the API acknowledges success.

The customer nonce and request ID are unique. An identical retry returns the original record and journal transaction. Reusing either identity for different request fields fails without moving funds. Serializable balance locks prevent concurrent withdrawals from exceeding available ownership.

## Cancellation and signing boundary

`POST /withdrawals/{id}/cancel` requires a fresh customer signature. Only `reserved` requests can be cancelled. Cancellation journals the exact reverse movement and records `cancelled` in the same transaction; retries return the original release rather than crediting twice.

The legacy `/withdrawal-signature` route now fails closed whenever the authoritative ledger is configured. This prevents the environment-held demo signer from releasing custody assets without consuming a durable reservation. A future signer adapter must bind the withdrawal request ID, chain, vault, customer, destination, asset, amount, nonce, and expiry before the `approved`, `signing`, and `signed` transitions can be enabled.

## Customer status

`GET /account/{address}/withdrawals` reads only PostgreSQL and returns `503` when the authoritative ledger is absent. The frontend preflights this route before requesting a wallet signature, polls it for exact status, explains whether funds are reserved or require intervention, and allows a safe pre-signing cancellation. It makes no claim that a reservation is an on-chain transfer.

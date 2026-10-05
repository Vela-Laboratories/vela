# Vela TypeScript client

`src/schema.ts` is generated from `docs/api-reference/openapi.json` with
`openapi-typescript`. Regenerate it after changing the v1 API contract:

```sh
./frontend/node_modules/.bin/openapi-typescript docs/api-reference/openapi.json -o sdk/typescript/src/schema.ts
./frontend/node_modules/.bin/tsc -p sdk/typescript/tsconfig.json
```

```ts
import { VelaClient } from './src/index'

const client = new VelaClient()
const markets = await client.markets()
```

Customer account reads need a short-lived bearer session. Signed mutations
need EIP-191 signatures created by the master wallet; the client takes the
signature as input and never handles a private key.

## Execution algorithms

TWAP is available in development/testnet mode without an authoritative ledger.
Sign the full v2 message with an external wallet before calling `placeTwap`:

```ts
import { VelaClient, twapSigningMessage, algoCancelSigningMessage } from './src/index'

const client = new VelaClient({ bearerToken: sessionToken, timeoutMs: 10_000 })
const input = {
  address: masterAddress,
  market: 'ETH-USDC',
  side: 'Bid' as const,
  quantity: 1_000_000,
  duration_secs: 60,
  price_limit: 3_000_000_000,
  slices: 12,
  nonce: 100,
}
const signature = await wallet.signMessage({ message: twapSigningMessage(input) })
const parent = await client.placeTwap(input, signature)
const progress = await client.algo(parent.parent_id)
const parents = await client.algos(masterAddress)
const cancelSignature = await wallet.signMessage({ message: algoCancelSigningMessage(parent.parent_id, 200) })
await client.cancelAlgo({ parent_id: parent.parent_id, address: masterAddress, nonce: 200, signature: cancelSignature })
```

`wallet`, `masterAddress` and `sessionToken` are supplied by your application.
The SDK never stores a private key. Reads use the master wallet's bearer session;
signed submissions can use the master or a scoped delegated signer.

`orders(address)` and `fills(address)` read private history with exact decimal
strings. Fill reads currently return the most recent 500 records.

`VelaApiError` exposes `status` and integer `retryAfterSeconds` where provided.
Requests time out after 10 seconds by default. The client never retries writes.
A timeout can leave an indeterminate write outcome: inspect parent/account state
before deciding whether to submit another parent. Numbers outside JavaScript's
safe-integer range are rejected by signing helpers.

## Tests

```sh
node --experimental-strip-types --test sdk/typescript/tests/client.test.ts
```

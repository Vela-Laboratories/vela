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

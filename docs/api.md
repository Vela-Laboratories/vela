# Vela API Reference

All prices and quantities are **raw `u64` integers** scaled by `10^8`. For example, a price of `50000.00` USDC is represented as `5000000000000` (`50000 * 10^8`). The API returns these as decimal strings; the signing messages use the raw integer form.

Responses always have the shape:

```json
{ "ok": true, "data": <T>, "error": null }
{ "ok": false, "data": null, "error": "<message>" }
```

---

## HTTP Endpoints

Customer-data routes require `Authorization: Bearer <token>`. Request a
single-use challenge from `POST /auth/challenge`, sign the exact returned
message, and exchange it at `POST /auth/session`. Challenges expire after five
minutes and sessions after 30 minutes. `POST /auth/session/revoke` invalidates
the current token.

### `GET /health`

Liveness probe.

**Response:** `200 OK`, body `"ok"` (plain text, not JSON).

---

### `GET /markets`

List all registered markets with current best bid/ask.

**Response `data`:** array of market objects

```json
[
  {
    "id":       "BTC-USDC",
    "base":     "BTC",
    "quote":    "USDC",
    "best_bid": "49999.50",
    "best_ask": "50000.25",
    "spread":   "0.75"
  }
]
```

`best_bid`, `best_ask`, and `spread` are omitted (null) if the book is empty on either side.

---

### `GET /markets/:market/book`

Order book depth, up to 50 levels per side.

**Path parameter:** `market` — market ID string, e.g., `BTC-USDC`

**Response `data`:**

```json
{
  "market": "BTC-USDC",
  "bids": [
    { "price": "49999.50", "quantity": "0.5" },
    { "price": "49998.00", "quantity": "1.2" }
  ],
  "asks": [
    { "price": "50000.25", "quantity": "0.3" },
    { "price": "50001.00", "quantity": "2.0" }
  ]
}
```

Returns `404` with `error: "market not found"` if the market ID is not registered.

---

### `GET /account/:address/balances`

All balances for the authenticated address.

**Path parameter:** `address` — Ethereum address, with or without `0x` prefix

**Response `data`:** array of balance objects

```json
[
  {
    "asset":     "USDC",
    "available": "9850.00",
    "locked":    "150.00",
    "total":     "10000.00"
  },
  {
    "asset":     "BTC",
    "available": "1.5",
    "locked":    "0",
    "total":     "1.5"
  }
]
```

Returns `400` if the address is not a valid 20-byte Ethereum address.
Returns `401` for a missing or invalid session and `403` when the session is
bound to another address.

---

### `GET /account/:address/orders`

All open orders for a given address, across all markets.

**Path parameter:** `address` — Ethereum address

**Response `data`:** array of order objects

```json
[
  {
    "id":              12345,
    "market":          "BTC-USDC",
    "side":            "bid",
    "order_type":      "goodtillcanceled",
    "price":           "49999.50",
    "quantity":        "0.5",
    "filled_quantity": "0",
    "status":          "open",
    "nonce":           42,
    "client_order_id": null,
    "timestamp":       1712345678000000
  }
]
```

`timestamp` is Unix microseconds.

---

### `POST /orders`

Submit a new order. Requires ECDSA signature.

**Request body:**

```json
{
  "market":          "BTC-USDC",
  "side":            "bid",
  "order_type":      "goodtillcanceled",
  "price":           4999950000000,
  "quantity":        50000000,
  "nonce":           42,
  "client_order_id": null,
  "address":         "0xAbCd1234...",
  "signature":       "0x<65-byte-hex>"
}
```

**Fields:**

| Field | Type | Description |
|---|---|---|
| `market` | string | Market ID, e.g. `"BTC-USDC"` |
| `side` | `"bid"` \| `"ask"` | Order direction |
| `order_type` | `"goodtillcanceled"` \| `"postonly"` \| `"immediateorcancel"` \| `"fillorkill"` | Order type |
| `price` | u64 | Limit price in raw units (price × 10^8) |
| `quantity` | u64 | Order quantity in raw units (qty × 10^8) |
| `nonce` | u64 | Strictly increasing per-user nonce (window size 20) |
| `client_order_id` | string \| null | Optional client-assigned ID (must be unique per user) |
| `address` | string | Ethereum address of the signer |
| `signature` | string | EIP-191 personal_sign of the signing message (hex, 65 bytes) |

**Signing message:**

```
vela:order:{market}:{side}:{price}:{quantity}:{nonce}
```

Where `side` is lowercase (`"bid"` or `"ask"`), `price` and `quantity` are decimal representations of the raw u64 values, and `nonce` is decimal. Sign with `personal_sign` (EIP-191), which prepends `"\x19Ethereum Signed Message:\n{len}"` before hashing with keccak256.

Example (JavaScript):
```js
const msg = `vela:order:BTC-USDC:bid:4999950000000:50000000:42`;
const sig = await signer.signMessage(msg);
```

**Error codes returned in `Response::Error`:**

| Code | Meaning |
|---|---|
| `InvalidNonce` | Nonce already used or too old |
| `InsufficientBalance` | Available balance < required collateral |
| `CreditLimitExceeded` | `current_quoted + new_notional > deposited × credit_ratio` |
| `MarketNotFound` | Unknown market ID |
| `OrderBookFull` | Book at `max_orders` capacity |
| `PostOnlyWouldMatch` | Post-only order would immediately match |
| `FokNotFilled` | Fill-or-kill order could not be fully filled |
| `DuplicateClientOrderId` | `client_order_id` already exists for this user |
| `InvalidSignature` | Signature does not match `address` |

Returns `401` on signature failure, `400` on address parse error, `200` otherwise (engine errors are in `Response::Error` within `data`).

---

### `POST /orders/cancel`

Cancel an existing order. Requires ECDSA signature.

**Request body:**

```json
{
  "order_id":        12345,
  "client_order_id": null,
  "nonce":           43,
  "address":         "0xAbCd1234...",
  "signature":       "0x<65-byte-hex>"
}
```

Provide either `order_id` or `client_order_id`; at least one must be non-null.

**Signing message:**

```
vela:cancel:{order_id}:{client_order_id}:{nonce}
```

Use empty string for whichever identifier is absent:

```js
// Cancel by order_id only:
const msg = `vela:cancel:12345::43`;

// Cancel by client_order_id only:
const msg = `vela:cancel::my-order-label:43`;
```

---

### `POST /withdrawals`

Initiate a withdrawal. Requires ECDSA signature. The withdrawal is included in the next committed batch; L1 settlement becomes available after the batch's proof is finalized (7-day optimistic window or fast-finality ZK proof).

**Request body:**

```json
{
  "asset":     "USDC",
  "amount":    1000000000000,
  "nonce":     44,
  "address":   "0xAbCd1234...",
  "signature": "0x<65-byte-hex>"
}
```

`amount` is raw u64 (amount × 10^8).

**Signing message:**

```
vela:withdraw:{asset}:{amount}:{nonce}
```

```js
const msg = `vela:withdraw:USDC:1000000000000:44`;
const sig = await signer.signMessage(msg);
```

---

## WebSocket Protocol

Connect to `ws://<host>/ws`. All messages are JSON text frames.

Messages use a tagged union format:

```json
{ "type": "<message_type>", ...fields }
```

---

### Client → Server Messages

#### `subscribe`

Subscribe to one or more channels. Immediately receive a book snapshot for any `book.*` channel.

```json
{ "type": "subscribe", "channels": ["book.BTC-USDC", "book.ETH-USDC"] }
```

Supported channel names:
- `book.<market>` — public order book updates and trade events for the specified market

#### `unsubscribe`

```json
{ "type": "unsubscribe", "channels": ["book.BTC-USDC"] }
```

#### `request_challenge`

Step 1 of private feed authentication. The server responds with a single-use challenge nonce.

```json
{ "type": "request_challenge" }
```

#### `auth`

Step 2 of private feed authentication. Sign the server-issued nonce to prove address ownership.

```json
{
  "type":      "auth",
  "address":   "0xAbCd1234...",
  "nonce":     "a3f9b2c1d4e5f607...",
  "signature": "0x<65-byte-hex>"
}
```

**Signing message:**

```
vela:auth:{nonce}
```

```js
const msg = `vela:auth:${serverNonce}`;
const sig = await signer.signMessage(msg);
```

The nonce must exactly match the value received in the `challenge` response. The server rejects auth if no `request_challenge` was sent first, or if the nonce does not match.

#### `session_auth`

Preferred browser flow. Reuse the short-lived HTTP session so the user does not
sign a second challenge for the private WebSocket:

```json
{
  "type": "session_auth",
  "address": "0xAbCd1234...",
  "token": "<opaque-session-token>"
}
```

#### `ping`

```json
{ "type": "ping" }
```

---

### Server → Client Messages

#### `subscribed`

Acknowledgement of a subscribe or unsubscribe request.

```json
{ "type": "subscribed", "channels": ["book.BTC-USDC"] }
```

#### `book_snapshot`

Sent immediately after subscribing to a `book.*` channel. Contains up to 20 levels per side.

```json
{
  "type":   "book_snapshot",
  "market": "BTC-USDC",
  "bids":   [["49999.50", "0.5"], ["49998.00", "1.2"]],
  "asks":   [["50000.25", "0.3"], ["50001.00", "2.0"]]
}
```

Each entry is `[price, quantity]` as decimal strings.

#### `trade`

A fill event. Sent to all subscribers of the relevant `book.*` channel.

```json
{
  "type":      "trade",
  "market":    "BTC-USDC",
  "price":     "50000.00",
  "quantity":  "0.1",
  "side":      "bid",
  "timestamp": 1712345678000000
}
```

#### `order_update`

**Private.** Sent to the order owner when an order is posted, partially filled, filled, or canceled.

```json
{
  "type":            "order_update",
  "order_id":        12345,
  "status":          "open",
  "filled_quantity": "0"
}
```

`status` values: `"open"`, `"partiallyfilled"`, `"filled"`, `"canceled"`, `"rejected"`

#### `fill`

**Private.** Sent to both the maker and taker when a fill occurs.

```json
{
  "type":           "fill",
  "maker_order_id": 100,
  "taker_order_id": 200,
  "price":          "50000.00",
  "quantity":       "0.1",
  "side":           "bid",
  "maker_fee":      "-1000",
  "taker_fee":      "3500",
  "timestamp":      1712345678000000
}
```

`maker_fee` and `taker_fee` are signed raw integers (negative = rebate). Divide by `10^8` for decimal value.

#### `balance_update`

**Private.** Sent when a user's balance changes (deposit, withdrawal, fill).

```json
{
  "type":      "balance_update",
  "asset":     "USDC",
  "available": "9850.00",
  "locked":    "150.00"
}
```

#### `challenge`

Response to `request_challenge`.

```json
{ "type": "challenge", "nonce": "a3f9b2c1d4e5f607a8b9c0d1e2f30405" }
```

`nonce` is 16 random bytes as a 32-character lowercase hex string.

#### `authenticated`

Response to a successful `auth`.

```json
{ "type": "authenticated", "address": "0xAbCd1234..." }
```

After this message, the connection receives private events for the authenticated address.

#### `error`

```json
{ "type": "error", "code": "AUTH_FAILED", "message": "invalid signature" }
```

Error codes:
- `INVALID_MESSAGE` — JSON parse failure
- `NO_CHALLENGE` — `auth` sent without prior `request_challenge`
- `INVALID_NONCE` — nonce in `auth` does not match server challenge
- `AUTH_FAILED` — ECDSA signature verification failed

#### `pong`

Response to `ping`.

```json
{ "type": "pong" }
```

---

## Authentication Summary

All three mutation endpoints (`POST /orders`, `POST /orders/cancel`, `POST /withdrawals`) require an ECDSA signature from the address specified in the `address` field. The signature scheme is EIP-191 `personal_sign`:

```
hash = keccak256("\x19Ethereum Signed Message:\n" + len(msg) + msg)
sig  = secp256k1_sign(hash, private_key)   // 65 bytes: r (32) + s (32) + v (1)
```

The server recovers the signer address from `(hash, sig)` and rejects the request if it does not match `address`. Standard Ethereum wallets (MetaMask, ethers.js `signer.signMessage()`, viem `signMessage`) produce compatible signatures.

For WebSocket private feeds, a separate challenge/response flow proves address ownership without submitting any on-chain transaction. The server issues a single-use nonce; the client signs `"vela:auth:{nonce}"` and presents the signature in the `auth` message.

---

## Numeric Encoding

All prices and quantities in the API use `10^8` as the scaling factor:

| Human-readable | Raw u64 |
|---|---|
| 1.0 USDC | `100000000` |
| 50000.00 USDC | `5000000000000` |
| 0.001 BTC | `100000` |
| 1.5 BTC | `150000000` |

In signing messages and POST request bodies, use the raw u64 integer. In GET responses and WebSocket messages, amounts are returned as formatted decimal strings with trailing zeros removed.

The internal representation uses two separate scales — `PRICE_SCALE = 10^8` and `QUANTITY_SCALE = 10^8` — so notional values are computed with `u128` intermediates to prevent overflow:

```
notional = (price × quantity) / QUANTITY_SCALE
```

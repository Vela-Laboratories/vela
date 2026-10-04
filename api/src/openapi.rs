//! OpenAPI 3.1 spec + Redoc docs page.
//!
//! This module publishes a machine-readable schema of Vela's HTTP API so
//! institutional clients can generate typed SDKs (py / ts / rust) via
//! `openapi-generator-cli` and CI-verify their integration against a
//! versioned contract. The spec is hand-authored (not runtime-generated)
//! because we want tight control over descriptions, examples, and error
//! shapes.
//!
//! Consumers:
//! - `GET /openapi.json` — machine-readable spec.
//! - `GET /docs` — human-readable Redoc-rendered docs (CDN, no deps).
//!
//! Versioning contract:
//! - Every documented route is stable within the current major version.
//! - Breaking changes bump to `/v2` (and this spec's `info.version`).
//! - New response fields and new endpoints are additive within a version.

use axum::response::{Html, IntoResponse};
use axum::Json;

/// The OpenAPI document. Kept inline as a big JSON literal so this file
/// stays in the same review context as the routing table it documents.
/// If the spec grows past a few hundred lines this should move into
/// `api/openapi.json` and be `include_str!`'d.
pub fn openapi_spec() -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Vela Exchange API",
            "version": "0.4.0",
            "description": "HTTP + WebSocket API for the Vela verifiable spot DEX. See https://github.com/Monolith-Investments-LP/vela for source and https://vela.monolithsystematic.com for live beta.",
            "contact": { "email": "asomu@ucsd.edu" }
        },
        "servers": [
            { "url": "https://vela-engine.fly.dev", "description": "Live beta (Sepolia)" },
            { "url": "http://127.0.0.1:3001", "description": "Local dev" }
        ],
        "paths": {
            "/health": {
                "get": { "summary": "Liveness check", "responses": { "200": { "description": "ok" } } }
            },
            "/ready": {
                "get": {
                    "summary": "Dependency readiness check",
                    "responses": {
                        "200": { "description": "Ready to serve this runtime mode" },
                        "503": { "description": "A required dependency is unavailable" }
                    }
                }
            },
            "/metrics": {
                "get": { "summary": "Prometheus metrics", "responses": { "200": { "description": "Prometheus text 0.0.4" } } }
            },
            "/auth/challenge": {
                "post": {
                    "summary": "Issue a single-use wallet authentication challenge",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/HttpAuthChallengeRequest" } } } },
                    "responses": {
                        "200": { "description": "Versioned message to sign", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/HttpAuthChallengeResponse" } } } },
                        "400": { "description": "Invalid address" },
                        "429": { "description": "Challenge capacity reached" }
                    }
                }
            },
            "/auth/session": {
                "post": {
                    "summary": "Exchange a wallet signature for a short-lived account session",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/HttpAuthSessionRequest" } } } },
                    "responses": {
                        "200": { "description": "Opaque bearer session", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/HttpAuthSessionResponse" } } } },
                        "401": { "description": "Invalid or replayed proof" },
                        "503": { "description": "Session capacity reached" }
                    }
                }
            },
            "/auth/session/revoke": {
                "post": {
                    "summary": "Revoke the current account session",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Session revoked" },
                        "401": { "description": "Invalid or expired session" }
                    }
                }
            },
            "/markets": {
                "get": {
                    "summary": "List all markets",
                    "responses": {
                        "200": {
                            "description": "Markets with best bid/ask/spread",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/MarketResponse" } }
                                }
                            } } }
                        }
                    }
                }
            },
            "/markets/{market}/book": {
                "get": {
                    "summary": "Order book snapshot",
                    "parameters": [{ "name": "market", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Bids + asks up to N levels" } }
                }
            },
            "/orders": {
                "post": {
                    "summary": "Place a signed order (master or agent-wallet)",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PostOrderBody" } } } },
                    "responses": {
                        "200": { "description": "Order accepted; response contains fills + status" },
                        "401": { "description": "Signature invalid or agent cap exceeded" },
                        "504": { "description": "Engine dispatch timed out" }
                    }
                }
            },
            "/orders/cancel": {
                "post": {
                    "summary": "Cancel an order by id or client_order_id",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CancelOrderBody" } } } },
                    "responses": { "200": { "description": "Cancel result" } }
                }
            },
            "/orders/{order_id}": {
                "get": {
                    "summary": "Order lifecycle with fill history",
                    "parameters": [{ "name": "order_id", "in": "path", "required": true, "schema": { "type": "integer" } }],
                    "responses": { "200": { "description": "Order details" } }
                }
            },
            "/orders/algo/twap": {
                "post": {
                    "summary": "Start a server-side TWAP",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/TwapAlgoBody" } } } },
                    "responses": { "200": { "description": "parent_id + status" } }
                }
            },
            "/orders/algo/cancel": {
                "post": {
                    "summary": "Cancel a running algo by parent_id",
                    "responses": { "200": { "description": "Canceled" } }
                }
            },
            "/orders/algo/{parent_id}": {
                "get": {
                    "summary": "Algo snapshot",
                    "parameters": [{ "name": "parent_id", "in": "path", "required": true, "schema": { "type": "integer" } }],
                    "responses": { "200": { "description": "TWAP parent status" } }
                }
            },
            "/agents/register": {
                "post": {
                    "summary": "Register a session-key / agent wallet",
                    "responses": { "200": { "description": "Delegation stored" } }
                }
            },
            "/agents/revoke": {
                "post": { "summary": "Revoke an agent", "responses": { "200": { "description": "Revoked" } } }
            },
            "/agents/{master}": {
                "get": {
                    "summary": "List agents for a master wallet",
                    "parameters": [{ "name": "master", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Agents + status" } }
                }
            },
            "/account/{address}/balances": {
                "get": {
                    "summary": "Balances by asset",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Balances",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/BalanceResponse" } }
                                }
                            } } }
                        }
                    }
                }
            },
            "/account/{address}/deposits": {
                "get": {
                    "summary": "Authoritative verified-deposit status and chain evidence",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "address", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "transaction_hash", "in": "query", "required": false, "schema": { "type": "string", "pattern": "^0x[0-9a-fA-F]{64}$" } },
                        { "name": "limit", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 } }
                    ],
                    "responses": {
                        "200": {
                            "description": "Ledger-backed deposit states",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/DepositStatusResponse" } }
                                }
                            } } }
                        },
                        "400": { "description": "Invalid address, transaction hash, or limit" },
                        "503": { "description": "Authoritative ledger is not configured or unavailable" }
                    }
                }
            },
            "/account/{address}/withdrawals": {
                "get": {
                    "summary": "Authoritative customer withdrawal reservations and status",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "address", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "limit", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 } }
                    ],
                    "responses": {
                        "200": {
                            "description": "Ledger-backed withdrawal states",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/WithdrawalStatusResponse" } }
                                }
                            } } }
                        },
                        "400": { "description": "Invalid address or limit" },
                        "503": { "description": "Authoritative ledger is not configured or unavailable" }
                    }
                }
            },
            "/perp/markets": {
                "get": {
                    "summary": "Perp market state (mark/index/funding)",
                    "responses": {
                        "200": {
                            "description": "Perp markets",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/PerpMarket" } }
                                }
                            } } }
                        }
                    }
                }
            },
            "/perp/account/{address}": {
                "get": {
                    "summary": "Perp positions + margin report for one address",
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Perp account",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "$ref": "#/components/schemas/PerpAccount" }
                                }
                            } } }
                        }
                    }
                }
            },
            "/perp/liquidatable": {
                "get": {
                    "summary": "Positions below maintenance margin (eligible for public liquidator)",
                    "responses": {
                        "200": {
                            "description": "Candidates",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/PerpLiquidationCandidate" } }
                                }
                            } } }
                        }
                    }
                }
            },
            "/perp/liquidate": {
                "post": {
                    "summary": "Execute a public perp liquidation",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/PerpLiquidateBody" } } } },
                    "responses": {
                        "200": { "description": "Liquidation applied" },
                        "401": { "description": "Signature invalid" },
                        "404": { "description": "Unknown market" },
                        "409": { "description": "Borrower not liquidatable / no open position" }
                    }
                }
            },
            "/borrow-lend/markets": {
                "get": {
                    "summary": "Borrow-lend market state",
                    "responses": {
                        "200": {
                            "description": "Markets",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/BorrowLendMarket" } }
                                }
                            } } }
                        }
                    }
                }
            },
            "/borrow-lend/account/{address}": {
                "get": {
                    "summary": "Borrow-lend positions + health factor",
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "Account",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "$ref": "#/components/schemas/BorrowLendAccount" }
                                }
                            } } }
                        }
                    }
                }
            },
            "/tee/stats": {
                "get": {
                    "summary": "TEE attestation counts + platform label",
                    "responses": {
                        "200": {
                            "description": "TEE stats",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "$ref": "#/components/schemas/TeeStats" }
                                }
                            } } }
                        }
                    }
                }
            },
            "/proofs/stats": {
                "get": {
                    "summary": "ZK proof counts + provider label",
                    "responses": {
                        "200": {
                            "description": "Proof stats",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "$ref": "#/components/schemas/ProofStats" }
                                }
                            } } }
                        }
                    }
                }
            },
            "/account/{address}/orders": {
                "get": {
                    "summary": "Open orders for an account",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Open orders" } }
                }
            },
            "/account/{address}/orders/by-client-id/{client_id}": {
                "get": {
                    "summary": "Look up an account order by client order ID",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "address", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "client_id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Order owned by the authenticated account" },
                        "401": { "description": "Missing or invalid session" },
                        "403": { "description": "Session belongs to another account" },
                        "404": { "description": "Order not found" }
                    }
                }
            },
            "/account/{address}/fills": {
                "get": {
                    "summary": "Private fill history for an account",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": {
                        "200": {
                            "description": "At most 500 newest account fills",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "type": "array", "items": { "$ref": "#/components/schemas/AccountFillResponse" } }
                                }
                            } } }
                        },
                        "401": { "description": "Missing or invalid session" },
                        "403": { "description": "Session belongs to another account" }
                    }
                }
            },
            "/portfolio/{address}": {
                "get": {
                    "summary": "Realized + unrealized PnL, cost basis, per market",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "address", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "method", "in": "query", "required": false, "schema": { "type": "string", "enum": ["fifo", "hifo"] } }
                    ],
                    "responses": {
                        "200": {
                            "description": "Portfolio breakdown",
                            "content": { "application/json": { "schema": {
                                "type": "object",
                                "properties": {
                                    "ok": { "type": "boolean" },
                                    "data": { "$ref": "#/components/schemas/PortfolioResponse" }
                                }
                            } } }
                        }
                    }
                }
            },
            "/portfolio/{address}/csv": {
                "get": {
                    "summary": "Full fill history as CSV (Koinly/CoinTracker)",
                    "security": [{ "BearerAuth": [] }],
                    "responses": { "200": { "description": "text/csv", "content": { "text/csv": {} } } }
                }
            },
            "/points/{address}": {
                "get": {
                    "summary": "Points + toxicity-gated fill counts",
                    "responses": { "200": { "description": "Points breakdown" } }
                }
            },
            "/leaderboard": {
                "get": { "summary": "Top traders by points, 30d rolling", "responses": { "200": { "description": "Leaderboard" } } }
            },
            "/fees/schedule": {
                "get": { "summary": "Volume-tiered fee schedule", "responses": { "200": { "description": "Tier table" } } }
            },
            "/fees/tier/{address}": {
                "get": {
                    "summary": "Current fee tier for an address",
                    "parameters": [{ "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Tier + volume + next threshold" } }
                }
            },
            "/deposit": {
                "post": {
                    "summary": "Create an unverified demo credit (testnet-demo only)",
                    "responses": { "200": { "description": "Demo credit created" }, "503": { "description": "Disabled outside testnet-demo" } }
                }
            },
            "/deposit/bridge": {
                "post": {
                    "summary": "Cross-chain bridge-attested deposit",
                    "responses": { "200": { "description": "Credited" }, "401": { "description": "Bridge sig invalid or not allowlisted" } }
                }
            },
            "/deposit/bridges": {
                "get": { "summary": "Public bridge allowlist", "responses": { "200": { "description": "Bridges + pubkeys" } } }
            },
            "/reserves/public": {
                "get": {
                    "summary": "Sanitized aggregate engine liabilities",
                    "responses": { "200": { "description": "Aggregate liabilities with scope and verification status" } }
                }
            },
            "/withdrawals": {
                "post": {
                    "summary": "Authenticate and atomically reserve a customer withdrawal",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/WithdrawBody" } } } },
                    "responses": {
                        "200": { "description": "Withdrawal durably reserved" },
                        "400": { "description": "Invalid request" },
                        "401": { "description": "Invalid customer signature" },
                        "409": { "description": "Insufficient balance or idempotency conflict" },
                        "503": { "description": "Authoritative reservation unavailable" }
                    }
                }
            },
            "/withdrawals/{id}/cancel": {
                "post": {
                    "summary": "Cancel a pre-signing withdrawal and release its reservation",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CancelWithdrawalBody" } } } },
                    "responses": {
                        "200": { "description": "Reservation released exactly once" },
                        "401": { "description": "Invalid customer signature" },
                        "404": { "description": "Withdrawal not found for customer" },
                        "409": { "description": "Withdrawal can no longer be safely cancelled" }
                    }
                }
            },
            "/vaults": {
                "get": { "summary": "List MM credit vaults + AUM", "responses": { "200": { "description": "Vaults" } } }
            },
            "/vaults/create": {
                "post": { "summary": "Create a vault (operator-signed)", "responses": { "200": { "description": "Vault created" } } }
            },
            "/vaults/{vault_id}": {
                "get": {
                    "summary": "Vault snapshot + share price",
                    "parameters": [{ "name": "vault_id", "in": "path", "required": true, "schema": { "type": "integer" } }],
                    "responses": { "200": { "description": "Vault" } }
                }
            },
            "/vaults/{vault_id}/deposit": {
                "post": { "summary": "LP deposits USDC, receives shares", "responses": { "200": { "description": "Deposited" } } }
            },
            "/vaults/{vault_id}/withdraw": {
                "post": { "summary": "LP burns shares, receives USDC", "responses": { "200": { "description": "Withdrawn" } } }
            },
            "/vaults/{vault_id}/positions/{lp}": {
                "get": { "summary": "LP position in a vault", "responses": { "200": { "description": "Position" } } }
            },
            "/listings": {
                "get": { "summary": "Pending / accepted / rejected permissionless listings", "responses": { "200": { "description": "Listings" } } }
            },
            "/listings/propose": {
                "post": { "summary": "Propose a market (posts bond)", "responses": { "200": { "description": "Proposal" } } }
            },
            "/trades": { "get": { "summary": "Recent fills (all markets)", "responses": { "200": { "description": "Fills" } } } },
            "/trades/{market_id}": {
                "get": {
                    "summary": "Recent fills for one market",
                    "parameters": [{ "name": "market_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Fills" } }
                }
            },
            "/ohlcv/{market_id}": {
                "get": {
                    "summary": "OHLCV candles derived from real fills",
                    "parameters": [{ "name": "market_id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Candles" } }
                }
            },
            "/status": {
                "get": { "summary": "Engine status + today's counters", "responses": { "200": { "description": "Status" } } }
            },
            "/ws": {
                "get": {
                    "summary": "WebSocket upgrade — see WS section for channel protocol",
                    "responses": { "101": { "description": "Switching Protocols" } }
                }
            }
        },
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "opaque-session-token",
                    "description": "Short-lived account-bound session returned by POST /auth/session"
                }
            },
            "schemas": {
                "ApiResponse": {
                    "type": "object",
                    "required": ["ok"],
                    "properties": {
                        "ok": { "type": "boolean" },
                        "data": {},
                        "error": { "type": "string" }
                    }
                },
                "HttpAuthChallengeRequest": {
                    "type": "object",
                    "required": ["address"],
                    "properties": {
                        "address": { "type": "string", "description": "0x-prefixed Ethereum address" }
                    }
                },
                "HttpAuthChallengeResponse": {
                    "type": "object",
                    "required": ["ok", "data"],
                    "properties": {
                        "ok": { "type": "boolean" },
                        "data": {
                            "type": "object",
                            "required": ["address", "nonce", "message", "expires_at_ms"],
                            "properties": {
                                "address": { "type": "string" },
                                "nonce": { "type": "string" },
                                "message": { "type": "string" },
                                "expires_at_ms": { "type": "integer", "format": "int64" }
                            }
                        }
                    }
                },
                "HttpAuthSessionRequest": {
                    "type": "object",
                    "required": ["address", "nonce", "signature"],
                    "properties": {
                        "address": { "type": "string" },
                        "nonce": { "type": "string" },
                        "signature": { "type": "string", "description": "personal_sign signature over the exact challenge message" }
                    }
                },
                "HttpAuthSessionResponse": {
                    "type": "object",
                    "required": ["ok", "data"],
                    "properties": {
                        "ok": { "type": "boolean" },
                        "data": {
                            "type": "object",
                            "required": ["address", "token", "expires_at_ms"],
                            "properties": {
                                "address": { "type": "string" },
                                "token": { "type": "string", "description": "Opaque bearer token; store in memory only" },
                                "expires_at_ms": { "type": "integer", "format": "int64" }
                            }
                        }
                    }
                },
                "PostOrderBody": {
                    "type": "object",
                    "required": ["address", "market", "side", "order_type", "price", "quantity", "nonce", "signature"],
                    "properties": {
                        "address": { "type": "string", "description": "Master wallet (order signer may be master or authorized agent)" },
                        "market": { "type": "string", "example": "BTC-USDC" },
                        "side": { "type": "string", "enum": ["Bid", "Ask"] },
                        "order_type": { "type": "string", "enum": ["GoodTillCanceled", "PostOnly", "ImmediateOrCancel", "FillOrKill"] },
                        "price": { "type": "integer", "description": "USDC × 1e6" },
                        "quantity": { "type": "integer", "description": "Base × 1e6" },
                        "nonce": { "type": "integer" },
                        "client_order_id": { "type": "string" },
                        "signature_version": { "type": "integer", "enum": [2], "description": "Required for advanced controls. V2 signs the compact JSON array [\"vela:order:v2\",market,side,order_type,price,quantity,nonce,client_order_id|null,stp,min_quantity|null,display_quantity|null]." },
                        "stp": { "type": "string", "enum": ["None", "CancelTaker", "CancelMaker", "CancelBoth", "DecrementAndCancel"], "default": "None" },
                        "min_quantity": { "type": "integer", "minimum": 1, "description": "Atomic minimum fill, base × 1e6" },
                        "display_quantity": { "type": "integer", "minimum": 1, "description": "Visible iceberg slice, base × 1e6" },
                        "signature": { "type": "string", "description": "0x-prefixed 65-byte ECDSA" }
                    }
                },
                "CancelOrderBody": {
                    "type": "object",
                    "required": ["address", "nonce", "signature"],
                    "properties": {
                        "address": { "type": "string" },
                        "order_id": { "type": "integer" },
                        "client_order_id": { "type": "string" },
                        "nonce": { "type": "integer" },
                        "signature": { "type": "string" }
                    }
                },
                "TwapAlgoBody": {
                    "type": "object",
                    "required": ["address", "market", "side", "quantity", "duration_secs", "nonce", "signature"],
                    "properties": {
                        "address": { "type": "string" },
                        "market": { "type": "string" },
                        "side": { "type": "string", "enum": ["Bid", "Ask"] },
                        "quantity": { "type": "integer" },
                        "duration_secs": { "type": "integer" },
                        "price_limit": { "type": "integer" },
                        "slices": { "type": "integer", "default": 12 },
                        "nonce": { "type": "integer" },
                        "signature": { "type": "string" }
                    }
                },
                "MarketResponse": {
                    "type": "object",
                    "required": ["id", "base", "quote"],
                    "properties": {
                        "id": { "type": "string", "example": "BTC-USDC" },
                        "base": { "type": "string" },
                        "quote": { "type": "string" },
                        "best_bid": { "type": "string" },
                        "best_ask": { "type": "string" },
                        "spread": { "type": "string" }
                    }
                },
                "BalanceResponse": {
                    "type": "object",
                    "required": ["asset", "available", "locked", "total"],
                    "properties": {
                        "asset": { "type": "string" },
                        "available": { "type": "string" },
                        "locked": { "type": "string" },
                        "total": { "type": "string" }
                    }
                },
                "AccountFillResponse": {
                    "type": "object",
                    "required": ["id", "market", "price", "quantity", "order_id", "counterparty_order_id", "role", "side", "timestamp"],
                    "properties": {
                        "id": { "type": "string" },
                        "market": { "type": "string" },
                        "price": { "type": "string" },
                        "quantity": { "type": "string" },
                        "order_id": { "type": "integer", "format": "int64" },
                        "counterparty_order_id": { "type": "integer", "format": "int64" },
                        "role": { "type": "string", "enum": ["maker", "taker"] },
                        "side": { "type": "string", "enum": ["buy", "sell"] },
                        "timestamp": { "type": "integer", "format": "int64" }
                    }
                },
                "DepositStatusResponse": {
                    "type": "object",
                    "required": [
                        "id", "asset", "amount", "amount_atomic", "state", "chain_id",
                        "block_number", "block_hash", "transaction_hash", "transaction_index",
                        "log_index", "vault_address", "vault_deposit_id", "observed_at", "updated_at"
                    ],
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "asset": { "type": "string" },
                        "amount": { "type": "string", "description": "Human-readable exact decimal amount" },
                        "amount_atomic": { "type": "string", "description": "Integer amount in the asset's smallest unit" },
                        "state": { "type": "string", "enum": ["observed", "confirming", "finalized", "credited", "rejected", "orphaned", "reversed", "quarantined"] },
                        "chain_id": { "type": "integer", "format": "int64" },
                        "block_number": { "type": "integer", "format": "int64" },
                        "block_hash": { "type": "string" },
                        "transaction_hash": { "type": "string" },
                        "transaction_index": { "type": "integer" },
                        "log_index": { "type": "integer" },
                        "vault_address": { "type": "string" },
                        "vault_deposit_id": { "type": "string" },
                        "asset_address": { "type": ["string", "null"] },
                        "observed_at": { "type": "string", "format": "date-time" },
                        "credited_at": { "type": ["string", "null"], "format": "date-time" },
                        "reversed_at": { "type": ["string", "null"], "format": "date-time" },
                        "updated_at": { "type": "string", "format": "date-time" }
                    }
                },
                "WithdrawBody": {
                    "type": "object",
                    "required": ["withdrawal_id", "asset", "amount", "nonce", "address", "destination_address", "signature"],
                    "properties": {
                        "withdrawal_id": { "type": "string", "format": "uuid" },
                        "asset": { "type": "string", "enum": ["ETH", "USDC"] },
                        "amount": { "type": "string", "pattern": "^[1-9][0-9]*$", "description": "Positive integer in the matching engine's 8-decimal units" },
                        "nonce": { "type": "integer", "format": "int64", "minimum": 0 },
                        "address": { "type": "string" },
                        "destination_address": { "type": "string" },
                        "signature": { "type": "string" }
                    }
                },
                "CancelWithdrawalBody": {
                    "type": "object",
                    "required": ["nonce", "address", "signature"],
                    "properties": {
                        "nonce": { "type": "integer", "format": "int64", "minimum": 0 },
                        "address": { "type": "string" },
                        "signature": { "type": "string" }
                    }
                },
                "WithdrawalStatusResponse": {
                    "type": "object",
                    "required": [
                        "id", "asset", "amount", "amount_atomic", "state", "destination_address",
                        "phase", "funds_status", "terminal", "can_cancel", "action_required",
                        "status_message", "chain_id", "vault_address", "nonce", "created_at", "updated_at"
                    ],
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "asset": { "type": "string", "enum": ["ETH", "USDC"] },
                        "amount": { "type": "string", "description": "Human-readable exact decimal amount" },
                        "amount_atomic": { "type": "string", "description": "Integer amount in the asset's smallest unit" },
                        "state": { "type": "string", "enum": ["requested", "reserved", "approved", "signing", "signed", "broadcast", "confirmed", "rejected", "cancelled", "signing_failed", "broadcast_failed", "replaced", "dropped", "reversed"] },
                        "phase": { "type": "string", "enum": ["review", "custody", "chain", "recovery", "complete"] },
                        "funds_status": { "type": "string", "enum": ["reserved", "released", "settled"] },
                        "terminal": { "type": "boolean" },
                        "can_cancel": { "type": "boolean" },
                        "action_required": { "type": "string", "enum": ["none", "wait", "wait_for_finality", "operator_review"] },
                        "status_message": { "type": "string" },
                        "destination_address": { "type": "string" },
                        "chain_id": { "type": "integer", "format": "int64" },
                        "vault_address": { "type": "string" },
                        "nonce": { "type": "string" },
                        "transaction_hash": { "type": ["string", "null"] },
                        "reservation_transaction_id": { "type": ["string", "null"], "format": "uuid" },
                        "completion_transaction_id": { "type": ["string", "null"], "format": "uuid" },
                        "release_transaction_id": { "type": ["string", "null"], "format": "uuid" },
                        "expires_at": { "type": ["string", "null"], "format": "date-time" },
                        "created_at": { "type": "string", "format": "date-time" },
                        "updated_at": { "type": "string", "format": "date-time" }
                    }
                },
                "PortfolioLot": {
                    "type": "object",
                    "required": ["asset", "quantity", "cost_basis_usdc", "acquired_at"],
                    "properties": {
                        "asset": { "type": "string" },
                        "quantity": { "type": "string", "description": "Decimal base-asset units" },
                        "cost_basis_usdc": { "type": "string", "description": "Decimal USDC, not atomic units" },
                        "acquired_at": { "type": "integer", "description": "Unix seconds" }
                    }
                },
                "PortfolioPerMarket": {
                    "type": "object",
                    "required": ["market", "realized_usdc", "unrealized_usdc", "pnl_complete"],
                    "properties": {
                        "market": { "type": "string" },
                        "realized_usdc": { "type": "string", "description": "Decimal USDC" },
                        "unrealized_usdc": { "type": ["string", "null"], "description": "Null when no mark price is available" },
                        "pnl_complete": { "type": "boolean", "description": "False when a sell has no matching retained acquisition lot" }
                    }
                },
                "PortfolioMarket": {
                    "type": "object",
                    "required": ["market_id", "base_asset", "position_base", "cost_basis_price_usdc", "open_cost_basis_usdc", "mark_price_usdc", "realized_pnl_usdc", "unrealized_pnl_usdc", "fill_count", "pnl_complete", "unmatched_sell_quantity_base"],
                    "properties": {
                        "market_id": { "type": "string" },
                        "base_asset": { "type": "string" },
                        "position_base": { "type": "string" },
                        "cost_basis_price_usdc": { "type": "string" },
                        "open_cost_basis_usdc": { "type": "string" },
                        "mark_price_usdc": { "type": ["string", "null"] },
                        "realized_pnl_usdc": { "type": "string" },
                        "unrealized_pnl_usdc": { "type": ["string", "null"] },
                        "fill_count": { "type": "integer" },
                        "pnl_complete": { "type": "boolean" },
                        "unmatched_sell_quantity_base": { "type": "string", "description": "Decimal base units sold without a retained acquisition lot" }
                    }
                },
                "PortfolioResponse": {
                    "type": "object",
                    "required": [
                        "address",
                        "realized_pnl_usdc",
                        "unrealized_pnl_usdc",
                        "total_realized_pnl_usdc",
                        "total_unrealized_pnl_usdc",
                        "total_pnl_usdc",
                        "cost_basis_method",
                        "tax_lots",
                        "per_market",
                        "markets"
                    ],
                    "properties": {
                        "address": { "type": "string" },
                        "realized_pnl_usdc": { "type": "string", "description": "Decimal USDC" },
                        "unrealized_pnl_usdc": { "type": "string", "description": "Decimal USDC; excludes markets without a mark" },
                        "total_realized_pnl_usdc": { "type": "string", "description": "Same value as realized_pnl_usdc" },
                        "total_unrealized_pnl_usdc": { "type": "string", "description": "Same value as unrealized_pnl_usdc" },
                        "total_pnl_usdc": { "type": "string", "description": "Decimal USDC" },
                        "cost_basis_method": { "type": "string", "enum": ["FIFO", "HIFO"] },
                        "tax_lots": { "type": "array", "items": { "$ref": "#/components/schemas/PortfolioLot" } },
                        "per_market": { "type": "array", "items": { "$ref": "#/components/schemas/PortfolioPerMarket" } },
                        "markets": { "type": "array", "items": { "$ref": "#/components/schemas/PortfolioMarket" } }
                    }
                },
                "PerpMarket": {
                    "type": "object",
                    "required": [
                        "market",
                        "mark_price_micro_usdc",
                        "index_price_micro_usdc",
                        "funding_index",
                        "funding_rate_bps_per_hour",
                        "gross_open_interest",
                        "net_open_interest",
                        "initial_margin_bps",
                        "maintenance_margin_bps",
                        "max_leverage"
                    ],
                    "properties": {
                        "market": { "type": "string", "example": "BTC-PERP" },
                        "mark_price_micro_usdc": { "type": "integer" },
                        "index_price_micro_usdc": { "type": "integer" },
                        "funding_index": { "type": "integer" },
                        "funding_rate_bps_per_hour": { "type": "integer" },
                        "gross_open_interest": { "type": "integer" },
                        "net_open_interest": { "type": "integer" },
                        "initial_margin_bps": { "type": "integer" },
                        "maintenance_margin_bps": { "type": "integer" },
                        "max_leverage": { "type": "integer" }
                    }
                },
                "PerpPosition": {
                    "type": "object",
                    "required": [
                        "market",
                        "size",
                        "entry_price_micro_usdc",
                        "realized_pnl_micro_usdc",
                        "notional_micro_usdc",
                        "unrealized_pnl_micro_usdc",
                        "initial_requirement_micro_usdc",
                        "maintenance_requirement_micro_usdc",
                        "mark_price_micro_usdc"
                    ],
                    "properties": {
                        "market": { "type": "string" },
                        "size": { "type": "string", "description": "Signed size, base × 1e6 (positive = long)" },
                        "entry_price_micro_usdc": { "type": "integer" },
                        "realized_pnl_micro_usdc": { "type": "string" },
                        "notional_micro_usdc": { "type": "string" },
                        "unrealized_pnl_micro_usdc": { "type": "string" },
                        "initial_requirement_micro_usdc": { "type": "string" },
                        "maintenance_requirement_micro_usdc": { "type": "string" },
                        "mark_price_micro_usdc": { "type": "integer" }
                    }
                },
                "PerpAccount": {
                    "type": "object",
                    "required": ["user", "positions"],
                    "properties": {
                        "user": { "type": "string" },
                        "positions": { "type": "array", "items": { "$ref": "#/components/schemas/PerpPosition" } }
                    }
                },
                "PerpLiquidationCandidate": {
                    "type": "object",
                    "required": [
                        "user",
                        "market",
                        "size",
                        "entry_price_micro_usdc",
                        "mark_price_micro_usdc",
                        "notional_micro_usdc",
                        "maintenance_requirement_micro_usdc",
                        "equity_micro_usdc"
                    ],
                    "properties": {
                        "user": { "type": "string" },
                        "market": { "type": "string" },
                        "size": { "type": "string" },
                        "entry_price_micro_usdc": { "type": "integer" },
                        "mark_price_micro_usdc": { "type": "integer" },
                        "notional_micro_usdc": { "type": "string" },
                        "maintenance_requirement_micro_usdc": { "type": "string" },
                        "equity_micro_usdc": { "type": "string" }
                    }
                },
                "PerpLiquidateBody": {
                    "type": "object",
                    "required": ["liquidator", "signature", "borrower", "market", "nonce"],
                    "properties": {
                        "liquidator": { "type": "string" },
                        "signature": { "type": "string" },
                        "borrower": { "type": "string" },
                        "market": { "type": "string" },
                        "nonce": { "type": "integer" }
                    }
                },
                "BorrowLendMarket": {
                    "type": "object",
                    "required": [
                        "asset",
                        "total_supply",
                        "total_borrows",
                        "utilization_bps",
                        "borrow_rate_apr_bps",
                        "supply_rate_apr_bps",
                        "collateral_factor_bps",
                        "liquidation_bonus_bps",
                        "price_micro_usdc"
                    ],
                    "properties": {
                        "asset": { "type": "string" },
                        "total_supply": { "type": "string" },
                        "total_borrows": { "type": "string" },
                        "utilization_bps": { "type": "integer" },
                        "borrow_rate_apr_bps": { "type": "integer" },
                        "supply_rate_apr_bps": { "type": "integer" },
                        "collateral_factor_bps": { "type": "integer" },
                        "liquidation_bonus_bps": { "type": "integer" },
                        "price_micro_usdc": { "type": "integer" }
                    }
                },
                "BorrowLendPosition": {
                    "type": "object",
                    "required": [
                        "asset",
                        "supply_native",
                        "borrow_native",
                        "supply_value_micro_usdc",
                        "borrow_value_micro_usdc"
                    ],
                    "properties": {
                        "asset": { "type": "string" },
                        "supply_native": { "type": "string" },
                        "borrow_native": { "type": "string" },
                        "supply_value_micro_usdc": { "type": "string" },
                        "borrow_value_micro_usdc": { "type": "string" }
                    }
                },
                "BorrowLendAccount": {
                    "type": "object",
                    "required": [
                        "user",
                        "positions",
                        "borrowing_power_micro_usdc",
                        "total_borrow_value_micro_usdc",
                        "health_factor_bps"
                    ],
                    "properties": {
                        "user": { "type": "string" },
                        "positions": { "type": "array", "items": { "$ref": "#/components/schemas/BorrowLendPosition" } },
                        "borrowing_power_micro_usdc": { "type": "string" },
                        "total_borrow_value_micro_usdc": { "type": "string" },
                        "health_factor_bps": { "type": "string" }
                    }
                },
                "TeeStats": {
                    "type": "object",
                    "required": [
                        "total_batches",
                        "attested",
                        "simulated",
                        "pending",
                        "failed",
                        "platform",
                        "binary_hash",
                        "platform_status"
                    ],
                    "properties": {
                        "total_batches": { "type": "integer" },
                        "attested": { "type": "integer" },
                        "simulated": { "type": "integer" },
                        "pending": { "type": "integer" },
                        "failed": { "type": "integer" },
                        "platform": { "type": "string" },
                        "binary_hash": { "type": "string" },
                        "platform_status": { "type": "string" }
                    }
                },
                "ProofStats": {
                    "type": "object",
                    "required": ["total", "proven", "pending", "skipped", "failed", "provider"],
                    "properties": {
                        "total": { "type": "integer" },
                        "proven": { "type": "integer" },
                        "pending": { "type": "integer" },
                        "skipped": { "type": "integer" },
                        "failed": { "type": "integer" },
                        "provider": { "type": "string" }
                    }
                },
                "OraclePriceEntry": {
                    "type": "object",
                    "required": ["asset", "price_micro_usdc", "timestamp_ms"],
                    "properties": {
                        "asset": { "type": "string" },
                        "price_micro_usdc": { "type": "integer" },
                        "timestamp_ms": { "type": "integer" }
                    }
                }
            }
        },
        "x-websocket": {
            "url": "wss://vela-engine.fly.dev/ws",
            "channels": {
                "orderbook:{market}": "Top-50 depth snapshots per second",
                "trades:{market}": "Fills as they land",
                "markets": "Best bid/ask/spread across all markets every 5s",
                "account:{address}": "Authenticated: balances, orders, fills for one account",
                "dropcopy:{address}": "Authenticated: fills-only, delivered on connection independent of trading",
                "feed/toxicity": "Authenticated: per-fill toxicity scores"
            },
            "auth": "Send { type: 'auth', address, signature, timestamp } after connect. `signature` covers `vela:ws:{address}:{timestamp}` via personal_sign."
        }
    })
}

/// Serve the OpenAPI JSON.
pub async fn openapi_handler() -> impl IntoResponse {
    Json(openapi_spec())
}

/// Redoc-rendered HTML for the OpenAPI spec. Uses Redoc's public CDN so
/// there's no bundling / hosting overhead.
pub async fn docs_handler() -> impl IntoResponse {
    Html(
        r#"<!doctype html>
<html>
  <head>
    <title>Vela API — reference</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <style>body { margin: 0; padding: 0; }</style>
  </head>
  <body>
    <redoc spec-url="/openapi.json"></redoc>
    <script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
  </body>
</html>"#,
    )
}

# Vela Exchange

A high-performance spot-exchange research preview built by [Monolith Systematic LLC](https://monolithsystematic.com).

**Live:** [dippi.fun](https://dippi.fun) · **Docs:** [monolithsystematicllc.mintlify.app](https://monolithsystematicllc.mintlify.app) · **White Paper:** [SSRN 6579199](https://ssrn.com/abstract=6579199)

---

## Overview

Vela is a central limit order book (CLOB) spot-exchange research system.
Orders are wallet-signed and matching is deterministic, but the current
public beta is a Sepolia demo. It does not have an authoritative production
ledger, reconciled custody, real SP1 proofs, an on-chain fraud-proof verifier,
or enforceable forced inclusion. Do not use real funds.

---

## Performance

> **Historical benchmark snapshot (May 2026).** These figures have not been
> reproduced against the current Dippi code. The workload and HTTP success
> checks were corrected in October 2026; a fresh, archived run is required
> before citing current performance. See [`BENCHMARKS.md`](BENCHMARKS.md).

The May 2026 numbers remain in [`BENCHMARKS.md`](BENCHMARKS.md) and
[`docs/benchmarks.md`](docs/benchmarks.md) for provenance. Their workload
validation, hardware, and system boundaries do not support a current Dippi
performance claim or a cross-exchange speedup. The updated technical report
in [`paper/dippi-technical-report.tex`](paper/dippi-technical-report.tex)
separates validated in-process matching, loopback HTTP, sustained HTTP load,
and the financial commit path. Read its measurement boundary and failure
counts with each new result.

---

## Performance Tuning

The batch dispatcher coalesces multiple orders into a single engine lock acquisition, amortizing CoW cache overhead across N orders per dispatch window.

| Environment Variable | Default | Description |
|----------------------|---------|-------------|
| `VELA_BATCH_WINDOW_US` | `500` | Dispatch window in microseconds. The window opens on the first incoming order and closes when it elapses or `VELA_BATCH_MAX_SIZE` orders have accumulated — whichever comes first. |
| `VELA_BATCH_MAX_SIZE` | `256` | Maximum orders coalesced into one batch dispatch. Hitting this limit triggers early dispatch before the window expires. |

**Guidance:**
- Lower `VELA_BATCH_WINDOW_US` (e.g. 100–200 µs) for lower tail latency at the cost of smaller batches and higher per-order lock overhead.
- Higher `VELA_BATCH_WINDOW_US` (e.g. 1000–2000 µs) maximises throughput by amortising lock cost across more orders; adds latency for the first order in each batch.
- `VELA_BATCH_MAX_SIZE` caps memory growth; values above 256 provide diminishing returns.

---

## Architecture
```
┌─────────────────────────────────────────────┐
│              Next.js Frontend               │
│                dippi.fun                    │
└────────────────────┬────────────────────────┘
                     │ HTTP / WebSocket / FIX
┌────────────────────▼────────────────────────┐
│           Rust Matching Engine              │
│         vela-engine.fly.dev                 │
│                                             │
│  ┌──────────┐ ┌──────────┐ ┌────────────┐  │
│  │  engine  │ │  state   │ │    api     │  │
│  └──────────┘ └──────────┘ └────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌────────────┐  │
│  │  types   │ │ committer│ │   zkvm     │  │
│  └──────────┘ └──────────┘ └────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌────────────┐  │
│  │committee │ │   tee    │ │  perp/fix  │  │
│  └──────────┘ └──────────┘ └────────────┘  │
└────────────────────┬────────────────────────┘
                     │
┌────────────────────▼────────────────────────┐
│        VelaSettlement.sol (Solidity)        │
│  0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686 │
│              Ethereum Sepolia               │
└─────────────────────────────────────────────┘
```

**Stack:** Rust · Solidity/Foundry · Next.js 16 · TypeScript · Tailwind · fly.io · Vercel

---

## Transparency

Vela exposes research and testnet telemetry for inspection:

| Feature | Description |
|---------|-------------|
| Live trade feed | Every fill, every counterparty, no anonymization, real time |
| Testnet balance comparison | Sepolia contract ETH vs. aggregate demo-engine credits; not an independent proof of reserves |
| Order audit trail | Every order's full lifecycle, wallet signature, and fill history |
| Operator disclosure | Named operator, signed commitments, exact powers and limits |
| Batch explorer | Locally produced batch records and keccak256 state roots; public data availability is not yet guaranteed |
| Verification research UI | Inspect local commitments and report discrepancies; no on-chain challenge enforcement |

All live at [dippi.fun/transparency](https://dippi.fun/transparency).

---

## Engine Features (v0.2.0)

**Order types:** GTC · Post-Only · IOC · FOK

**Market maker infrastructure:**
- Credit-system research: enforce aggregate quote limits across markets
- HFT nonce scheme: rolling 20-window, 20 concurrent in-flight orders
- Client order IDs: assign and cancel by your own identifiers
- Auto-cancel on credit breach: oldest orders cancelled to make room
- Configurable maker/taker fees: -1 bps maker rebate, 5 bps taker

**Atomicity:**
- Copy-on-Write delta buffer: failed FOK/IOC rolls back with zero state corruption
- All mutations through DeltaBuffer — atomic across the full order lifecycle

**Adverse Selection Toxicity Scoring:**
- Every taker fill scored in [0.0, 1.0] on the hot path: fixed-size ring buffer OFI accumulator, book-walk depth, and order size relative to quoted liquidity
- Score emitted per fill on the authenticated `ws://.../feed/toxicity` WebSocket channel
- Minimized allocations on the matching path via reference-based level iteration (`matchable_*_ref`); estimated hot-path overhead <50 ns at p50

**Data layer:**
- Depth-32 sparse Merkle tree with O(dirty×32) root recompute
- DA layer: all fills posted async, content-addressed, retrievable
- Forced-inclusion scaffolding: API/admin queue only; no deployed L1 delayed inbox

**Real-time feeds:**
- WebSocket: orderbook:{market}, trades:{market}, markets, account:{address}
- Private L3 feeds: authenticated account channel (personal_sign)
- Sequence numbers per channel for gap detection and reconnection

**Transparency:**
- Real OHLCV from trade history (GET /ohlcv/:market_id)
- Batch state roots (keccak256 of all fill IDs per 30s window)
- Referral program: 20% of taker fees to referrers for 90 days

---

## Smart Contract

**VelaSettlement.sol** deployed to Ethereum Sepolia
```
Address:   0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686
Network:   Ethereum Sepolia (chainId: 11155111)
Etherscan: https://sepolia.etherscan.io/address/0xAa8E680c11a883F9bf6eb980B2D4E9D18DD25686
```

| Function | Description |
|----------|-------------|
| `depositETH()` | Locks ETH — not held by operator |
| `depositToken(asset, amount)` | Locks ERC20 (USDC two-step: approve → deposit) |
| `withdraw(asset, amount, nonce, sig)` | Operator-signed, verified on-chain |
| `initiateEmergencyExit(asset)` | User-triggered 7-day timelock |
| `executeEmergencyExit(asset)` | Reclaim funds after timelock, no operator needed |

The contract enforces its own accounting rules, but this beta does not yet
connect contract balances to an authoritative post-trade ledger. Do not infer
production custody or solvency guarantees from the contract alone.

---

## Markets

16 spot markets, all vs. USDC:

`BTC` `ETH` `SOL` `AVAX` `LINK` `UNI` `ARB` `OP` `AAVE` `MATIC` `DOGE` `PEPE` `WIF` `JUP` `PENDLE` `EIGEN`

Order books maintained by an internal MM bot: CoinGecko prices every 60s, 
10 bid + 10 ask levels per market at 0.05% spread.

---

## API

Base URL: `https://vela-engine.fly.dev`

### Orders

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/orders` | Place a signed order |
| `POST` | `/orders/cancel` | Cancel by order ID or client order ID |
| `GET` | `/account/:address/orders/by-client-id/:id` | Look up by client ID |

### Market data

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/markets` | All markets with best bid/ask/spread |
| `GET` | `/orderbook/:pair` | Full order book |
| `GET` | `/ohlcv/:market_id` | OHLCV candles from real trade history |
| `GET` | `/trades` | All fills, newest first (max 500) |
| `GET` | `/trades/:market_id` | Fills filtered by market |

### Account

Customer-data reads require `Authorization: Bearer <token>`. Obtain the short-lived,
account-bound token by signing the exact message returned by `POST /auth/challenge`
and exchanging it at `POST /auth/session`. Challenges are single-use and expire
after five minutes; sessions expire after 30 minutes and can be revoked at
`POST /auth/session/revoke`.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/account/:address/balances` | User balances by asset |
| `GET` | `/account/:address/orders` | Open orders owned by the authenticated wallet |
| `GET` | `/account/:address/fills` | Private fill history owned by the authenticated wallet |
| `GET` | `/account/:address/deposits` | Authoritative verified-deposit state and chain evidence |
| `GET` | `/account/:address/withdrawals` | Authoritative withdrawal reservation and lifecycle state |
| `POST` | `/deposit` | Create an unverified demo credit (`testnet-demo` only) |
| `POST` | `/withdrawals` | Authenticate and atomically reserve an ETH/USDC withdrawal |
| `POST` | `/withdrawals/:id/cancel` | Cancel and release a pre-signing reservation exactly once |
| `POST` | `/withdrawal-signature` | Legacy demo signer; disabled whenever the authoritative ledger is configured |

### Transparency

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/orders/:order_id` | Full order lifecycle with fill history |
| `GET` | `/batches` | Trade batches with keccak256 state roots |
| `GET` | `/batches/:id` | Single batch with full fill objects |
| `GET` | `/state-root` | Current engine state root |
| `GET` | `/orders/:id/da-proof` | DA content hash for order |

### Referrals

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/referral/register` | Register a referrer |
| `GET` | `/referral/:address` | Referral stats and earnings |
| `GET` | `/leaderboard` | Top traders by volume + top referrers |

### WebSocket

`wss://vela-engine.fly.dev/ws`

Channels: `orderbook:{market_id}` · `trades:{market_id}` · `markets` · `account:{address}`

### Rate limits

| Endpoint group | Limit |
|----------------|-------|
| POST /orders, /orders/cancel | 20/min per wallet |
| POST /deposit, /withdrawals | 5/min per wallet |
| GET endpoints | 100/min per IP |

### Order signing
```
vela:order:{market_id}:{side}:{price}:{quantity}:{nonce}
vela:order:{market_id}:{side}:{price}:{quantity}:{nonce}:{client_order_id}
vela:cancel:{order_id}:{client_order_id}:{nonce}
```

Sign via MetaMask `personal_sign`. Prices/quantities in fixed-point (×1,000,000).

---

## Running Locally

### Engine

```bash
git clone https://github.com/arpjw/vela
cd vela
cargo build --release --bin api
SNAPSHOT_DIR=./data cargo run --release --bin api
```

### Frontend

```bash
cd frontend
cp .env.example .env.local
# NEXT_PUBLIC_API_URL=http://localhost:3001
# NEXT_PUBLIC_WS_URL=ws://localhost:3001
npm install && npm run dev
```

### Tests

```bash
cargo test
# 180 tests passing
```

### Benchmarks

```bash
BENCH_REPEATS=3 bash scripts/run_benchmarks.sh
# Results and run metadata: benchmark-results/<run-id>/
```

### Contracts

```bash
cd contracts && forge build && forge test
```

---

## Deployment

| Component | Platform | Region |
|-----------|----------|--------|
| Rust engine | fly.io | sjc |
| Frontend | Vercel | global |
| Domain | Cloudflare | — |
| Engine state | fly.io volume (1GB) | sjc |

```bash
flyctl deploy                      # deploy engine
cd frontend && npx vercel --prod   # deploy frontend
```

Engine snapshots to `/data/engine_snapshot.json` every 60s.
All state (orders, balances, fills, referrals) survives redeployment.

---

## Repo Structure

```
vela/
├── types/       # Shared types (Order, Fill, Market, Request, Response)
├── engine/      # Matching engine (CLOB, CoW, HFT nonces, fees)
├── state/       # Depth-32 sparse Merkle tree, delta snapshots
├── api/         # Axum HTTP + WebSocket + MM bot + snapshot + DA
│   └── src/     # Includes borrow_lend, portfolio_margin, credit,
│                # perp_service, mcp, rfq, subaccounts, strategies,
│                # reputation, verifiable_intent, agent_tox, algos,
│                # backtest_attest, prompt_firewall, reasoning_attest
├── engine/      # Core matcher (order_book, matching_engine, cow_cache,
│                # delta_buffer, market_shards, credit, ofi)
├── committer/   # Batch commitment + forced inclusion
├── committee/   # BLS12-381 threshold decryption (TEOB)
├── zkvm/        # Optimistic-ZK + SP1 prover (feature-gated)
├── tee/         # TEE attestation adapter (SEV-SNP wiring pending)
├── perp/        # Perpetual futures margin math + funding accrual
├── fix/         # FIX 4.4 acceptor (session layer)
├── contracts/   # Solidity (VelaSettlement.sol, Foundry)
├── frontend/    # Next.js 16 frontend
├── docs/        # Mintlify documentation
├── scripts/     # Benchmark runner
└── BENCHMARKS.md
```

---

## Project Status

Currently a **testnet demo** on Ethereum Sepolia. Do not use real funds. Engine
credits are not production custody balances, withdrawals are not backed by a
durable reservation workflow, and proof/forced-inclusion enforcement is not live.

**Phase 1 — Core Exchange**
- [x] Rust matching engine (6-crate workspace)
- [x] Wallet-signed orders and cancellations
- [~] Sepolia contract deposit/withdrawal demo; engine reconciliation is incomplete
- [x] Live market maker bot (CoinGecko, 16 markets)
- [x] Persistent engine state (60s snapshots)
- [x] Rate limiting and input validation

**Phase 2 — Engine Prototype**
- [x] Parallel signature verification (100k+ ops/sec ceiling)
- [x] HFT rolling 20-window nonce scheme
- [x] Client order IDs with cancel-by-client-id
- [x] Copy-on-Write delta buffer (atomic FOK/IOC)
- [x] Batched response sending
- [x] Auto-cancel on credit ratio breach
- [x] Configurable maker/taker fee framework
- [x] DA layer (content-addressed fill storage)
- [x] Depth-32 sparse Merkle tree (SMT)
- [~] Forced-inclusion engine scaffolding; deployed delayed inbox pending

**Phase 3 — Testnet Integration Prototype**
- [x] ERC20 on-chain deposits (USDC two-step approve/deposit)
- [x] Real OHLCV from trade history with LIVE/SIMULATED badge
- [x] Full WebSocket real-time feeds (orderbook, trades, account)
- [x] Market maker API guide (~850 lines, published to Mintlify)
- [x] Comprehensive benchmark suite (8 criterion benchmarks)
- [x] 16 spot markets (added PEPE, WIF, JUP, PENDLE, EIGEN)
- [x] Referral program (20% taker fee split, 90 days)
- [x] Trading leaderboard (/leaderboard)
- [ ] Smart contract security audit
- [ ] Mainnet deployment

**Phase 4 — Experimental / partially shipped**

Live in the codebase but not audit-clean; several are scaffolds or run
behind feature flags. Treat as public preview, not production.

- [~] Perpetual futures MVP — margin math + funding accrual in `perp/`;
  liquidator watcher and on-chain settlement contract pending.
- [~] Portfolio margin (spot cross-collateralization) — scenario sweep
  in `api/src/portfolio_margin.rs`; wires to real Pyth prices via the
  `oracle` module.
- [~] Spot borrow-lend — interest model + liquidation math in
  `api/src/borrow_lend.rs`; oracle-fed pricing.
- [~] FIX 4.4 acceptor — session layer in `fix/`; adapter into the
  matching engine in `api/src/fix_adapter.rs`.
- [~] ZK proving via SP1 — `PlaceholderProver` by default; real SP1
  requires the Succinct Prover Network and is selected via
  `ZKVM_PROVIDER=sp1`. Fails closed on `ENVIRONMENT=production` when
  the provider infra is missing.
- [~] TEE attestation — `PlaceholderAttester` by default; real AMD
  SEV-SNP requires attested hardware and is selected via
  `TEE_PLATFORM=amd-sev-snp`. Fails closed on production without it.
- [~] Committee (TEOB) — real BLS12-381 threshold decryption in
  `committee/`; the HTTP path between nodes still uses HMAC auth.
- [~] MCP server, capability tokens, RFQ, sub-accounts, strategies,
  reputation, verifiable intent, backtest attestation — see BUILDPLAN
  Tier 2/3 for scope. Backend endpoints exist; UI coverage is partial.

---

## Changelog

### v0.2.0 (April 2026)
Phase 2 + Phase 3 research implementation. Non-custodial production launch work remains. HFT nonces,
CoW semantics, fee framework, WebSocket feeds, real OHLCV, ERC20 
deposits, 16 markets, referral program, and benchmark suite.

### v0.1.0 (April 2026)
Initial public beta. Core exchange, transparency layer, on-chain 
ETH settlement, 11 markets, MM bot, Mintlify docs, white paper.

---

## License

MIT — see [LICENSE](./LICENSE)

---

Built by [Monolith Systematic LLC](https://monolithsystematic.com) · San Francisco, 2026

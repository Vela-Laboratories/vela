# Next 30 feature delivery

2026-10-04. Changes begin after `989442f`. Each feature and follow-up is retained as a separate commit.

| # | Requested feature | Delivery |
|---|---|---|
| 1 | Indexer Clippy fixes | Implemented; worker Clippy fixes included |
| 2 | ESLint dependency security chain | Blocked upstream; five development-only advisories remain |
| 3 | Tailwind dependency chain | Migrated to Tailwind 4; browser layout verified |
| 4 | Pinned Rust toolchain | Rust 1.99.0 in local tools, CI and container |
| 5 | Postdeployment smoke checks | Health, readiness, markets and OpenAPI checks in Fly workflow |
| 6 | Deterministic equal-time fills | Fill-ID tie breaking |
| 7 | Fixed point portfolio PnL | Integer quantity, price and PnL calculations |
| 8 | Fees in PnL and basis | Stored maker/taker fees and fee-adjusted basis |
| 9 | Missing acquisition lots | Completeness flags and frontend explanation |
| 10 | Mark provenance | Mark source and observation timestamp |
| 11 | Exact CSV export | Exact decimal amounts, IDs and fee evidence |
| 12 | FIFO/HIFO regressions | Full acquisition and disposal sequences |
| 13 | Durable agents | Runtime/chain namespace with PostgreSQL permissions and evidence |
| 14 | Persist signed registrations | Signature evidence and current permission transaction |
| 15 | Persist revocations | Tombstones and signed revocation evidence |
| 16 | Atomic monotonic nonces | Per-agent transaction lock and strict nonce advancement |
| 17 | Restart recovery | Database registry recreation tests |
| 18 | Multiinstance revocation | Separate registry instances and signature authorization tests |
| 19 | Agent scope list | Complete capability fields returned |
| 20 | Typed registration helpers | Canonical registration signing and typed client |
| 21 | External agent registration UI | Public address entry and master-wallet signature |
| 22 | Agent limits UI | Market, side, expiry and notional limits |
| 23 | Browser lifecycle test | Actual Chrome registration, scoped order and revocation |
| 24 | Stable `/v1` API | Versioned routes, OpenAPI snapshot and contract tests |
| 25 | TypeScript SDK | Typed reads and signed agent registration/revocation |
| 26 | Read-only Python SDK | Standard-library HTTP client and unit tests |
| 27 | WebSocket recovery | Gap detection, snapshot resync and reconnect tests |
| 28 | Synthetic rewards exclusion | Synthetic market maker fills excluded from points |
| 29 | Testnet financial lifecycle | Local finalized deposit, trade, withdrawal reservation and cancellation reconciliation |
| 30 | Daily reconciliation report | Repeatable-read export from immutable journal and custody evidence |

## Operational requirements

Durable agents require PostgreSQL, migration 15 and the updated database roles. The live Fly configuration remains a testnet demo; without a database it uses the in-memory registry. Daily reporting uses `DATABASE_READER_URL`; the scheduled workflow is opt-in through `VELA_DAILY_REPORT_ENABLED`. The financial lifecycle test uses deterministic local chain evidence and does not submit an onchain withdrawal.

Subaccount and vault creation fail closed when the ledger is authoritative because their metadata is still ephemeral. Regular orders, MCP orders and TWAP authorization enforce delegated scopes and quote-value caps. Notional bucket overflow is rejected.

## Dependency blocker

The current Next ESLint plugin pins `fast-glob@3.3.1`, which brings the vulnerable `braces@3.0.3` chain. The current stable and canary plugin versions retain this pin. [GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm) has no patched braces version. Production dependency audit is clean. The full dependency audit gate remains enabled and fails until upstream resolves the chain. Downgrading the lint configuration or suppressing the advisory would not establish a fix.

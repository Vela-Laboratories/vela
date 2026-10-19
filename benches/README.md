# Vela Benchmark Suite — Methodology

This document describes how the three benchmark tiers are constructed,
what they measure, and what the numbers mean.  Read this before citing
any figure from `docs/benchmarks.md`.

---

## Tier 1 — Matching-engine microbenchmark (`benches/matching.rs`)

**Scope:** Pure in-process matching-engine throughput with no HTTP, no
authentication, and no I/O.  Uses Criterion for statistical rigor.

**What it measures:** The core order-matching loop: insert-limit, cancel,
market-order matching against a pre-populated book.

**Honest caveats:**
- No ECDSA verification, no rate-limiting, no batch-dispatcher latency.
- No disk, no network.  Numbers reflect the matching algorithm only.
- Not comparable to end-to-end exchange performance.
- Stateful groups in `matching.rs` have unvalidated cancellation outcomes and
  some placeholder resting-order bookkeeping. Treat their current output as
  diagnostic until the workload is corrected and rerun.

**How to run:**

```sh
cargo bench --bench matching
```

---

## Tier 2 — End-to-end HTTP latency (`benches/e2e_http.rs`)

**Scope:** HTTP request/response lifecycle: HTTP POST → API auth →
rate-limit → batch-dispatcher window → engine lock → matching → response.
Client-side request construction and ECDSA signing occur before the timer.

**What it does NOT mock:**
- Signature verification (real k256/secp256k1 ECDSA)
- Rate limiter (DashMap per-address sliding-window, set to 10M/s for the bench)
- Batch dispatcher (500 µs coalescing window, max 256 per batch)
- Matching engine mutex

**What IS simplified:**
- Transport: loopback only (`127.0.0.1`).  No real network hop.
- No BFT consensus.  Vela's execution layer is measured in isolation.
- All keys pre-funded to 1B USDC + 1B base asset; no margin calls.

### Scenarios

| Scenario | Clients | Duration | Key pool |
|---|---|---|---|
| S1 — single-threaded | 1 | 30 s warmup + 120 s measure | keys 0–4999 |
| S2 — concurrent-32 | 32 | 30 s warmup + 120 s measure | keys 5000–9999 |
| S3 — burst-1000 | 1000 | one-shot, all-concurrent | keys 10000–10999 |

Key pools are non-overlapping to prevent nonce conflicts.  Each key
maintains a monotonically increasing per-address nonce.  Within S2 each
task gets an exclusive slice of keys (~156 per task) to eliminate
inter-task nonce collisions. S2 inserts a 50 ms think interval after each
request per client, keeping the number of resting orders per account bounded;
it is a latency test at finite offered load, not a maximum-throughput test.

Timing wraps `client.send().await` + `response.bytes().await` only.
Request construction and signing happen before the timer starts.
The harness records every measured attempt with its HTTP status (or status 0
for a transport failure). Only successful responses enter latency percentiles;
attempted, successful, and failed requests are reported separately. When
`VELA_BENCH_RESULTS_DIR` is set, it saves each attempt and status in CSV.
`scripts/run_benchmarks.sh` generates a temporary admin token and uses a
5,000 ms engine-dispatch timeout for the HTTP suites. The timeout is recorded
in run metadata and can be changed with `BENCH_DISPATCH_TIMEOUT_MS`.

### Warmup rationale

30 seconds is long enough for the batch-dispatcher queue, rate-limiter
DashMap, and Tokio's thread pool to reach thermal equilibrium.  Without
warmup, the first few thousand requests inflate p99 significantly.

**How to run:**

```sh
cargo bench --bench e2e_http
```

Expected runtime: at least 305 seconds (30 + 120 + 30 + 120 seconds,
plus burst and setup).

---

## Tier 3 — Sustained throughput (`benches/sustained_throughput.rs`)

**Scope:** 5 independent market-maker agents operating simultaneously for
300 seconds.  Tests whether performance holds over time (no GC cliff, no
lock-convoy, no memory runaway).

**Branch weights (per MM agent, random each iteration):**

| Weight | Operation |
|---|---|
| 60% | Cancel-repost: cancel a resting order, post a new limit at updated price |
| 20% | New GTC limit: post both sides around mid-price |
| 15% | Aggressive IOC: cross the spread by 5% to generate actual fills |
|  5% | Public market query: `GET /markets` |

If the resting-order queue is empty, a cancel-repost branch issues a new
post instead. The percentages are branch probabilities, not observed HTTP
operation shares.

10,000 unique addresses (2,000 per MM), all pre-funded.  Each MM agent
operates on 3–5 of the 11 markets (randomly selected at startup).

5-second sampling buckets produce a 60-point time series for visual
inspection of throughput stability.

**Memory tracking:** RSS sampled at t=0, t=150 s, t=300 s via
`/proc/self/status` (Linux) or `ps -o rss=` (macOS).

The sustained harness reports attempted, successful, and failed HTTP
operations separately. Its latency is time to response headers for successful
requests, not matcher-only time. With `VELA_BENCH_RESULTS_DIR` set, it saves
successful request latencies and five-second throughput buckets as CSV.

**Honest caveats:**
- Cancel-repost uses two HTTP requests when a resting order is available,
  while the empty-queue fallback uses one. Inspect the measured buckets and
  success rate before interpreting a throughput change.
- `127.0.0.1` loopback.  No real network latency.
- No consensus.

**How to run:**

```sh
cargo bench --bench sustained_throughput
```

Expected runtime: ~5.5 minutes (setup + 300 s run).

---

## Historical comparisons (not current evidence)

The values below come from the May 2026 benchmark notes. The workload and
success checks have since changed, and the hardware differs across projects.
Do not use these tables as current Vela results or as a measured speedup.

The earlier Vela-versus-Pulse ratio lacks matched hardware and validated
request outcomes. The Hyperliquid figures use a different system boundary.
Neither comparison establishes a Vela speedup. See `BENCHMARKS.md` for the
archived values and their provenance; the new technical report uses only
measurements with an explicit workload and timing boundary.

---

## Tier 4 — Additional workload benchmarks (`benches/matching.rs`)

Four new benchmark groups added in addition to the existing seven:

| Group | What it measures |
|---|---|
| `fill_ratio_sweep` | Cancel/fill ratio variants: 98/2, 90/10, 80/20, 50/50 |
| `concurrent_takers` | N simultaneous taker IOCs (1, 4, 8, 16) — amortised per-taker latency |
| `burst_profile` | 50-MM burst (cancel+repost) + recovery probe |
| `deep_book` | Insert/cancel cost at 10, 100, 1 000, 5 000 price levels per side |

All new benchmarks also emit an HDR latency histogram (10 equal-width buckets)
alongside the existing `[latency]` percentile lines.

---

## Environment notes

Benchmarks set the following environment variables before starting:

```
VELA_ORDER_RATE_LIMIT=10000000   # effectively unlimited for the bench
VELA_RATE_WINDOW_SECS=1
VELA_PYTH_ENABLED=false          # suppress live price feed during bench
VELA_BATCH_WINDOW_US=500         # 500 µs batch-coalescing window (default)
DA_DIR=<tmpdir>                  # ephemeral DA storage
```

WAL and DA directories are created under `$TMPDIR` and deleted on process
exit.  Do not run benchmarks against a production data directory.

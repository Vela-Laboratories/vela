# syntax=docker/dockerfile:1.7

# ── Build stage ──────────────────────────────────────────────────────────────
FROM rust:1.88-slim AS builder
WORKDIR /app

# Prime the dependency layer separately from source so incremental builds
# don't rebuild every dependency on every code change.
COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml           api/Cargo.toml
COPY engine/Cargo.toml        engine/Cargo.toml
COPY state/Cargo.toml         state/Cargo.toml
COPY types/Cargo.toml         types/Cargo.toml
COPY committer/Cargo.toml     committer/Cargo.toml
COPY committee/Cargo.toml     committee/Cargo.toml
COPY tee/Cargo.toml           tee/Cargo.toml
COPY zkvm/Cargo.toml          zkvm/Cargo.toml
COPY perp/Cargo.toml          perp/Cargo.toml
COPY fix/Cargo.toml           fix/Cargo.toml
COPY ledger/Cargo.toml        ledger/Cargo.toml
COPY ledger/migrations        ledger/migrations
COPY indexer/Cargo.toml       indexer/Cargo.toml
COPY withdrawal-worker/Cargo.toml withdrawal-worker/Cargo.toml
COPY reconciliation-worker/Cargo.toml reconciliation-worker/Cargo.toml
COPY benches/Cargo.toml       benches/Cargo.toml
RUN mkdir -p api/src engine/src engine/benches state/src types/src committer/src committee/src tee/src zkvm/src perp/src fix/src ledger/src/bin indexer/src/bin withdrawal-worker/src reconciliation-worker/src benches/benches \
    && echo 'fn main() {}' > api/src/main.rs \
    && for d in engine state types committer committee tee zkvm perp fix ledger; do echo '' > $d/src/lib.rs; done \
    && echo 'fn main() {}' > ledger/src/bin/migrate.rs \
    && echo 'fn main() {}' > ledger/src/bin/bootstrap.rs \
    && echo '' > indexer/src/lib.rs \
    && echo 'fn main() {}' > indexer/src/main.rs \
    && echo 'fn main() {}' > indexer/src/bin/register.rs \
    && echo '' > withdrawal-worker/src/lib.rs \
    && echo 'fn main() {}' > withdrawal-worker/src/main.rs \
    && echo '' > reconciliation-worker/src/lib.rs \
    && echo 'fn main() {}' > reconciliation-worker/src/main.rs \
    && echo 'fn main() {}' > engine/benches/matching_engine_bench.rs \
    && echo 'fn main() {}' > benches/benches/matching.rs \
    && echo 'fn main() {}' > benches/benches/e2e_http.rs \
    && echo 'fn main() {}' > benches/benches/sustained_throughput.rs \
    && cargo build --release --bin api \
    && rm -rf api engine state types committer committee tee zkvm perp fix ledger indexer withdrawal-worker reconciliation-worker benches

# Now copy the real source and build for real.
COPY . .
RUN find api/src engine/src state/src types/src committer/src committee/src tee/src zkvm/src perp/src fix/src ledger/src indexer/src withdrawal-worker/src reconciliation-worker/src \
      -type f -exec touch {} + \
    && cargo build --release --bin api --bin ledger-migrate --bin ledger-bootstrap \
      --bin deposit-indexer --bin deposit-indexer-register

# ── Runtime stage ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 vela \
    && useradd  --system --uid 10001 --gid vela --home-dir /app --shell /usr/sbin/nologin vela \
    && mkdir -p /data \
    && chown -R vela:vela /data

COPY --from=builder /app/target/release/api /usr/local/bin/api
COPY --from=builder /app/target/release/ledger-migrate /usr/local/bin/ledger-migrate
COPY --from=builder /app/target/release/ledger-bootstrap /usr/local/bin/ledger-bootstrap
COPY --from=builder /app/target/release/deposit-indexer /usr/local/bin/deposit-indexer
COPY --from=builder /app/target/release/deposit-indexer-register /usr/local/bin/deposit-indexer-register

USER vela
WORKDIR /app
EXPOSE 3001

# Container-level healthcheck. Fly's [http_service.checks] block is the
# authoritative signal for the platform; this makes `docker ps`/local runs
# also expose health.
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3001/health || exit 1

CMD ["api"]

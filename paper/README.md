# Dippi technical report and benchmark evidence

## LaTeX format and build

The report uses the full-version LNCS layout and alphabetic BibTeX style from
Geoffroy Couteau's cryptography-paper starter template at
https://geoffroycouteau.github.io/latex/. The included llncs.cls (v2.17)
and alpha-short.bst came from its downloadable Template_Latex.zip. The
report uses the template's full-version A4 layout and alphabetic references.
Its content and references.bib are specific to Dippi. The template's
anonymous-submission settings, unused crypto macros, and generic crypto
bibliography are not used.

Build from the repository root with a TeX installation or Tectonic that can
run BibTeX:

    tectonic --outdir paper paper/dippi-technical-report.tex

The source is a multi-file LaTeX project. The desktop standalone-document
preview does not compile its additional class, style, and bibliography files;
use the command above for the final PDF.

`dippi-technical-report.tex` is the editable source for the website PDF. The
baseline claims refer to repository commit
`940f3a48fe413fa83ff997fa3d6a770ac9488df8`; the baseline run records
also identify the exact uncommitted harness diff by SHA-256. The follow-up
repair and optimization source was archived before its CPU campaign; its
SHA-256 is
`d5b52c78bce4c760d4b2f43fd333ab39ebd99ef13ca95ef1c7faa41fe7d43f14`.
Neither a bare checkout of the baseline commit nor a later working tree
reproduces both campaigns without the recorded patches or source archive.

## Reproduce the CPU campaign

Run on a dedicated, otherwise idle machine with the Rust toolchain and the
repository's locked dependencies available. The runner writes to a new
`benchmark-results/<UTC timestamp>-<commit>/` directory and records machine
metadata. The HTTP suites generate temporary synthetic accounts and an
ephemeral local admin token. They do not connect to PostgreSQL, an external
chain, or real customer accounts.

```sh
BENCH_REPEATS=3 bash scripts/run_benchmarks.sh engine matching
BENCH_REPEATS=3 BENCH_DISPATCH_TIMEOUT_MS=5000 \
  bash scripts/run_benchmarks.sh e2e_http sustained_throughput
python3 paper/analyze_benchmarks.py \
  --engine-run benchmark-results/<engine-run-id> \
  --http-run benchmark-results/<http-run-id>
```

Each HTTP and sustained repetition lasts roughly 10 minutes before setup and compilation:
two scenarios use 30 seconds of warmup and 120 seconds of measurement, followed
by a 1,000-request burst and a 300-second sustained run. The isolated Criterion
suite uses different timing boundaries. Never combine its operation rate with
HTTP round-trip latency or call either figure a financial commit rate.

`analyze_benchmarks.py` reads raw successful HTTP timings and status counts,
Criterion estimates, and five-second sustained buckets. A failed HTTP attempt
remains in the failure denominator but does not enter the success latency
percentiles. The legacy `matching` suite has stateful fixture bookkeeping
problems and is retained as exploratory evidence only.

The full cloud evidence archives are tracked under `paper/evidence/` in this
private repository. They include source patches and raw internal benchmark
artifacts. The website publishes only the reviewed PDF, with the limitations
and evidence digests stated in the paper. From a fresh clone, extract the
baseline archive into `benchmark-results/cloud-evidence-20261008/` and the
follow-up archive into `benchmark-results/cloud-rebenchmark-20261008/full/`
before running the analyzers below.

## Completed October 8, 2026 run

The validated engine run is
benchmark-results/20261008T221849Z-940f3a48fe41; the HTTP and sustained
run is benchmark-results/20261008T225316Z-940f3a48fe41. Each has three
repetitions. The engine and HTTP source-diff SHA-256 values are, respectively,
5606bc8d2f07fcd9e41051c27dfbf467ee0074bf43aceabb1baebb7cbd62e0ed
and 811dc4740aaaa0a4d426b593b89d59ba4983514af5db92063e27ef308251b391.
The complete private archive is
`paper/evidence/dippi-benchmark-evidence-20261008.tar.gz` and has SHA-256
1bc46eff4f8a36e0d2e1887ce39a7ea4ad85ac71a451da16047b9f74166a2dff.
The remote and local hashes matched before the Runpod pod was stopped.

    mkdir -p benchmark-results/cloud-evidence-20261008
    tar -xzf paper/evidence/dippi-benchmark-evidence-20261008.tar.gz \
      -C benchmark-results/cloud-evidence-20261008

The summary is generated from the raw archived records by running:

    python3 paper/analyze_benchmarks.py \
      --engine-run benchmark-results/cloud-evidence-20261008/benchmark-results/20261008T221849Z-940f3a48fe41 \
      --http-run benchmark-results/cloud-evidence-20261008/benchmark-results/20261008T225316Z-940f3a48fe41 \
      > paper/benchmark-summary.json

The rented Runpod cpu3c container had 16 available vCPUs and a 32 GB memory
limit on an AMD EPYC 9754 host. The pod used no GPU. The paper separates
validated in-process operation estimates, successful complete-body HTTP
latencies, and sustained response-header latencies. The archive contains raw
attempts and five-second buckets; rejected requests remain in failure
denominators. The rendered report is `paper/dippi-technical-report.pdf`, with
a published copy at https://aryasomu.com/papers/dippi-technical-report.pdf.

## Financial-state regression and follow-up

`engine/tests/sharding_tests.rs::diagnostic_idle_shard_must_not_change_other_users_balance`
reproduced a failure on the measured baseline. After Alice reserved 100 USDC
units in each of two markets, an unrelated order on the first market changed
her shared available/locked state from 800/200 back to 900/100. The test now
passes in the current source. The fix keeps each shard's
balance and metadata map limited to keys touched in the current batch and
reads untouched keys from the shared snapshot. Additional tests cover
duplicate nonces across markets and cancellation from a snapshot balance.
The full engine test suite and all 181 API library tests passed on the source
matching the benchmark archive; `cargo check -p api` also passed. These local
checks do not cover a hosted PostgreSQL commit or deployed custody.

The revised paper retains the baseline failure and reports the follow-up
regression result and separate CPU measurements. The repair addresses the
named reproduction; it is not a financial audit or a production service-level
qualification. The five-minute follow-up harness still recorded failures but
did not preserve their HTTP statuses or response bodies, so their causes remain
unclassified.

Across three validated engine repetitions, the follow-up median was 1,185,063
in-process operations/s versus 1,244,755 in the baseline. The HTTP scenarios
improved under the stated loopback fixture, while the five-minute follow-up
still had 10,412 failed attempts in 148,986. The paper reports both results
with their different timing boundaries and limitations.

## Reproduce the follow-up CPU campaign

The private source archive is the
`dippi-rebench-rust-source-20261008.tar.gz` member of the follow-up evidence
archive, SHA-256
`d5b52c78bce4c760d4b2f43fd333ab39ebd99ef13ca95ef1c7faa41fe7d43f14`.
It contains only the Rust workspace files needed for this
campaign, not credentials or website files. Extract it into an empty directory
and run the suites separately so their timing boundaries stay distinct:

```sh
mkdir -p benchmark-results/cloud-rebenchmark-20261008/full
tar -xzf paper/evidence/dippi-rebenchmark-evidence-20261008.tar.gz \
  -C benchmark-results/cloud-rebenchmark-20261008/full
mkdir -p /tmp/dippi-rebench-source
tar -xzf benchmark-results/cloud-rebenchmark-20261008/full/dippi-rebench-rust-source-20261008.tar.gz \
  -C /tmp/dippi-rebench-source
cd /tmp/dippi-rebench-source
export BENCH_SOURCE_ARCHIVE_SHA256=d5b52c78bce4c760d4b2f43fd333ab39ebd99ef13ca95ef1c7faa41fe7d43f14
export BENCH_BASE_GIT_HEAD=940f3a48fe413fa83ff997fa3d6a770ac9488df8
export BENCH_DISPATCH_TIMEOUT_MS=5000
BENCH_REPEATS=1 bash scripts/run_benchmarks.sh e2e_http
BENCH_REPEATS=2 bash scripts/run_benchmarks.sh e2e_http
BENCH_REPEATS=3 bash scripts/run_benchmarks.sh sustained_throughput
BENCH_REPEATS=3 bash scripts/run_benchmarks.sh engine matching
```

The recorded HTTP campaign used one initial single-repeat invocation and one
two-repeat invocation, both with identical source and settings. The exploratory
`matching` suite is retained in the archive but not used for headline capacity
claims. Use `paper/analyze_rebenchmark.py` with the engine directory, both HTTP
directories, and the sustained directory to reproduce the comparison JSON.

The follow-up run IDs are `20261009T000155Z-d5b52c78bce4` (one HTTP repeat),
`20261009T001108Z-d5b52c78bce4` (two HTTP repeats),
`20261009T002116Z-d5b52c78bce4` (three sustained repeats), and
`20261009T003751Z-d5b52c78bce4` (three engine and matching repeats).
The complete private evidence archive is
`paper/evidence/dippi-rebenchmark-evidence-20261008.tar.gz`, SHA-256
`aea66214a43352c8a1e9c2fe767f3699a1a59650ff04a997cec92a4d1147e68b`.
The downloaded archive matched the pod digest before the pod was stopped.
The legacy matching sweep contains the same 30 named Criterion groups in each
of three repetitions.
After extracting the private evidence archive into
`benchmark-results/cloud-rebenchmark-20261008/full`, regenerate the summary
with:

```sh
root=benchmark-results/cloud-rebenchmark-20261008/full/dippi-rebench/benchmark-results
python3 paper/analyze_rebenchmark.py \
  --engine-run "$root/20261009T003751Z-d5b52c78bce4" \
  --http-run "$root/20261009T000155Z-d5b52c78bce4" \
  --http-run "$root/20261009T001108Z-d5b52c78bce4" \
  --sustained-run "$root/20261009T002116Z-d5b52c78bce4" \
  > paper/rebenchmark-summary.json
```

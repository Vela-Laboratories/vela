#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

repeats="${BENCH_REPEATS:-1}"
if [[ ! "$repeats" =~ ^[1-9][0-9]*$ ]]; then
  echo "BENCH_REPEATS must be a positive integer" >&2
  exit 2
fi
dispatch_timeout_ms="${BENCH_DISPATCH_TIMEOUT_MS:-5000}"
if [[ ! "$dispatch_timeout_ms" =~ ^[1-9][0-9]*$ ]]; then
  echo "BENCH_DISPATCH_TIMEOUT_MS must be a positive integer" >&2
  exit 2
fi

if (( $# == 0 )); then
  suites=(engine matching e2e_http sustained_throughput)
else
  suites=("$@")
fi
for suite in "${suites[@]}"; do
  case "$suite" in
    engine|matching|e2e_http|sustained_throughput) ;;
    *) echo "Unknown benchmark suite: $suite" >&2; exit 2 ;;
  esac
done

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  source_ref="$(git rev-parse HEAD)"
  source_status="$(git status --short)"
  source_diff_sha="$(git diff --binary HEAD -- engine/benches benches scripts | openssl dgst -sha256)"
else
  source_ref="${BENCH_SOURCE_ARCHIVE_SHA256:?Set BENCH_SOURCE_ARCHIVE_SHA256 for an extracted source archive}"
  source_status="extracted source archive"
  source_diff_sha="not applicable to source archive"
fi
run_id="$(date -u +%Y%m%dT%H%M%SZ)-${source_ref:0:12}"
run_dir="${BENCH_OUTPUT_DIR:-$repo_root/benchmark-results/$run_id}"
mkdir -p "$(dirname "$run_dir")"
mkdir "$run_dir"
run_dir="$(cd "$run_dir" && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

cpu_model="unknown"
if [[ "$(uname -s)" == "Darwin" ]]; then
  cpu_model="$(sysctl -n machdep.cpu.brand_string)"
  memory_bytes="$(sysctl -n hw.memsize)"
  logical_cpus="$(sysctl -n hw.logicalcpu)"
  host_logical_cpus="$logical_cpus"
else
  cpu_model="$(awk -F: '/^model name/{sub(/^[ \t]+/, "", $2); print $2; exit}' /proc/cpuinfo)"
  memory_bytes="$(awk '/MemTotal:/{print $2 * 1024; exit}' /proc/meminfo)"
  logical_cpus="$(nproc)"
  host_logical_cpus="$(getconf _NPROCESSORS_ONLN)"
fi

cpu_quota="unavailable"
memory_limit="unavailable"
if [[ -f /sys/fs/cgroup/cpu.max ]]; then
  cpu_quota="$(cat /sys/fs/cgroup/cpu.max)"
elif [[ -f /sys/fs/cgroup/cpu/cpu.cfs_quota_us ]]; then
  cpu_quota="$(cat /sys/fs/cgroup/cpu/cpu.cfs_quota_us)/$(cat /sys/fs/cgroup/cpu/cpu.cfs_period_us)"
fi
if [[ -f /sys/fs/cgroup/memory.max ]]; then
  memory_limit="$(cat /sys/fs/cgroup/memory.max)"
elif [[ -f /sys/fs/cgroup/memory/memory.limit_in_bytes ]]; then
  memory_limit="$(cat /sys/fs/cgroup/memory/memory.limit_in_bytes)"
fi

{
  echo "UTC start: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "Source ref: $source_ref"
  echo "Source status: $source_status"
  echo "Benchmark-source diff SHA-256: $source_diff_sha"
  echo "Base Git HEAD: ${BENCH_BASE_GIT_HEAD:-not recorded}"
  echo "OS: $(uname -srm)"
  echo "CPU: $cpu_model"
  echo "Available logical CPUs: $logical_cpus"
  echo "Host logical CPUs: $host_logical_cpus"
  echo "Host memory bytes: $memory_bytes"
  echo "Cgroup CPU quota: $cpu_quota"
  echo "Cgroup memory limit bytes: $memory_limit"
  echo "Rental allocation: ${BENCH_RENTAL_ALLOCATION:-none}"
  echo "Rust: $(rustc --version)"
  echo "Cargo: $(cargo --version)"
  echo "Repeats: $repeats"
  echo "Suites: ${suites[*]}"
  echo "Build profile: bench (fat LTO, one codegen unit)"
  echo "Loopback HTTP: 127.0.0.1"
  echo "HTTP dispatch timeout ms: $dispatch_timeout_ms"
  echo "HTTP admin token: ephemeral per suite, never saved"
} > "$run_dir/metadata.txt"

echo "Benchmark evidence: $run_dir"
for ((repeat = 1; repeat <= repeats; repeat++)); do
  repeat_dir="$run_dir/repeat-$repeat"
  mkdir -p "$repeat_dir"
  for suite in "${suites[@]}"; do
    echo "Running $suite, repetition $repeat/$repeats"
    marker="$repeat_dir/$suite-start.marker"
    touch "$marker"
    case "$suite" in
      engine)
        cargo bench -p engine --bench matching_engine_bench --locked 2>&1 | tee "$repeat_dir/engine.log"
        ;;
      matching)
        cargo bench -p benches --bench matching --locked 2>&1 | tee "$repeat_dir/matching.log"
        ;;
      e2e_http|sustained_throughput)
        ADMIN_TOKEN="$(openssl rand -hex 32)" VELA_DISPATCH_TIMEOUT_MS="$dispatch_timeout_ms" \
          VELA_BENCH_RESULTS_DIR="$repeat_dir" \
          cargo bench -p benches --bench "$suite" --locked 2>&1 | tee "$repeat_dir/$suite.log"
        ;;
    esac
    if [[ "$suite" == "engine" || "$suite" == "matching" ]] && [[ -d "$target_dir/criterion" ]]; then
      python3 - "$target_dir/criterion" "$marker" "$repeat_dir/$suite-criterion" <<'PY'
from pathlib import Path
import shutil
import sys

source, marker, destination = map(Path, sys.argv[1:])
cutoff = marker.stat().st_mtime_ns
for path in source.rglob("*"):
    if path.is_file() and path.stat().st_mtime_ns >= cutoff:
        target = destination / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
PY
    fi
  done
done
echo "Saved benchmark logs and artifacts in $run_dir"

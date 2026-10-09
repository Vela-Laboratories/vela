#!/usr/bin/env python3
"""Summarize archived Dippi benchmark runs without pooling timing boundaries."""

from __future__ import annotations

import argparse
import csv
import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import Any


def percentile(sorted_values: list[int], fraction: float) -> int | None:
    if not sorted_values:
        return None
    index = max(0, math.ceil(fraction * len(sorted_values)) - 1)
    return sorted_values[index]


def engine_run(root: Path) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    for repeat in sorted(root.glob("repeat-*")):
        estimates = (
            repeat
            / "engine-criterion"
            / "realistic_mm_workload"
            / "realistic_mm_workload"
            / "new"
            / "estimates.json"
        )
        if not estimates.exists():
            continue
        data = json.loads(estimates.read_text())
        mean_ns_per_10000 = data["mean"]["point_estimate"]
        rows.append(
            {
                "repeat": repeat.name,
                "criterion_mean_ns_per_10000": mean_ns_per_10000,
                "mean_ns_per_operation": mean_ns_per_10000 / 10_000,
                "in_process_operations_per_second": 10_000 * 1e9 / mean_ns_per_10000,
                "criterion_mean_95pct_ci_ns_per_10000": data["mean"][
                    "confidence_interval"
                ],
            }
        )
    rates = sorted(row["in_process_operations_per_second"] for row in rows)
    matching_groups = [
        sum(1 for _ in (repeat / "matching-criterion").rglob("new/estimates.json"))
        for repeat in sorted(root.glob("repeat-*"))
    ]
    return {
        "metadata": (root / "metadata.txt").read_text() if (root / "metadata.txt").exists() else None,
        "validated_realistic_mm": rows,
        "rate_median_across_repeats": percentile(rates, 0.5),
        "rate_range_across_repeats": [rates[0], rates[-1]] if rates else None,
        "exploratory_matching_group_counts": matching_groups,
    }


def http_scenario(path: Path) -> dict[str, Any]:
    with path.open(newline="") as handle:
        rows = [
            (int(row["latency_ns"]), int(row["http_status"]))
            for row in csv.DictReader(handle)
        ]
    successful = sorted(latency for latency, status in rows if 200 <= status < 300)
    counts = Counter(status for _, status in rows)
    return {
        "attempts": len(rows),
        "successes": len(successful),
        "failures": len(rows) - len(successful),
        "success_fraction": len(successful) / len(rows) if rows else None,
        "status_counts": dict(sorted(counts.items())),
        "success_latency_ns_p50": percentile(successful, 0.5),
        "success_latency_ns_p99": percentile(successful, 0.99),
        "success_latency_ns_p999": percentile(successful, 0.999),
    }


def sustained_run(repeat: Path) -> dict[str, Any]:
    log = (repeat / "sustained_throughput.log").read_text()

    def field(label: str) -> int | None:
        match = re.search(rf"^{re.escape(label)}:\s+(\d+)", log, re.MULTILINE)
        return int(match.group(1)) if match else None

    with (repeat / "sustained_buckets.csv").open(newline="") as handle:
        buckets = [
            int(row["successful_operations"])
            for row in csv.DictReader(handle)
        ]
    with (repeat / "sustained_http_latency_ns.csv").open(newline="") as handle:
        latencies = sorted(int(row["latency_ns"]) for row in csv.DictReader(handle))
    successes = field("Successful HTTP operations")
    return {
        "attempts": field("Attempted HTTP operations"),
        "successes": successes,
        "failures": field("Failed HTTP operations"),
        "fills": field("Total fills"),
        "bucket_count": len(buckets),
        "bucket_success_sum": sum(buckets),
        "buckets_match_successes": successes == sum(buckets),
        "zero_throughput_buckets": sum(value == 0 for value in buckets),
        "mean_successful_operations_per_second": sum(buckets) / 300,
        "minimum_5s_bucket_operations_per_second": min(buckets) / 5 if buckets else None,
        "maximum_5s_bucket_operations_per_second": max(buckets) / 5 if buckets else None,
        "success_header_latency_ns_p50": percentile(latencies, 0.5),
        "success_header_latency_ns_p99": percentile(latencies, 0.99),
        "rss_mb_t0": field("Memory at t=0"),
        "rss_mb_t150": field("Memory at t=150s"),
        "rss_mb_t300": field("Memory at t=300s"),
    }


def http_run(root: Path) -> dict[str, Any]:
    repeats: list[dict[str, Any]] = []
    for repeat in sorted(root.glob("repeat-*")):
        scenarios = {
            path.stem.removeprefix("e2e_http_").removesuffix("_outcomes"): http_scenario(path)
            for path in sorted(repeat.glob("e2e_http_*_outcomes.csv"))
        }
        result: dict[str, Any] = {"repeat": repeat.name, "http": scenarios}
        if (repeat / "sustained_buckets.csv").exists():
            result["sustained"] = sustained_run(repeat)
        repeats.append(result)
    return {
        "metadata": (root / "metadata.txt").read_text() if (root / "metadata.txt").exists() else None,
        "repeats": repeats,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine-run", type=Path)
    parser.add_argument("--http-run", type=Path)
    args = parser.parse_args()
    if not args.engine_run and not args.http_run:
        parser.error("provide --engine-run or --http-run")
    result: dict[str, Any] = {}
    if args.engine_run:
        result["engine"] = engine_run(args.engine_run)
    if args.http_run:
        result["http"] = http_run(args.http_run)
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()

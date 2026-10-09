#!/usr/bin/env python3
"""Combine a fixed-source campaign whose suites ran in separate directories."""

from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any

from analyze_benchmarks import engine_run, http_run, sustained_run


def summarize_http(repeats: list[dict[str, Any]]) -> dict[str, Any]:
    names = sorted({name for repeat in repeats for name in repeat["http"]})
    result: dict[str, Any] = {}
    for name in names:
        scenarios = [repeat["http"][name] for repeat in repeats if name in repeat["http"]]
        result[name] = {
            "repetitions": len(scenarios),
            "attempts": sum(row["attempts"] for row in scenarios),
            "successes": sum(row["successes"] for row in scenarios),
            "failures": sum(row["failures"] for row in scenarios),
            "median_repeat_p50_ns": statistics.median(
                row["success_latency_ns_p50"] for row in scenarios
            ),
            "median_repeat_p99_ns": statistics.median(
                row["success_latency_ns_p99"] for row in scenarios
            ),
            "repeat_p50_range_ns": [
                min(row["success_latency_ns_p50"] for row in scenarios),
                max(row["success_latency_ns_p50"] for row in scenarios),
            ],
            "repeat_p99_range_ns": [
                min(row["success_latency_ns_p99"] for row in scenarios),
                max(row["success_latency_ns_p99"] for row in scenarios),
            ],
        }
    return result


def summarize_sustained(repeats: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "repetitions": len(repeats),
        "attempts": sum(row["attempts"] for row in repeats),
        "successes": sum(row["successes"] for row in repeats),
        "failures": sum(row["failures"] for row in repeats),
        "fills": sum(row["fills"] for row in repeats),
        "success_rate_range_per_second": [
            min(row["mean_successful_operations_per_second"] for row in repeats),
            max(row["mean_successful_operations_per_second"] for row in repeats),
        ],
        "end_rss_mb_range": [
            min(row["rss_mb_t300"] for row in repeats),
            max(row["rss_mb_t300"] for row in repeats),
        ],
        "all_bucket_sums_match": all(row["buckets_match_successes"] for row in repeats),
        "zero_throughput_buckets": sum(row["zero_throughput_buckets"] for row in repeats),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine-run", type=Path, required=True)
    parser.add_argument("--http-run", type=Path, action="append", required=True)
    parser.add_argument("--sustained-run", type=Path, required=True)
    args = parser.parse_args()

    http_repeats: list[dict[str, Any]] = []
    for root in args.http_run:
        run = http_run(root)
        for repeat in run["repeats"]:
            if repeat["http"]:
                http_repeats.append({"source_run": str(root), **repeat})

    sustained_repeats = []
    for repeat in sorted(args.sustained_run.glob("repeat-*")):
        if (repeat / "sustained_buckets.csv").exists():
            sustained_repeats.append(
                {"source_repeat": str(repeat), **sustained_run(repeat)}
            )

    roots = [*args.http_run, args.sustained_run, args.engine_run]
    metadata = {str(root): (root / "metadata.txt").read_text() for root in roots}
    source_refs = {
        next(
            line.removeprefix("Source ref: ")
            for line in record.splitlines()
            if line.startswith("Source ref: ")
        )
        for record in metadata.values()
    }
    if len(source_refs) != 1:
        raise ValueError(f"Campaign directories have different source refs: {source_refs}")
    source_ref = source_refs.pop()
    engine = engine_run(args.engine_run)
    if not engine["validated_realistic_mm"] or not http_repeats or not sustained_repeats:
        raise ValueError("Engine, HTTP, and sustained evidence must all be present")
    result = {
        "source_archive_sha256": source_ref,
        "engine": engine,
        "http_repeats": http_repeats,
        "http_summary": summarize_http(http_repeats),
        "sustained_repeats": sustained_repeats,
        "sustained_summary": summarize_sustained(sustained_repeats),
        "run_metadata": metadata,
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Plot archived five-second successful-operation buckets, one line per repeat."""

from __future__ import annotations

import argparse
import csv
from pathlib import Path

import matplotlib.pyplot as plt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    paths = sorted(args.run_dir.glob("repeat-*/sustained_buckets.csv"))
    if not paths:
        parser.error("no sustained bucket files found")

    fig, ax = plt.subplots(figsize=(7.2, 3.1), constrained_layout=True)
    colors = ["#226c7a", "#b45c39", "#73649b"]
    for index, path in enumerate(paths):
        with path.open(newline="") as handle:
            rows = list(csv.DictReader(handle))
        if len(rows) != 60:
            parser.error(f"{path}: expected 60 buckets, found {len(rows)}")
        values = [int(row["successful_operations"]) / 5 for row in rows]
        centers = [(bucket + 0.5) * 5 for bucket in range(len(values))]
        ax.plot(
            centers,
            values,
            color=colors[index % len(colors)],
            linewidth=1.3,
            alpha=0.85,
            label=f"Repeat {index + 1}",
        )

    ax.set(xlim=(0, 300), ylim=(0, None), xlabel="Elapsed seconds", ylabel="Successful HTTP operations/s")
    ax.grid(axis="y", color="#d8dde0", linewidth=0.6)
    ax.legend(frameon=False, fontsize=8, loc="upper right")
    for spine in ("top", "right"):
        ax.spines[spine].set_visible(False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(args.output, dpi=180)
    plt.close(fig)


if __name__ == "__main__":
    main()

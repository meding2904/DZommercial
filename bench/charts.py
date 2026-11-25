#!/usr/bin/env python3
"""Render the benchmark charts from the bench documents.

Usage:
    python3 bench/charts.py [--baseline bench/baseline.json] [--after bench/after.json]
                            [--k6-cpu bench/k6-cpu.json]

Writes PNGs to docs/charts/. Uses the storefront's shipped palette so the charts
look like they belong to the project rather than a default plotting theme.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter

ROOT = Path(__file__).resolve().parents[1]
INK = "#2c2325"
MUTED = "#6a5d5e"
BEFORE = "#8d7f7a"
GROUND = "#f6f0eb"
ACCENT = "#9a3c4c"
PANEL = "#ebe0d9"

plt.rcParams.update(
    {
        "font.family": "DejaVu Sans",
        "text.color": INK,
        "axes.labelcolor": INK,
        "xtick.color": MUTED,
        "ytick.color": MUTED,
        "axes.edgecolor": PANEL,
        "figure.facecolor": GROUND,
        "axes.facecolor": GROUND,
        "savefig.facecolor": GROUND,
        "axes.spines.top": False,
        "axes.spines.right": False,
        "axes.grid": False,
    }
)


def load(path: Path) -> dict:
    document = json.loads(path.read_text())
    if "load" not in document:
        raise SystemExit(f"{path} is not a bench document")
    return document


def bar_labels(ax, bars, values, suffix="", decimals=0):
    for bar, value in zip(bars, values):
        text = f"{value:,.{decimals}f}{suffix}"
        ax.annotate(
            text,
            xy=(bar.get_x() + bar.get_width() / 2, bar.get_height()),
            xytext=(0, 6),
            textcoords="offset points",
            ha="center",
            va="bottom",
            fontsize=9,
            color=INK,
        )


def latency_chart(before: dict, after: dict, out: Path) -> None:
    before_read = before["load"]["read_mix"]["latency_ms"]
    after_read = after["load"]["read_mix"]["latency_ms"]
    before_login = before["load"]["login_flow"]["latency_ms"]
    after_login = after["load"]["login_flow"]["latency_ms"]

    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.6), dpi=160)
    metrics = ["p50", "p95", "p99"]
    width = 0.36
    positions = range(len(metrics))
    left_values = [before_read[m] for m in metrics]
    right_values = [after_read[m] for m in metrics]
    left_before = left.bar(
        [p - width / 2 for p in positions], left_values, width, color=MUTED, label="Go oracle"
    )
    left_after = left.bar(
        [p + width / 2 for p in positions], right_values, width, color=ACCENT, label="Rust"
    )
    left.set_xticks(list(positions), [m.upper() for m in metrics])
    left.set_ylabel("milliseconds")
    left.set_title("Public read mix, latency", loc="left", fontsize=13, color=INK, pad=12)
    bar_labels(left, left_before, left_values, " ms", 2)
    bar_labels(left, left_after, right_values, " ms", 2)
    left.set_ylim(0, max(left_values + right_values + [1]) * 1.25)

    positions = range(len(metrics))
    left_values = [before_login[m] for m in metrics]
    right_values = [after_login[m] for m in metrics]
    bars_before = right.bar(
        [p - width / 2 for p in positions], left_values, width, color=MUTED, label="Go oracle"
    )
    bars_after = right.bar(
        [p + width / 2 for p in positions], right_values, width, color=ACCENT, label="Rust"
    )
    right.set_xticks(list(positions), [m.upper() for m in metrics])
    right.set_ylabel("milliseconds")
    right.set_title("Login flow, latency", loc="left", fontsize=13, color=INK, pad=12)
    bar_labels(right, bars_before, left_values, " ms", 1)
    bar_labels(right, bars_after, right_values, " ms", 1)
    right.set_ylim(0, max(left_values + right_values + [1]) * 1.25)

    figure.legend(
        [left_before, left_after],
        ["Go oracle", "Rust"],
        loc="upper center",
        ncol=2,
        frameon=False,
        bbox_to_anchor=(0.5, 0.02),
    )
    figure.suptitle("Latency before and after the Rust port", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0.05, 1, 0.94))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def throughput_chart(before: dict, after: dict, out: Path) -> None:
    before_rps = before["load"]["read_mix"]["throughput_rps"]
    after_rps = after["load"]["read_mix"]["throughput_rps"]
    before_login = before["load"]["login_flow"]["throughput_rps"]
    after_login = after["load"]["login_flow"]["throughput_rps"]

    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.4), dpi=160)
    for axis, values, title, suffix in (
        (left, [before_rps, after_rps], "Read mix throughput", " req/s"),
        (right, [before_login, after_login], "Login throughput", " logins/s"),
    ):
        bars = axis.bar(["Go oracle", "Rust"], values, width=0.5, color=[MUTED, ACCENT])
        bar_labels(axis, bars, values, suffix, 1)
        axis.set_title(title, loc="left", fontsize=13, color=INK, pad=12)
        axis.set_ylim(0, max(values + [1]) * 1.25)
        axis.yaxis.set_major_formatter(FuncFormatter(lambda value, _: f"{value:,.0f}"))
    figure.suptitle("Throughput before and after the Rust port", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def memory_chart(before: dict, after: dict, out: Path) -> None:
    metrics = [
        ("Reads only", "peak_rss_after_reads_mb"),
        ("Reads + logins", "peak_rss_mb"),
    ]
    before_values = [before["server"][key] for _, key in metrics]
    after_values = [after["server"][key] for _, key in metrics]
    positions = range(len(metrics))
    width = 0.36
    figure, axis = plt.subplots(figsize=(7.2, 4.4), dpi=160)
    bars_before = axis.bar(
        [p - width / 2 for p in positions], before_values, width, color=MUTED, label="Go oracle"
    )
    bars_after = axis.bar(
        [p + width / 2 for p in positions], after_values, width, color=ACCENT, label="Rust"
    )
    axis.set_xticks(list(positions), [label for label, _ in metrics])
    axis.set_ylabel("peak resident memory, MiB")
    axis.set_title("Peak memory", loc="left", fontsize=13, color=INK, pad=12)
    bar_labels(axis, bars_before, before_values, " MiB", 1)
    bar_labels(axis, bars_after, after_values, " MiB", 1)
    axis.set_ylim(0, max(before_values + after_values + [1]) * 1.25)
    axis.legend(frameon=False, loc="upper left")
    figure.suptitle("Memory before and after the Rust port", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def cpu_chart(k6: dict, out: Path) -> None:
    """Server CPU per request from the k6 keep-alive runs."""
    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.4), dpi=160)
    panels = (
        (left, k6["read"], "Public read mix", [BEFORE, MUTED, ACCENT]),
        (right, k6["churn"], "20,000 rotated addresses", [BEFORE, ACCENT]),
    )
    for axis, runs, title, colors in panels:
        labels = [run["label"] for run in runs]
        values = [run["cpu_ms_per_request"] * 1000 for run in runs]
        bars = axis.bar(labels, values, width=0.5, color=colors)
        bar_labels(axis, bars, values, " µs", 1)
        axis.set_title(title, loc="left", fontsize=13, color=INK, pad=12)
        axis.set_ylabel("server CPU, microseconds per request")
        axis.set_ylim(0, max(values + [1]) * 1.25)
        axis.tick_params(axis="x", labelsize=9)

    figure.suptitle(
        "Server CPU per read request, k6 keep-alive probe",
        x=0.02,
        ha="left",
        fontsize=15,
    )
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def size_chart(before: dict, after: dict, out: Path) -> None:
    before_bytes = before["binary"]["bytes"]
    after_bytes = after["binary"]["bytes"]
    figure, axis = plt.subplots(figsize=(7.2, 4.2), dpi=160)
    bars = axis.bar(
        ["Go oracle", "Rust"],
        [before_bytes / (1024 * 1024), after_bytes / (1024 * 1024)],
        width=0.5,
        color=[MUTED, ACCENT],
    )
    bar_labels(
        axis,
        bars,
        [before_bytes / (1024 * 1024), after_bytes / (1024 * 1024)],
        " MiB",
        2,
    )
    axis.set_ylabel("release binary, MiB")
    axis.set_title("Binary size", loc="left", fontsize=13, color=INK, pad=12)
    axis.set_ylim(0, max(before_bytes, after_bytes) / (1024 * 1024) * 1.25)
    figure.suptitle("Binary size before and after the Rust port", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", default=str(ROOT / "bench/baseline.json"))
    parser.add_argument("--after", default=str(ROOT / "bench/after.json"))
    parser.add_argument("--k6-cpu", default=str(ROOT / "bench/k6-cpu.json"))
    parser.add_argument("--out", default=str(ROOT / "docs/charts"))
    args = parser.parse_args()

    before = load(Path(args.baseline))
    after = load(Path(args.after))
    k6 = json.loads(Path(args.k6_cpu).read_text())
    destination = Path(args.out)
    destination.mkdir(parents=True, exist_ok=True)

    latency_chart(before, after, destination / "latency.png")
    throughput_chart(before, after, destination / "throughput.png")
    memory_chart(before, after, destination / "memory.png")
    size_chart(before, after, destination / "binary-size.png")
    cpu_chart(k6, destination / "cpu-per-request.png")
    print(f"wrote 5 charts to {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

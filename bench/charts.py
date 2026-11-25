#!/usr/bin/env python3
"""Render the k6 benchmark charts from bench/k6-results.json.

Usage:
    python3 bench/charts.py [--results bench/k6-results.json] [--out docs/charts]

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
    if "read" not in document:
        raise SystemExit(f"{path} is not a k6 results document")
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
            fontsize=8,
            color=INK,
        )


def latency_chart(results: dict, out: Path) -> None:
    read = results["read"]
    login = results["login"]
    metrics = [("p50", "P50"), ("p95", "P95"), ("p99", "P99")]
    colors = [MUTED, BEFORE, ACCENT]

    figure, (left, right) = plt.subplots(1, 2, figsize=(11, 4.6), dpi=160)

    width = 0.26
    positions = range(len(metrics))
    for index, run in enumerate(read):
        values = [run["latency_ms"][key] for key, _ in metrics]
        offsets = [position + (index - 1) * width for position in positions]
        left.bar(offsets, values, width, color=colors[index], label=run["label"])
    left.set_xticks(list(positions), [label for _, label in metrics])
    left.set_ylabel("milliseconds")
    left.set_title("Read mix, latency", loc="left", fontsize=13, color=INK, pad=12)
    left.set_ylim(0, max(run["latency_ms"][key] for run in read for key, _ in metrics) * 1.25)
    left.legend(frameon=False, loc="upper left", fontsize=8)

    width = 0.36
    positions = range(len(metrics))
    for index, run in enumerate(login):
        values = [run["flow_ms"][key] for key, _ in metrics]
        offsets = [position + (index - 0.5) * width for position in positions]
        right.bar(
            offsets,
            values,
            width,
            color=[MUTED, ACCENT][index],
            label=run["label"],
        )
    right.set_xticks(list(positions), [label for _, label in metrics])
    right.set_ylabel("milliseconds")
    right.set_title("Login flow, latency", loc="left", fontsize=13, color=INK, pad=12)
    right.set_ylim(0, max(run["flow_ms"][key] for run in login for key, _ in metrics) * 1.25)

    figure.suptitle("k6 keep-alive latency", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.93))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def throughput_chart(results: dict, out: Path) -> None:
    read = results["read"]
    login = results["login"]
    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.4), dpi=160)

    colors = [MUTED, BEFORE, ACCENT]
    values = [run["throughput_rps"] for run in read]
    bars = left.bar(
        [run["label"] for run in read],
        values,
        width=0.5,
        color=colors,
    )
    bar_labels(left, bars, values, " req/s", 0)
    left.set_title("Read mix throughput", loc="left", fontsize=13, color=INK, pad=12)
    left.set_ylim(0, max(values + [1]) * 1.25)
    left.yaxis.set_major_formatter(FuncFormatter(lambda value, _: f"{value:,.0f}"))

    values = [run["throughput_logins_per_s"] for run in login]
    bars = right.bar(
        [run["label"] for run in login],
        values,
        width=0.5,
        color=[MUTED, ACCENT],
    )
    bar_labels(right, bars, values, " logins/s", 2)
    right.set_title("Login throughput", loc="left", fontsize=13, color=INK, pad=12)
    right.set_ylim(0, max(values + [1]) * 1.25)

    figure.suptitle("k6 keep-alive throughput", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def cpu_chart(results: dict, out: Path) -> None:
    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.4), dpi=160)
    panels = (
        (left, results["read"], "Public read mix", [MUTED, BEFORE, ACCENT]),
        (right, results["churn"], "20,000 rotated addresses", [BEFORE, ACCENT]),
    )
    for axis, runs, title, colors in panels:
        values = [run["cpu_ms_per_request"] * 1000 for run in runs]
        bars = axis.bar([run["label"] for run in runs], values, width=0.5, color=colors)
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


def capacity_chart(results: dict, out: Path) -> None:
    capacity = results["capacity"]
    figure, (left, right) = plt.subplots(1, 2, figsize=(10.5, 4.4), dpi=160)
    groups = (
        ("9,999 addresses", capacity["read"]),
        ("200,000 addresses", capacity["churn"]),
    )
    positions = range(len(groups))
    width = 0.34
    labels = ["Go oracle", "Rust after"]
    legend_bars = []
    for index in range(2):
        values = [runs[index]["throughput_rps"] for _, runs in groups]
        offsets = [position + (index - 0.5) * width for position in positions]
        bars = left.bar(
            offsets,
            values,
            width,
            color=[MUTED, ACCENT][index],
            label=labels[index],
        )
        legend_bars.append(bars)
        bar_labels(left, bars, values, " req/s", 0)
    left.set_xticks(list(positions), [label for label, _ in groups])
    left.set_ylabel("requests per second")
    left.set_title("Throughput", loc="left", fontsize=13, color=INK, pad=12)
    left.set_ylim(0, max(run["throughput_rps"] for _, runs in groups for run in runs) * 1.25)
    left.yaxis.set_major_formatter(FuncFormatter(lambda value, _: f"{value:,.0f}"))

    for index in range(2):
        values = [runs[index]["cpu_ms_per_request"] * 1000 for _, runs in groups]
        offsets = [position + (index - 0.5) * width for position in positions]
        bars = right.bar(offsets, values, width, color=[MUTED, ACCENT][index])
        bar_labels(right, bars, values, " µs", 1)
    right.set_xticks(list(positions), [label for label, _ in groups])
    right.set_ylabel("server CPU, microseconds per request")
    right.set_title("Server CPU", loc="left", fontsize=13, color=INK, pad=12)
    right.set_ylim(0, max(run["cpu_ms_per_request"] for _, runs in groups for run in runs) * 1000 * 1.25)

    figure.legend(
        legend_bars,
        labels,
        loc="upper center",
        ncol=2,
        frameon=False,
        bbox_to_anchor=(0.5, 0.02),
    )
    figure.suptitle(
        "Server capacity, 10 k6 processes on 10 client cores",
        x=0.02,
        ha="left",
        fontsize=15,
    )
    figure.tight_layout(rect=(0, 0.05, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def memory_chart(results: dict, out: Path) -> None:
    footprint = results["footprint"]
    metrics = [
        ("Reads only", "peak_rss_after_reads_mb"),
        ("Reads + logins", "peak_rss_mb"),
    ]
    colors = [MUTED, BEFORE, ACCENT]
    positions = range(len(metrics))
    width = 0.26
    figure, axis = plt.subplots(figsize=(7.6, 4.4), dpi=160)
    for index, run in enumerate(footprint):
        values = [run[key] for _, key in metrics]
        offsets = [position + (index - 1) * width for position in positions]
        bars = axis.bar(
            offsets,
            values,
            width,
            color=colors[index],
            label=run["label"],
        )
        bar_labels(axis, bars, values, " MiB", 1)
    axis.set_xticks(list(positions), [label for label, _ in metrics])
    axis.set_ylabel("peak resident memory, MiB")
    axis.set_title("Peak memory", loc="left", fontsize=13, color=INK, pad=12)
    axis.set_ylim(
        0,
        max(run[key] for run in footprint for _, key in metrics) * 1.25,
    )
    axis.legend(frameon=False, loc="upper left", fontsize=8)
    figure.suptitle("Server memory", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def size_chart(results: dict, out: Path) -> None:
    footprint = results["footprint"]
    colors = [MUTED, BEFORE, ACCENT]
    values = [run["binary_bytes"] / 1_000_000 for run in footprint]
    figure, axis = plt.subplots(figsize=(7.6, 4.2), dpi=160)
    bars = axis.bar(
        [run["label"] for run in footprint],
        values,
        width=0.5,
        color=colors,
    )
    bar_labels(axis, bars, values, " MB", 2)
    axis.set_ylabel("release binary, MB")
    axis.set_title("Binary size", loc="left", fontsize=13, color=INK, pad=12)
    axis.set_ylim(0, max(values + [1]) * 1.25)
    figure.suptitle("Release binary", x=0.02, ha="left", fontsize=15)
    figure.tight_layout(rect=(0, 0, 1, 0.92))
    figure.savefig(out, bbox_inches="tight")
    plt.close(figure)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", default=str(ROOT / "bench/k6-results.json"))
    parser.add_argument("--out", default=str(ROOT / "docs/charts"))
    args = parser.parse_args()

    results = load(Path(args.results))
    destination = Path(args.out)
    destination.mkdir(parents=True, exist_ok=True)

    latency_chart(results, destination / "latency.png")
    throughput_chart(results, destination / "throughput.png")
    cpu_chart(results, destination / "cpu-per-request.png")
    capacity_chart(results, destination / "capacity.png")
    memory_chart(results, destination / "memory.png")
    size_chart(results, destination / "binary-size.png")
    print(f"wrote 6 charts to {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

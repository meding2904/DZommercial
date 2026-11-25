#!/usr/bin/env python3
"""Keep-alive throughput probe for the Azahar API.

`bench.py` reproduces the Go-oracle comparison with one fresh TCP connection
per request. That is useful for parity, but it measures the Python client's
connection setup as much as the server. This probe keeps a fixed number of
HTTP/1.1 connections open, so the numbers describe the server's request path:
routing, middleware, the read cache and response writing.

It records per-run throughput, latency percentiles, server CPU time per
request and peak RSS. CPU time per request is the metric to compare when the
client and server share a small machine, because it does not depend on how
fast the Python client happens to be.

Requires `aiohttp` (Python 3.10+):

    make dev-db
    cargo build --release -p azahar-api
    python3 bench/hotpath.py --bin target/release/azahar-api --label rust \
        --out bench/hotpath.json

On a two-core machine, pin the server and the client to separate cores:

    taskset -c 1 python3 bench/hotpath.py --server-cpu 0 --bin ... --label ...
"""

from __future__ import annotations

import argparse
import asyncio
import http.client
import json
import math
import os
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path

import aiohttp

READ_PATHS = [
    "/api/products",
    "/api/categories",
    "/api/plugins",
    "/api/storefront/theme",
    "/api/products/rose-petal-bath-soak",
    "/api/health",
]

CLOCK_TICKS = os.sysconf("SC_CLK_TCK")


def wait_health(port: int, process: subprocess.Popen[bytes], seconds: float = 60.0) -> float:
    started = time.perf_counter()
    deadline = started + seconds
    while time.perf_counter() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"server exited with {process.returncode}")
        try:
            conn = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            conn.request("GET", "/api/health")
            response = conn.getresponse()
            response.read()
            conn.close()
            if response.status == 200:
                return time.perf_counter() - started
        except OSError:
            pass
        time.sleep(0.05)
    raise RuntimeError("server did not become healthy")


def proc_cpu_seconds(pid: int) -> float:
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    # After the command name: state is field 3, utime/stime are fields 14/15.
    utime, stime = int(fields[11]), int(fields[12])
    return (utime + stime) / CLOCK_TICKS


def peak_rss_kb(pid: int) -> int:
    try:
        status = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return 0
    for line in status.splitlines():
        if line.startswith("VmHWM:"):
            return int(line.split()[1])
    return 0


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = max(1, math.ceil(fraction * len(ordered)))
    return ordered[min(len(ordered) - 1, rank - 1)]


def summarise(latencies: list[float]) -> dict[str, float]:
    if not latencies:
        return {"p50": 0.0, "p95": 0.0, "p99": 0.0, "mean": 0.0, "max": 0.0}
    values = [value * 1000 for value in latencies]
    return {
        "p50": round(percentile(values, 0.50), 3),
        "p95": round(percentile(values, 0.95), 3),
        "p99": round(percentile(values, 0.99), 3),
        "mean": round(statistics.fmean(values), 3),
        "max": round(max(values), 3),
    }


async def read_worker(
    session: aiohttp.ClientSession,
    port: int,
    count: int,
    ips: list[str],
    offset: int,
    stride: int,
    latencies: list[float],
    statuses: dict[int, int],
) -> None:
    local: list[float] = []
    url = f"http://127.0.0.1:{port}"
    for index in range(count):
        path = READ_PATHS[(offset + index) % len(READ_PATHS)]
        headers = {
            "Accept": "application/json",
            "X-Forwarded-For": ips[(index * stride + offset) % len(ips)],
        }
        started = time.perf_counter()
        async with session.get(url + path, headers=headers) as response:
            await response.read()
            status = response.status
        local.append(time.perf_counter() - started)
        statuses[status] = statuses.get(status, 0) + 1
    latencies.extend(local)


async def run_reads(port: int, connections: int, total: int, ip_count: int) -> dict[str, object]:
    ips = [
        f"10.{(serial >> 16) & 0xFF}.{(serial >> 8) & 0xFF}.{serial & 0xFF}"
        for serial in range(1, max(ip_count, 1) + 1)
    ]
    per_worker = [total // connections] * connections
    for index in range(total % connections):
        per_worker[index] += 1

    latencies: list[float] = []
    statuses: dict[int, int] = {}
    connector = aiohttp.TCPConnector(limit=connections, limit_per_host=connections)
    timeout = aiohttp.ClientTimeout(total=30)
    started = time.perf_counter()
    async with aiohttp.ClientSession(connector=connector, timeout=timeout) as session:
        await asyncio.gather(
            *(
                read_worker(
                    session, port, per_worker[i], ips, i, connections, latencies, statuses
                )
                for i in range(connections)
            )
        )
    elapsed = time.perf_counter() - started
    return {
        "connections": connections,
        "requests": total,
        "elapsed_seconds": round(elapsed, 3),
        "throughput_rps": round(total / elapsed, 1),
        "latency_ms": summarise(latencies),
        "status_counts": {str(key): value for key, value in sorted(statuses.items())},
        "all_succeeded": set(statuses) == {200} and sum(statuses.values()) == total,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--port", type=int, default=18081)
    parser.add_argument("--connections", type=int, default=32)
    parser.add_argument("--requests", type=int, default=12000)
    parser.add_argument("--warmup", type=int, default=1200)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--unique-ips", type=int, default=4096,
                        help="rotate this many client addresses")
    parser.add_argument("--server-cpu", type=int, default=None,
                        help="pin the server to this CPU (Linux)")
    parser.add_argument("--env", action="append", default=[], help="KEY=VALUE passed to the server")
    args = parser.parse_args()
    if args.unique_ips < 1:
        parser.error("--unique-ips must be at least 1")
    requests_per_ip = (args.warmup + args.requests * args.runs) / args.unique_ips
    if requests_per_ip >= 100:
        parser.error(
            f"{requests_per_ip:.0f} requests per address; the general limiter "
            "allows 120 per minute. Raise --unique-ips."
        )

    binary = Path(args.bin).resolve()
    env = os.environ.copy()
    env.update(
        {
            "APP_ENV": "development",
            "API_ADDR": f"127.0.0.1:{args.port}",
            "APP_ORIGIN": "http://127.0.0.1:5173",
            "TURSO_DATABASE_URL": "http://127.0.0.1:8880",
            "COOKIE_SECURE": "false",
            "SEED_DEMO": "true",
            "TRUST_PROXY": "true",
        }
    )
    for item in args.env:
        key, _, value = item.partition("=")
        env[key] = value

    preexec = None
    if args.server_cpu is not None:
        cpu = args.server_cpu

        def preexec() -> None:  # pragma: no cover - Linux only
            os.sched_setaffinity(0, {cpu})

    log = open("/tmp/azahar-hotpath-server.log", "wb")
    process = subprocess.Popen(
        [str(binary)],
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
        preexec_fn=preexec,
    )
    try:
        startup = wait_health(args.port, process)
        asyncio.run(run_reads(args.port, args.connections, args.warmup, args.unique_ips))

        runs = []
        for _ in range(args.runs):
            cpu_start = proc_cpu_seconds(process.pid)
            result = asyncio.run(
                run_reads(args.port, args.connections, args.requests, args.unique_ips)
            )
            cpu_seconds = proc_cpu_seconds(process.pid) - cpu_start
            if not result["all_succeeded"]:
                raise RuntimeError(f"read run did not return 200 for every request: {result}")
            result["server_cpu_seconds"] = round(cpu_seconds, 4)
            result["server_cpu_ms_per_request"] = round(
                cpu_seconds * 1000 / args.requests, 4
            )
            result["peak_rss_kb"] = peak_rss_kb(process.pid)
            runs.append(result)

        throughput = [run["throughput_rps"] for run in runs]
        cpu_per_request = [run["server_cpu_ms_per_request"] for run in runs]
        document = {
            "label": args.label,
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "binary": {"path": str(binary), "bytes": binary.stat().st_size},
            "server": {
                "startup_seconds": round(startup, 3),
                "pinned_cpu": args.server_cpu,
            },
            "environment": {
                "connections": args.connections,
                "requests_per_run": args.requests,
                "warmup_requests": args.warmup,
                "runs": args.runs,
                "unique_ips": args.unique_ips,
                "database": env["TURSO_DATABASE_URL"],
            },
            "runs": runs,
            "summary": {
                "throughput_rps_median": round(statistics.median(throughput), 1),
                "throughput_rps_min": round(min(throughput), 1),
                "throughput_rps_max": round(max(throughput), 1),
                "server_cpu_ms_per_request_median": round(
                    statistics.median(cpu_per_request), 4
                ),
                "server_cpu_ms_per_request_min": round(min(cpu_per_request), 4),
                "server_cpu_ms_per_request_max": round(max(cpu_per_request), 4),
                "latency_p50_ms_median": round(
                    statistics.median([run["latency_ms"]["p50"] for run in runs]), 3
                ),
                "latency_p95_ms_median": round(
                    statistics.median([run["latency_ms"]["p95"] for run in runs]), 3
                ),
                "latency_p99_ms_median": round(
                    statistics.median([run["latency_ms"]["p99"] for run in runs]), 3
                ),
            },
        }
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps(document, indent=2) + "\n")
        print(json.dumps(document["summary"], indent=2))
        return 0
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
        log.close()


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Baseline / after benchmark harness for the Azahar API.

Starts the server binary, waits for /api/health, drives a fixed read mix and a
login flow at a fixed concurrency, records latency percentiles, throughput,
peak RSS and binary size, then writes one JSON document.

The same script and the same request counts run against the Go oracle and the
Rust build, so the two documents are directly comparable.

Usage:
    python3 bench/bench.py --bin target/release/azahar-api --label rust --out bench/after.json
"""

from __future__ import annotations

import argparse
import http.client
import json
import math
import os
import random
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
READ_PATHS = [
    "/api/products",
    "/api/categories",
    "/api/plugins",
    "/api/storefront/theme",
    "/api/products/rose-petal-bath-soak",
    "/api/health",
]
DEMO = {
    "email": "client@azahar.example",
    "password": "azahar-dev-client-2026",
}


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


def request(port: int, method: str, path: str, body: bytes | None = None,
            headers: dict[str, str] | None = None) -> tuple[int, bytes, list[str]]:
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
    try:
        conn.request(method, path, body=body, headers=headers or {})
        response = conn.getresponse()
        payload = response.read()
        raw_headers = [f"{k}: {v}" for k, v in response.getheaders()]
        return response.status, payload, raw_headers
    finally:
        conn.close()


def spread_ip(rng: random.Random) -> str:
    return f"10.{rng.randrange(1, 250)}.{rng.randrange(1, 250)}.{rng.randrange(1, 250)}"


def read_worker(port: int, count: int, seed: int, latencies: list[float],
                statuses: dict[int, int], errors: list[str], lock: threading.Lock) -> None:
    rng = random.Random(seed)
    local_latency: list[float] = []
    local_status: dict[int, int] = {}
    local_errors: list[str] = []
    for _ in range(count):
        path = rng.choice(READ_PATHS)
        started = time.perf_counter()
        try:
            status, _, _ = request(
                port, "GET", path,
                headers={"Accept": "application/json", "X-Forwarded-For": spread_ip(rng)},
            )
            local_status[status] = local_status.get(status, 0) + 1
        except Exception as exc:  # noqa: BLE001 - the report needs the class of failure
            local_errors.append(repr(exc))
        local_latency.append(time.perf_counter() - started)
    with lock:
        latencies.extend(local_latency)
        for key, value in local_status.items():
            statuses[key] = statuses.get(key, 0) + value
        errors.extend(local_errors)


def percentile(values: list[float], fraction: float) -> float:
    """Nearest-rank percentile: the smallest value with rank >= fraction."""
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = max(1, math.ceil(fraction * len(ordered)))
    return ordered[min(len(ordered) - 1, rank - 1)]


def summarise_ms(latencies: list[float]) -> dict[str, float]:
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


def run_read_mix(port: int, workers: int, total: int) -> dict[str, object]:
    per_worker = [total // workers] * workers
    for index in range(total % workers):
        per_worker[index] += 1
    latencies: list[float] = []
    statuses: dict[int, int] = {}
    errors: list[str] = []
    lock = threading.Lock()
    threads = [
        threading.Thread(target=read_worker, args=(port, per_worker[i], i, latencies, statuses, errors, lock))
        for i in range(workers)
    ]
    started = time.perf_counter()
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    elapsed = time.perf_counter() - started
    return {
        "workers": workers,
        "requests": total,
        "elapsed_seconds": round(elapsed, 3),
        "throughput_rps": round(total / elapsed, 1),
        "latency_ms": summarise_ms(latencies),
        "status_counts": {str(k): v for k, v in sorted(statuses.items())},
        "errors": len(errors),
        "error_sample": errors[:3],
        "all_succeeded": not errors and set(statuses) == {200} and sum(statuses.values()) == total,
    }


def login_once(port: int, seed: int = 0) -> tuple[float, int, str]:
    started = time.perf_counter()
    client_ip = spread_ip(random.Random(seed))
    user_agent = f"azahar-bench/{seed}"
    common = {"X-Forwarded-For": client_ip, "User-Agent": user_agent}
    # The token is both set as a cookie and returned by the endpoint.
    status, payload, headers = request(port, "GET", "/api/csrf", headers=common)
    csrf_cookie = ""
    for line in headers:
        if line.lower().startswith("set-cookie:") and "azahar_csrf=" in line:
            csrf_cookie = line.split("azahar_csrf=", 1)[1].split(";", 1)[0]
    try:
        csrf_token = json.loads(payload).get("csrf_token", "")
    except json.JSONDecodeError:
        csrf_token = ""
    login_body = json.dumps(DEMO).encode()
    status, _, headers = request(
        port,
        "POST",
        "/api/auth/login",
        body=login_body,
        headers={
            "Content-Type": "application/json",
            "X-Requested-With": "azahar-web",
            "X-CSRF-Token": csrf_token,
            "Cookie": f"azahar_csrf={csrf_cookie}",
            "X-Forwarded-For": client_ip,
            "User-Agent": user_agent,
        },
    )
    session_cookie = ""
    for line in headers:
        if line.lower().startswith("set-cookie:") and "azahar_session=" in line:
            session_cookie = line.split("azahar_session=", 1)[1].split(";", 1)[0]
    status_me, _, _ = request(
        port,
        "GET",
        "/api/auth/me",
        headers={
            "Cookie": f"azahar_session={session_cookie}",
            "X-Forwarded-For": client_ip,
            "User-Agent": user_agent,
        },
    )
    return time.perf_counter() - started, status, "ok" if status_me == 200 else f"me={status_me}"


def run_login_flow(port: int, workers: int, total: int) -> dict[str, object]:
    latencies: list[float] = []
    outcomes: dict[str, int] = {}
    lock = threading.Lock()

    def worker(count: int, seed: int) -> None:
        local: list[float] = []
        local_out: dict[str, int] = {}
        # Logins must use distinct source users? The API deletes other sessions
        # for the same user agent, so each worker carries its own UA.
        for index in range(count):
            try:
                elapsed, _, outcome = login_once(port, seed * 1000 + index)
            except Exception as exc:  # noqa: BLE001 - report the failure class
                local_out[f"exception:{type(exc).__name__}"] = (
                    local_out.get(f"exception:{type(exc).__name__}", 0) + 1
                )
                continue
            local.append(elapsed)
            local_out[outcome] = local_out.get(outcome, 0) + 1
        with lock:
            latencies.extend(local)
            for key, value in local_out.items():
                outcomes[key] = outcomes.get(key, 0) + value

    per_worker = [total // workers] * workers
    for index in range(total % workers):
        per_worker[index] += 1
    threads = [threading.Thread(target=worker, args=(per_worker[i], i)) for i in range(workers)]
    started = time.perf_counter()
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    elapsed = time.perf_counter() - started
    return {
        "workers": workers,
        "requests": total,
        "elapsed_seconds": round(elapsed, 3),
        "throughput_rps": round(total / elapsed, 2),
        "latency_ms": summarise_ms(latencies),
        "outcomes": outcomes,
        "all_succeeded": set(outcomes) == {"ok"} and len(latencies) == total,
    }


def peak_rss_kb(pid: int) -> int:
    try:
        status = Path(f"/proc/{pid}/status").read_text()
    except OSError:
        return 0
    for line in status.splitlines():
        if line.startswith("VmHWM:"):
            return int(line.split()[1])
    return 0


def hyperfine(command: str, runs: int) -> dict[str, object] | None:
    binary = Path("/tmp/hyperfine")
    if not binary.exists():
        found = shutil_which("hyperfine")
        if not found:
            return None
        binary = Path(found)
    output = Path("/tmp/azahar-hyperfine.json")
    try:
        subprocess.run(
            [str(binary), "--warmup", "2", "--runs", str(runs), "--export-json", str(output), command],
            check=True,
            capture_output=True,
            timeout=300,
        )
        data = json.loads(output.read_text())["results"][0]
        wall = data["summary"]["time_wall_clock"]
        return {
            "command": command,
            "runs": wall["count"],
            "mean_seconds": round(wall["mean"], 4),
            "median_seconds": round(wall["median"], 4),
            "stddev_seconds": round(wall["stddev"], 4),
            "min_seconds": round(wall["min"], 4),
            "max_seconds": round(wall["max"], 4),
        }
    except (subprocess.SubprocessError, OSError, json.JSONDecodeError, KeyError) as exc:
        return {"error": str(exc)}


def shutil_which(name: str) -> str | None:
    for directory in os.environ.get("PATH", "").split(os.pathsep):
        candidate = Path(directory) / name
        if candidate.exists():
            return str(candidate)
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--port", type=int, default=18080)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument("--requests", type=int, default=2400)
    parser.add_argument("--login-requests", type=int, default=40)
    parser.add_argument("--env", action="append", default=[], help="KEY=VALUE passed to the server")
    args = parser.parse_args()

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

    log = open("/tmp/azahar-bench-server.log", "wb")
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=subprocess.STDOUT,
                               start_new_session=True)
    try:
        startup = wait_health(args.port, process)
        # Warm up caches and connections.
        run_read_mix(args.port, 4, 80)
        read_mix = run_read_mix(args.port, args.workers, args.requests)
        if not read_mix["all_succeeded"]:
            raise RuntimeError(f"read mix did not return 200 for every request: {read_mix}")
        rss_reads = peak_rss_kb(process.pid)
        login_flow = run_login_flow(args.port, 4, args.login_requests)
        if not login_flow["all_succeeded"]:
            raise RuntimeError(f"login flow did not succeed for every request: {login_flow}")
        rss = peak_rss_kb(process.pid)
        hf_health = hyperfine(
            f"curl -fsS -o /dev/null http://127.0.0.1:{args.port}/api/health", 10
        )
        document = {
            "label": args.label,
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "binary": {"path": str(binary), "bytes": binary.stat().st_size},
            "server": {
                "startup_seconds": round(startup, 3),
                "peak_rss_after_reads_kb": rss_reads,
                "peak_rss_after_reads_mb": round(rss_reads / 1024, 1),
                "peak_rss_kb": rss,
                "peak_rss_mb": round(rss / 1024, 1),
            },
            "load": {"read_mix": read_mix, "login_flow": login_flow},
            "hyperfine": {"health": hf_health},
            "environment": {
                "workers": args.workers,
                "requests": args.requests,
                "login_requests": args.login_requests,
                "database": env["TURSO_DATABASE_URL"],
            },
        }
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps(document, indent=2) + "\n")
        print(json.dumps(document, indent=2))
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

#!/usr/bin/env bash
# Run several k6 processes against one API server and record aggregate capacity.
#
#   ./bench/k6-capacity.sh target/release/azahar-api rust
#   CLIENTS=10 VUS=50 DURATION=15s SERVER_CPUS=0-1 ./bench/k6-capacity.sh ...
#
# The server is pinned to SERVER_CPUS and each k6 process to its own CPU, so a
# twelve-core machine can give the server two cores and the clients ten.

set -euo pipefail

BIN="${1:-target/release/azahar-api}"
LABEL="${2:-rust}"
PORT="${PORT:-18120}"
CLIENTS="${CLIENTS:-10}"
CLIENT_CPU_START="${CLIENT_CPU_START:-2}"
CLIENT_CPU_COUNT="${CLIENT_CPU_COUNT:-10}"
SERVER_CPUS="${SERVER_CPUS:-0-1}"
VUS="${VUS:-50}"
DURATION="${DURATION:-15s}"
IP_POOL="${IP_POOL:-200000}"
OUT="${OUT:-bench/k6-capacity-$LABEL.json}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$BIN" = /* ]]; then
    BIN_PATH="$BIN"
else
    BIN_PATH="$ROOT/$BIN"
fi
LOG="/tmp/k6-capacity-$LABEL.server.log"
rm -f /tmp/k6-capacity-"$LABEL"-*.json /tmp/k6-capacity-"$LABEL"-*.log

APP_ENV=development \
API_ADDR="127.0.0.1:$PORT" \
APP_ORIGIN="http://127.0.0.1:5173" \
TURSO_DATABASE_URL="http://127.0.0.1:8880" \
COOKIE_SECURE=false \
SEED_DEMO=true \
TRUST_PROXY=true \
taskset -c "$SERVER_CPUS" "$BIN_PATH" >"$LOG" 2>&1 &
SERVER_PID=$!
trap 'kill -TERM "$SERVER_PID" 2>/dev/null || true' EXIT

started=$(date +%s.%N)
for _ in $(seq 1 300); do
    if curl -fsS -m 1 "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
curl -fsS -m 1 "http://127.0.0.1:$PORT/api/health" >/dev/null
startup=$(python3 -c "import sys,time; print(time.time()-float(sys.argv[1]))" "$started")

cpu_ticks() {
    awk '{print $14+$15}' "/proc/$SERVER_PID/stat"
}

peak_rss() {
    awk '/VmHWM/ {print $2}' "/proc/$SERVER_PID/status" 2>/dev/null || true
}

before=$(cpu_ticks)
run_started=$(date +%s.%N)
pids=()
for ((index = 0; index < CLIENTS; index++)); do
    cpu=$((CLIENT_CPU_START + index % CLIENT_CPU_COUNT))
    taskset -c "$cpu" k6 run \
        --summary-export "/tmp/k6-capacity-$LABEL-$index.json" \
        -e BASE_URL="http://127.0.0.1:$PORT" \
        -e VUS="$VUS" \
        -e DURATION="$DURATION" \
        -e IP_POOL="$IP_POOL" \
        -e IP_OFFSET=$((index * 1000)) \
        "$ROOT/bench/k6-capacity.js" \
        >"/tmp/k6-capacity-$LABEL-$index.log" 2>&1 &
    pids+=($!)
done
wait "${pids[@]}"
run_finished=$(date +%s.%N)
after=$(cpu_ticks)
rss=$(peak_rss)
ticks=$(getconf CLK_TCK)

python3 - "$LABEL" "$CLIENTS" "$VUS" "$DURATION" "$startup" "$run_started" \
    "$run_finished" "$before" "$after" "$ticks" "$rss" "$OUT" <<'EOF'
import json
import sys
from pathlib import Path

(label, clients, vus, duration, startup, started, finished, before, after, ticks, rss, out) = sys.argv[1:]
label = label
clients = int(clients)
vus = int(vus)
startup = float(startup)
started = float(started)
finished = float(finished)
cpu_seconds = (float(after) - float(before)) / float(ticks)
wall = finished - started
requests = 0
failed = 0
summaries = []
p50s = []
p95s = []
p99s = []
for index in range(clients):
    path = Path(f"/tmp/k6-capacity-{label}-{index}.json")
    if not path.exists():
        raise SystemExit(f"missing k6 summary: {path}")
    metrics = json.loads(path.read_text())["metrics"]
    count = metrics["http_reqs"]["count"]
    duration_metrics = metrics["http_req_duration"]
    requests += count
    failed += metrics["http_req_failed"]["value"] * count
    p50s.append(duration_metrics["med"])
    p95s.append(duration_metrics["p(95)"])
    p99s.append(duration_metrics["p(99)"])
    summaries.append(
        {
            "requests": count,
            "throughput_rps": round(metrics["http_reqs"]["rate"], 1),
            "p99_ms": round(duration_metrics["p(99)"], 3),
        }
    )


def median(values):
    values = sorted(values)
    middle = len(values) // 2
    if len(values) % 2:
        return values[middle]
    return (values[middle - 1] + values[middle]) / 2


document = {
    "label": label,
    "clients": clients,
    "vus_per_client": vus,
    "duration": duration,
    "wall_seconds": round(wall, 3),
    "startup_seconds": round(startup, 3),
    "peak_rss_kb": int(rss) if rss else 0,
    "requests": int(requests),
    "throughput_rps": round(requests / wall, 1),
    "failed_requests": int(round(failed)),
    "server_cpu_seconds": round(cpu_seconds, 3),
    "server_cpu_ms_per_request": round(cpu_seconds * 1000 / requests, 4),
    "server_core_fraction": round(cpu_seconds / wall, 3),
    "latency_ms": {
        "p50_median": round(median(p50s), 3),
        "p95_median": round(median(p95s), 3),
        "p99_median": round(median(p99s), 3),
        "p99_max": round(max(p99s), 3),
    },
    "clients_detail": summaries,
}
Path(out).write_text(json.dumps(document, indent=2) + "\n")
print(json.dumps({key: document[key] for key in (
    "label", "clients", "wall_seconds", "requests", "throughput_rps",
    "failed_requests", "server_cpu_ms_per_request", "server_core_fraction",
    "peak_rss_kb", "latency_ms",
)}, indent=2))
EOF

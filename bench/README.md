# Benchmarks

The same harness runs against the Go oracle and the Rust port. It starts the
binary itself (development env, `SEED_DEMO=true`, local `sqld` on :8880),
waits for `/api/health`, drives a fixed read mix and a login flow, and records:

- p50/p95/p99 and mean latency in milliseconds
- throughput in requests per second
- peak resident memory after the read mix and after the logins
- binary size in bytes
- startup time to the first healthy response
- a hyperfine wall-clock measurement of `curl /api/health`

## Run it

```sh
make dev-db                       # sqld on http://127.0.0.1:8880
cd backend && go build -o /tmp/azahar-api ./cmd/api
python3 bench/bench.py --bin /tmp/azahar-api --label go-oracle --out bench/baseline.json

cargo build --release -p azahar-api
make rust-bench                   # writes bench/after.json
```

`bench/bench.py` needs Python 3 and its standard library only. `bench/charts.py`
needs `matplotlib` (3.11 in this environment). A running libSQL server is
required; `hyperfine` is optional (`/tmp/hyperfine` or on `PATH`).
The Go baseline uses the plain `go build` artifact (DWARF included) while Rust
uses a release `cargo build`; both sizes are reported as produced.

## Reading the numbers

- The read mix is 16 workers × 2400 requests across `/api/products`,
  `/api/categories`, `/api/plugins`, `/api/storefront/theme`, one product and
  `/api/health`, with a rotating `X-Forwarded-For` and `TRUST_PROXY=true` so the
  per-IP rate limiter does not dominate. These routes are behind the same
  in-process read cache in both services, so the number measures the HTTP read
  path (routing, cache, JSON), not a fresh database query. The harness fails
  the run unless every request returns 200 and every login flow reaches `/me`.
- Percentiles are nearest-rank over one run; `p99` with fewer than 100 samples
  equals the maximum sample, which the report labels as such. Run the harness
  more than once if you need run-to-run variance.
- The login flow is 4 workers × 40 register-free logins with the demo customer,
  measuring Argon2id + session creation + `/api/auth/me`.
- `peak_rss_after_reads_kb` is the peak resident memory up to the end of the
  read mix (including startup and seeding); the overall peak includes
  concurrent Argon2id hashes (64 MiB per login), which dominates on both sides.
- Go micro-benchmarks are in `bench/baseline-micro.txt` (`go test -bench`).

Charts generated from these documents live in `docs/charts/`.
`bench/k6-cpu.json` holds the server CPU values measured around the k6 runs;
`python3 bench/charts.py` renders them as `cpu-per-request.png`.

## k6

Grafana k6 scripts drive the same read mix and login flow:

```sh
# Start the API yourself first, then:
BASE_URL=http://127.0.0.1:18080 VUS=50 DURATION=20s IP_POOL=8192 \
    k6 run bench/k6-read.js

BASE_URL=http://127.0.0.1:18080 VUS=4 ITERATIONS=10 \
    k6 run bench/k6-login.js
```

`k6-read.js` rotates `X-Forwarded-For` across `IP_POOL` addresses; keep it
below 10,000 for the normal read path and above 10,000 to exercise rate-limiter
eviction. k6 itself is the throughput bottleneck on a two-core machine (one
pinned k6 core saturates at roughly 12-13k requests/s here), so read server CPU
per request from `/proc/<pid>/stat` around the run for the comparison metric.

Results with server pinned to CPU 0 and k6 to CPU 1, 50 VUs, 20 s, 8,192
addresses:

| run | throughput (rps) | server CPU (ms/request) | p50 (ms) | p99 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Rust before | 13,044 | 0.0337 | 3.30 | 12.77 |
| Go oracle | 12,117 | 0.0422 | 3.56 | 13.32 |
| Rust after | 12,820 | 0.0236 | 3.39 | 12.82 |
| Rust before, 20k addresses | 12,284 | 0.0571 | 3.42 | 13.48 |
| Rust after, 20k addresses | 13,111 | 0.0241 | 3.33 | 12.52 |

The 20k-address rows cross the limiter cap; the before run produced a single
429 (one duplicated address), the after run produced none. Server CPU per
request was read from `/proc/<pid>/stat` around each run.

The raw k6 summaries are `k6-before-read.json`, `k6-go-read.json`,
`k6-rust-read.json`, `k6-before-churn.json`, `k6-after-churn.json`,
`k6-go-login.json` and `k6-rust-login.json`.

## Keep-alive probe

`bench.py` opens a fresh TCP connection per request, which is the right shape
for the Go-oracle comparison but makes the Python client a large part of the
measurement. `bench/hotpath.py` keeps a fixed pool of HTTP/1.1 connections and
reports server CPU time per request alongside latency, so it can compare two
Rust builds even when the client saturates its own core.

```sh
make dev-db
cargo build --release -p azahar-api
python3 bench/hotpath.py --bin target/release/azahar-api --label rust \
    --out bench/hotpath.json

# On a two-core machine, keep the client and server on separate cores.
taskset -c 1 python3 bench/hotpath.py --server-cpu 0 \
    --bin target/release/azahar-api --label rust --out bench/hotpath.json
```

It needs `aiohttp` in addition to the Python standard library. `--unique-ips`
controls how many rotated `X-Forwarded-For` addresses the probe uses; a value
larger than the rate limiter's 10,000-bucket cap exercises the eviction path.
`server_cpu_ms_per_request` is the most stable comparison metric on a small
machine; wall-clock throughput is limited by the Python client.

### Results

Measured on the 2-core development machine, server pinned to CPU 0 and client
to CPU 1, 32 connections, five runs of 12,000 requests. The baseline is the
Rust service before the hot-path work; the Go column is the oracle at
`backend/`:

| run | throughput (rps) | server CPU (ms/request) | p99 (ms) |
| --- | ---: | ---: | ---: |
| Rust before | 11,629 | 0.0317 | 3.97 |
| Go oracle | 11,265 | 0.0392 | 3.39 |
| Rust after | 11,600–12,100 | 0.0217–0.0233 | 3.0–3.7 |

Repeated runs on a loaded laptop vary by roughly 10%; the CPU column is the
stable comparison. The rotated-address churn run (20,000 distinct addresses,
16 connections) drops from 39.0 µs to 22.0 µs of server CPU per request after
the rate-limiter eviction change. The raw documents are
`hotpath-baseline.json`, `hotpath-go.json`, `hotpath.json`,
`hotpath-churn-baseline.json` and `hotpath-churn.json`.

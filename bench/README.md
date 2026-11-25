# Benchmarks

Grafana k6 drives the load. Two scripts cover the server paths:

- `k6-read.js` — 50 VUs for 20 s across `/api/products`, `/api/categories`,
  `/api/plugins`, `/api/storefront/theme`, one product and `/api/health`, with a
  rotating `X-Forwarded-For` address. This keeps the per-IP rate limiter out of
  the way and measures the HTTP path (routing, cache, JSON).
- `k6-login.js` — 4 VUs × 10 CSRF → login → `/api/auth/me` flows. It exercises
  Argon2id, session creation and the authenticated read.

Both scripts use keep-alive connections. k6 itself saturates one core at roughly
12–13k requests/s on the development machine, so the number that compares
implementations is **server CPU per request**, read from `/proc/<pid>/stat`
around the run. The charts and tables also report latency and throughput, but
the client ceiling applies to those.

## Run them

```sh
make dev-db                 # local Turso/libSQL on :8880
make dev-api                # Go oracle on :8080 (or run the Rust API there)

make k6-read                # API_URL defaults to http://127.0.0.1:8080
make k6-login
python3 bench/charts.py     # writes docs/charts/*.png
make diagrams               # renders docs/diagrams/request-path.svg
```

`API_URL`, `K6_VUS`, `K6_DURATION` and `K6_IP_POOL` override the defaults:

```sh
make k6-read API_URL=http://127.0.0.1:18080 K6_VUS=50 K6_DURATION=20s K6_IP_POOL=8192
```

`IP_POOL` below 10,000 measures the normal read path. Above 10,000 exercises
rate-limiter bucket eviction; the 20,000-address churn rows in
`bench/k6-results.json` come from the earlier churn runs.

## Results

`bench/k6-results.json` is the combined document used by the charts: read mix,
20k-address churn, login flow and process footprint (RSS, binary size, startup).
The raw k6 summary exports are `bench/k6-*.json`.

| Scenario | Metric | Go oracle | Rust before | Rust after |
| --- | --- | ---: | ---: | ---: |
| Read mix | Throughput | 12,117 req/s | 13,044 req/s | 12,820 req/s |
| Read mix | p50 / p95 / p99 | 3.56 / 10.20 / 13.32 ms | 3.30 / 9.67 / 12.77 ms | 3.39 / 9.84 / 12.82 ms |
| Read mix | Server CPU | 42.2 µs | 33.7 µs | 23.6 µs |
| 20k addresses | Server CPU | — | 57.1 µs | 24.1 µs |

Run conditions: server pinned to CPU 0, k6 to CPU 1, 50 VUs, 20 s, 8,192
addresses for the read mix and 20,000 for churn. Repeated runs on a shared
development machine vary by roughly 10%; treat them as single-run observations,
not a capacity claim. The k6 login comparison is included in
`k6-results.json`, but four concurrent Argon2id hashes make its wall-clock tail
noisy; the per-login CPU numbers are the more useful login signal.

## Charts

`bench/charts.py` renders five PNGs from `bench/k6-results.json`:

- `latency.png` — read and login p50/p95/p99
- `throughput.png` — read and login throughput
- `cpu-per-request.png` — server CPU per read, normal and churn
- `memory.png` — peak RSS after reads and after logins
- `binary-size.png` — release binary size

The script needs Python 3 and `matplotlib`. `make diagrams` needs Graphviz.

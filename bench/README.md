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
- The login flow is 4 workers × 24 register-free logins with the demo customer,
  measuring Argon2id + session creation + `/api/auth/me`.
- `peak_rss_after_reads_kb` is the peak resident memory up to the end of the
  read mix (including startup and seeding); the overall peak includes
  concurrent Argon2id hashes (64 MiB per login), which dominates on both sides.
- Go micro-benchmarks are in `bench/baseline-micro.txt` (`go test -bench`).

Charts generated from these documents live in `docs/charts/`.

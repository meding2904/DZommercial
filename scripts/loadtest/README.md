# Load probe

`main.go` is a small dependency-free probe. It sends a fixed number of GET
requests through a pool of workers and reports throughput, status distribution
and latency percentiles. It exits non-zero when any request is not a 2xx or the
expected status.

## Run

From this folder:

```bash
go run . -base http://127.0.0.1:8080 -paths /api/products,/api/categories -c 50 -n 5000
```

From the repository root, the short version is:

```bash
make loadtest
```

For per-client rate-limit testing, start the API with `TRUST_PROXY=true` and
pass `-spread-ips`; the probe then sends a rotating `X-Forwarded-For`. Use
`-headers` for a session cookie or the challenge header:

```bash
go run . -base http://127.0.0.1:8080 \
  -paths /api/checkout/challenge -c 50 -n 1000 -spread-ips \
  -headers 'X-Requested-With: azahar-web'
```

## What the demo measured

One local run of the demo stack, on the same machine as the API and the
database, gave:

| Target | Requests | Workers | 2xx | Throughput | p99 |
|---|---:|---:|---:|---:|---:|
| Public catalog reads | 20,000 | 200 | 100% | 11,725 req/s | 132 ms |
| Admin reads (metrics, orders, products, customers) | 5,000 | 50 | 100% | 1,421 req/s | 162 ms |
| SvelteKit pages, server-rendered | 3,000 | 50 | 100% | 617 req/s | 296 ms |
| Checkout challenges | 1,000 | 50 | 100% | 3,699 req/s | 15 ms |
| Counter sales, stock 10, 30 racing requests | 30 | 30 | 10 sold, 20 conflicts | n/a | n/a |

The 30 racing counter sales moved stock from 10 to 0 exactly. The other 20
requests returned `409 conflict`; none returned 500 and the stock never went
negative.

The same probe from one address without `-spread-ips` returns 180 `429` after
the 120 general requests, which is the limiter doing its job.

## Limits found and fixed

The first admin run returned 500 for 2,788 of 5,000 requests. The admin order
list fetched items one order at a time, so a 200-order page became 200 extra
database round trips and exhausted the libsql connection pool. The list now
reads every line for the page in one query. The counter sale path also took a
write lock around its transaction: the bundled libsql server has one writer, so
30 racing sales now become 10 sales and 20 stock conflicts instead of connection
errors.

The public and shop-wide admin reads now sit behind a short in-process cache
(1 to 5 seconds) and every write clears it. That is why the cached numbers are
much faster than the first uncached run. The first uncached public run before
the cache was 1,369 req/s with a 720 ms p99; the first uncached admin run was
330 req/s with a 531 ms p99. Both were clean at those levels.

One hundred concurrent admin reads still exceeds the demo libsql server's
connection-creation timeout. That is a database resource limit, not a crash;
the public storefront and the admin console at realistic staff concurrency were
clean. Raise the database resources before benchmarking higher admin
concurrency.

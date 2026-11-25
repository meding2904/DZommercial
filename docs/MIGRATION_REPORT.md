# Rust migration report

## What was done

The Go server was ported to a Rust workspace on `rust-migrate` while the Go tree
stayed runnable as the oracle. Every ported function is covered by a Rust test
that drives the real Go code through a line-delimited JSON oracle and compares
outputs, by a database differential, or by an HTTP-level differential.

- **624 / 624** non-test Go functions in the migration scope are
  ticked in `MIGRATION.md` (the four `cmd/oracle-*` harness functions added for
  testing are excluded from this count).
- **24 workspace crates** compile, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --all --check` pass.
- The Rust `azahar-api` boots against Turso, runs the same migrations and seed,
  and serves the ported HTTP surface.
- Live Go-vs-Rust HTTP diff (`scripts/httpdiff/httpdiff.py --group all`) matches
  every case: health, CSRF, auth, catalogue, product, theme, shipping,
  order-by-token, account orders, admin reads/mutations, plugin manifests and
  settings, AI runs, code-mode plans, phone devices, checkout
  validation/challenge and a successful 201 checkout driven through the
  deterministic-captcha oracle seam. The Tauri host has 1,437 oracle
  comparisons for profile, clamp, monitor-position, URL and bridge helpers.

## Packages

| Package | Ported | Total | Status |
| --- | ---: | ---: | --- |
| `internal/store` | 93 | 93 | done |
| `internal/auditbatch` | 1 | 1 | done |
| `internal/config` | 9 | 9 | done |
| `internal/courier` | 44 | 44 | done |
| `internal/db` | 5 | 5 | done |
| `internal/auth` | 6 | 6 | done |
| `internal/ids` | 1 | 1 | done |
| `internal/orderstatus` | 2 | 2 | done |
| `internal/secrets` | 3 | 3 | done |
| `internal/plugins` | 41 | 41 | done |
| `internal/orderintake` | 17 | 17 | done |
| `internal/productrules` | 9 | 9 | done |
| `internal/storefront` | 13 | 13 | done |
| `internal/httpapi` | 156 | 156 | done |
| `internal/plugins/ai` | 20 | 20 | done |
| `internal/plugins/codemode` | 55 | 55 | done |
| `internal/plugins/oils` | 3 | 3 | done |
| `internal/plugins/pcparts` | 3 | 3 | done |
| `internal/plugins/pcbuilder` | 6 | 6 | done |
| `internal/plugins/phone` | 26 | 26 | done |
| `internal/seed` | 5 | 5 | done |
| `cmd/api` | 1 | 1 | done |
| `desktop/internal/api` | 86 | 86 | done |
| `desktop` | 19 | 19 | done |

No scoped function remains. The Tauri host is built, unit-tested and smoke-tested
under a private Weston compositor: the real window opens, the clamp path runs,
the close hook persists bounds and the process exits cleanly. The headless
compositor adds a ~52 px client-side shadow to the reported inner size; physical
display placement, keyring and single-instance D-Bus behaviour remain under
risks.

## Benchmarks

Grafana k6 drives the load: `bench/k6-read.js` runs 50 VUs for 20 s against the
seeded local libSQL database, rotating 8,192 `X-Forwarded-For` addresses across
six public routes. The Go oracle, the pre-optimization Rust build and the final
Rust build ran the same script. `bench/k6-results.json` is the combined
document, `bench/k6-*.json` holds the raw k6 summaries, and `docs/charts/` holds
the charts.

| Metric | Go oracle | Rust before | Rust after |
| --- | ---: | ---: | ---: |
| Read throughput | 12,117 req/s | 13,044 req/s | 12,820 req/s |
| Read p50 / p95 / p99 | 3.56 / 10.20 / 13.32 ms | 3.30 / 9.67 / 12.77 ms | 3.39 / 9.84 / 12.82 ms |
| Server CPU per read | 42.2 µs | 33.7 µs | 23.6 µs |
| Peak RSS after reads | 100.1 MiB | 20.8 MiB | 17.0 MiB |
| Peak RSS including logins | 539.0 MiB | 276.6 MiB | 273.2 MiB |
| Release binary | 14.7 MB | 26.9 MB | 20.5 MB |
| Startup to healthy | 0.255 s | 0.355 s | 0.052 s |

The read path includes the in-process read cache on both sides. Server CPU was
read from `/proc/<pid>/stat` around each k6 run; it is the comparison metric
because k6 saturates one core at roughly 12–13k requests/s. The 20,000-address
churn path drops from 57.1 µs to 24.1 µs (`docs/charts/cpu-per-request.png`).
Login uses the same Argon2id parameters (64 MiB, 3 passes, 4 lanes); four
concurrent hashes dominate its wall-clock tail, so the read mix is the capacity
signal. These are single runs, not a capacity claim. Runtime memory is about a
sixth of Go; the binary is larger because the Rust release build keeps symbols
while the Go build keeps DWARF. The request-path diagram is
`docs/diagrams/request-path.svg`.

## Auth

Argon2id PHC strings are byte-compatible in both directions, including dummy
hashing for unknown emails. Sessions use `tower-sessions` 0.15 with a libSQL
store in the Rust-owned `web_sessions` table, absolute role-dependent expiry,
session-ID rotation on login and one-time logout. The `auth_turso` test proves
login/me/logout/expiry/rotation against the real Turso server. Details and crate
versions: `docs/auth.md`.

## Known behaviour differences

Recorded in `MIGRATION.md`:

- Sessions are not shared with the Go oracle (`web_sessions` vs `sessions`); a
  cutover needs one re-login.
- Go-verifiable hashes with Argon2 `m < 8*p` are deliberately rejected.
- Login no longer deletes every session for the same user agent; it rotates the
  current one.
- Plugin product enrichment is synchronous; the phone enricher is exposed as an
  async service until the HTTP layer switches enrichment to async.
- `ProductType.fields` cannot distinguish a nil slice from an empty one.
- Demo seed orders use a Rust RNG with the same plan but different concrete
  draws; order-dependent HTTP responses are compared by shape, not value.
- Storefront Unicode error-text tables are 16.0 vs Go's 17.0.
- The seed differential is `#[ignore]` because it needs a disposable libSQL
  server; it was run once against a throwaway container.

## Risks

- The Tauri host passes a headless Weston smoke (window opens, clamp runs, bounds
  are saved on close, no panic/window error). Physical-display monitor placement,
  CSP enforcement, keyring and D-Bus single-instance remain unverified, and
  Weston adds a client-side shadow to the reported window size. The Go Wails host
  remains the oracle.
- The keyring-backed secret store is abstracted and tested with fakes; live OS
  Secret Service integration and Windows/macOS hosts are accepted differences
  (user-approved).
- Desktop TLS roots use rustls/webpki rather than Go's OS trust store, and gzip
  negotiation is not enabled; enterprise CAs or gzip-only servers can differ.
- Checkout success is covered by 39 unit/integration tests and by the live HTTP
  diff through the deterministic-captcha seam; the seam exists only when
  `AZAHAR_TEST_CAPTCHA_CODE` is set.
- Phone device writes start their own transaction on the shared connection
  instead of taking the store's single-writer lock. Admin device edits are rare,
  but this should be folded into `begin_write` before using the phone API under
  concurrent load.
- The store uses one libSQL connection; local-database reads can observe an open
  write transaction, unlike the Go pool. Remote Turso uses a separate stream.
- A session-store failure after user creation returns 500 with the account
  already committed (documented in the HTTP core report).
- The k6 benchmark is a single run per build; use repeated runs and CPU pinning
  for capacity decisions.

## Reproduce

```sh
make dev-db
make rust-check          # fmt, clippy, workspace tests (needs Turso)
make oracle-diff         # all differential tests with --nocapture
make dev-api             # Go oracle on :8080; or run the Rust API there
make k6-read
make k6-login
make charts              # docs/charts/*.png from bench/k6-results.json
make diagrams            # docs/diagrams/request-path.svg
python3 scripts/httpdiff/httpdiff.py --oracle http://127.0.0.1:18080 \
    --candidate http://127.0.0.1:18081 --group all
```

# Rust migration report

## What was done

The Go server was ported to a Rust workspace on `rust-migrate` while the Go tree
stayed runnable as the oracle. Every ported function is covered by a Rust test
that drives the real Go code through a line-delimited JSON oracle and compares
outputs, by a database differential, or by an HTTP-level differential.

- **426 / 624** non-test Go functions in the migration scope are
  ticked in `MIGRATION.md` (the four `cmd/oracle-*` harness functions added for
  testing are excluded from this count).
- **22 workspace crates** compile, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --all --check` pass.
- The Rust `azahar-api` boots against Turso, runs the same migrations and seed,
  and serves the ported HTTP surface.
- Live Go-vs-Rust HTTP diff (`scripts/httpdiff/httpdiff.py --group all`) matches
  for health, CSRF, auth, catalogue, product, theme, shipping, order-by-token,
  account orders, admin reads/mutations, checkout validation/challenge and phone
  device listing. The only expected gap is the plugin list, which omits the AI
  and code-mode plugins that are not ported.

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
| `internal/plugins` | 31 | 41 | partial |
| `internal/orderintake` | 17 | 17 | done |
| `internal/productrules` | 9 | 9 | done |
| `internal/storefront` | 13 | 13 | done |
| `internal/httpapi` | 148 | 156 | partial |
| `internal/plugins/ai` | 0 | 20 | partial |
| `internal/plugins/codemode` | 0 | 55 | partial |
| `internal/plugins/oils` | 3 | 3 | done |
| `internal/plugins/pcparts` | 3 | 3 | done |
| `internal/plugins/pcbuilder` | 6 | 6 | done |
| `internal/plugins/phone` | 26 | 26 | done |
| `internal/seed` | 5 | 5 | done |
| `cmd/api` | 1 | 1 | done |
| `desktop/internal/api` | 0 | 86 | partial |
| `desktop` | 0 | 19 | partial |

Remaining: the AI plugin (20), the code-mode plugin (55), ten plugin HTTP
`Context` helpers and the generic handler-bearing route registration, eight
HTTP logging/panic/limiter-shutdown helpers, and the desktop Wails host (105).
The desktop host and the SvelteKit UI are documented as deferred in
`MIGRATION.md`.

## Benchmarks

Same harness, same request counts, same seeded local libSQL database, one run
per side on the development machine. `bench/baseline.json` and
`bench/after.json` hold the raw documents; `docs/charts/` holds the charts.

| Metric | Go oracle | Rust |
| --- | ---: | ---: |
| Read throughput | 5,964 req/s | 6,334 req/s |
| Read p50 / p95 / p99 | 2.50 / 4.29 / 5.29 ms | 2.34 / 4.10 / 5.00 ms |
| Login p50 / p95 / p99 | 160.4 / 657.8 / 827.4 ms | 145.2 / 153.0 / 153.9 ms |
| Peak RSS after reads | 100.0 MiB | 17.7 MiB |
| Peak RSS including logins | 541.9 MiB | 273.4 MiB |
| Release binary | 14.7 MB | 25.2 MB |
| Startup to healthy | 0.254 s | 0.061 s |

The read path includes the in-process read cache on both sides. Login uses the
same Argon2id parameters (64 MiB, 3 passes, 4 lanes); the Go tail is dominated by
CPU contention from four concurrent hashes. These are single runs, not a
capacity claim. Runtime memory is about a sixth of Go; the binary is larger
because the Rust release build keeps symbols while the Go build keeps DWARF.

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

- The AI and code-mode admin screens and their plugin endpoints do not exist in
  Rust yet; the corresponding README features are Go-only.
- Checkout success is covered by 39 unit/integration tests against a local
  database, but the live HTTP diff only compares validation and challenge
  responses because captcha codes are server-local.
- Phone device writes start their own transaction on the shared connection
  instead of taking the store's single-writer lock. Admin device edits are rare,
  but this should be folded into `begin_write` before using the phone API under
  concurrent load.
- The store uses one libSQL connection; local-database reads can observe an open
  write transaction, unlike the Go pool. Remote Turso uses a separate stream.
- A session-store failure after user creation returns 500 with the account
  already committed (documented in the HTTP core report).
- The benchmark is a single run; use repeated runs and CPU pinning for capacity
  decisions.

## Reproduce

```sh
make dev-db
make rust-check          # fmt, clippy, workspace tests (needs Turso)
make oracle-diff         # all differential tests with --nocapture
make bench-baseline      # Go oracle -> bench/baseline.json
make rust-bench          # Rust -> bench/after.json
python3 bench/charts.py
python3 scripts/httpdiff/httpdiff.py --oracle http://127.0.0.1:18080 \
    --candidate http://127.0.0.1:18081 --group all
```

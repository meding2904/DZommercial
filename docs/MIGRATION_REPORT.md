# Rust migration report

## What was done

The Go server was ported to a Rust workspace on `rust-migrate` while the Go tree
stayed runnable as the oracle. Every ported function is covered by a Rust test
that drives the real Go code through a line-delimited JSON oracle and compares
outputs, by a database differential, or by an HTTP-level differential.

- **605 / 624** non-test Go functions in the migration scope are
  ticked in `MIGRATION.md` (the four `cmd/oracle-*` harness functions added for
  testing are excluded from this count).
- **22 workspace crates** compile, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --all --check` pass.
- The Rust `azahar-api` boots against Turso, runs the same migrations and seed,
  and serves the ported HTTP surface.
- Live Go-vs-Rust HTTP diff (`scripts/httpdiff/httpdiff.py --group all`) matches
  every case: health, CSRF, auth, catalogue, product, theme, shipping,
  order-by-token, account orders, admin reads/mutations, plugin manifests and
  settings, AI runs, code-mode plans, phone devices and checkout
  validation/challenge.

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
| `desktop` | 0 | 19 | partial |

Remaining: the 19 Wails window-host functions in `desktop/main.go`. The desktop
API client (86 functions) is ported; the SvelteKit UI remains the UI layer by
design. Choosing a Rust GUI framework (Tauri, egui/wry, or keeping the Go host)
is a product decision, so it is recorded in `MIGRATION.md` rather than guessed.

## Benchmarks

Same harness, same request counts, same seeded local libSQL database, one run
per side on the development machine. `bench/baseline.json` and
`bench/after.json` hold the raw documents; `docs/charts/` holds the charts.

| Metric | Go oracle | Rust |
| --- | ---: | ---: |
| Read throughput | 6,106 req/s | 5,731 req/s |
| Read p50 / p95 / p99 | 2.49 / 4.07 / 5.01 ms | 2.56 / 4.77 / 5.87 ms |
| Login p50 / p95 / p99 | 480.0 / 926.1 / 983.2 ms | 168.0 / 174.8 / 179.3 ms |
| Peak RSS after reads | 100.1 MiB | 19.3 MiB |
| Peak RSS including logins | 539.0 MiB | 275.0 MiB |
| Release binary | 14.7 MB | 26.9 MB |
| Startup to healthy | 0.255 s | 0.062 s |

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

- The Wails window host (`desktop/main.go`, 19 functions) remains Go. The Rust
  desktop API client is ported, but no Rust GUI shell is wired yet.
- The keyring-backed secret store is abstracted and tested with fakes; live OS
  Secret Service integration is not covered by the differential.
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

# Azahar Go → Rust migration

Living plan for the Rust port. The Go tree stays runnable and untouched as the **oracle**; every Rust function is diff-tested against it before its box is ticked.

## Scope decisions

- **In scope:** every Go package under `backend/` (the API, store, plugins, courier, seed and CLI) and the Go host logic under `desktop/`.
- **Out of scope:** `frontend/` (SvelteKit + TypeScript). It is the UI layer, not a second implementation of server logic; the Rust API keeps its HTTP contract so the same frontend runs against it. Porting Svelte components to Rust is not a language migration of logic and would destroy the working UI. Logged as decision D1.
- **Oracle rule:** no file under `backend/` is deleted or modified. Tooling added next to it (for example a `cmd/oracle` harness) is additive and does not change behaviour.
- **Function unit:** each checkbox is one Go function or method (non-test). Tests are not ported one-to-one; equivalent Rust tests and the differential harness replace them.
- **Migration branch:** `rust-migrate`. `main` is never committed to from this work.
- **Ordering:** pure leaf packages first, then the store, plugins, order intake,
  HTTP layer, seed and binary. A function is only ticked after its differential
  harness reports zero diffs.
- **Decision D1 — frontend:** `frontend/` stays SvelteKit. It is the UI layer;
  the Rust API preserves the HTTP contract so the same UI runs against it.
- **Decision D2 — auth:** Argon2id via the `argon2` crate keeps the Go PHC
  format. Sessions move to `tower-sessions` 0.15 with a local libSQL-backed
  `SessionStore` and the Rust-owned `web_sessions` table. The Go `sessions`
  table is left alone. Full rationale and versions: `docs/auth.md`.
- **Decision D3 — desktop:** the Wails Go host in `desktop/` is inventoried
  (109 functions) and migrated after the API. Its renderer is Svelte/JS and
  stays.
- **Oracle harness convention:** each package adds `backend/cmd/oracle-<pkg>/`,
  a line-delimited JSON adapter around the Go functions. Rust integration
  tests under `crates/<crate>/tests/differential.rs` spawn it with `go run`.
  Batch all cases per invocation; per-case `go run` is too slow.
- **Oracle-only exports:** unexported Go helpers used by the harness are exposed
  from `backend/internal/<pkg>/oracle_export.go` under `//go:build oracle`, so
  the normal API binary is unchanged.
- **Decision D4 — commits:** one commit per package, with the function names in
  the body. A commit per function cannot compile on its own, which would break
  bisect and `git checkout` on the migration branch.

## Baseline (Go oracle)

`bench/baseline.json`, 16 workers x 2400 mixed reads on the local libSQL:

| Metric | Go oracle |
| --- | ---: |
| Throughput | 6191 req/s |
| Read p50 / p95 / p99 | 2.44 / 4.16 / 4.92 ms |
| Login flow p50 / p95 / p99 | 123.1 / 153.1 / 164.8 ms |
| Peak RSS after reads | 103.2 MiB |
| Peak RSS incl. Argon2 logins | 553.7 MiB |
| Binary size | 14,713,389 bytes |
| Startup to healthy | 0.102 s |
| hyperfine `curl /api/health` | 3.9 ms mean |

## Orientation

- Go packages: **27**, non-test functions/methods: **628**.
- Rust workspace crates mirror each Go package (`crates/azahar-<package>`), with `azahar-api` as the binary.
- Package dependency order (leaf-first):

```text
internal/store       <- —
internal/auditbatch  <- internal/store
internal/config      <- —
internal/courier     <- —
migrations           <- —
internal/db          <- migrations
internal/auth        <- —
internal/ids         <- —
internal/orderstatus <- —
internal/secrets     <- —
internal/plugins     <- internal/secrets, internal/store
internal/orderintake <- internal/ids, internal/orderstatus, internal/plugins, internal/store
internal/productrules <- internal/plugins
internal/storefront  <- —
internal/httpapi     <- internal/auth, internal/config, internal/courier, internal/ids, internal/orderintake, internal/orderstatus, internal/plugins, internal/productrules, internal/store, internal/storefront
internal/plugins/ai  <- internal/ids, internal/plugins, internal/store
internal/plugins/codemode <- internal/ids, internal/orderstatus, internal/plugins, internal/productrules, internal/store, internal/storefront
internal/plugins/oils <- internal/plugins
internal/plugins/pcparts <- internal/plugins
internal/plugins/pcbuilder <- internal/plugins, internal/plugins/pcparts, internal/store
internal/plugins/phone <- internal/ids, internal/plugins, internal/store
internal/seed        <- internal/auth, internal/ids, internal/store
cmd/api              <- internal/auditbatch, internal/config, internal/courier, internal/db, internal/httpapi, internal/plugins, internal/plugins/ai, internal/plugins/codemode, internal/plugins/oils, internal/plugins/pcbuilder, internal/plugins/pcparts, internal/plugins/phone, internal/seed, internal/store
cmd/oracle-ids-auth  <- internal/auth, internal/ids
cmd/oracle-orderstatus-secrets <- internal/orderstatus, internal/secrets
desktop/internal/api <- —
desktop              <- desktop/internal/api
```

## Package summary

| Package | Functions | Go LOC | Depends on |
| --- | ---: | ---: | --- |
| `internal/store` | 93 | 1906 | — |
| `internal/auditbatch` | 1 | 58 | internal/store |
| `internal/config` | 9 | 262 | — |
| `internal/courier` | 44 | 824 | — |
| `migrations` | 0 | 8 | — |
| `internal/db` | 5 | 196 | migrations |
| `internal/auth` | 6 | 136 | — |
| `internal/ids` | 1 | 15 | — |
| `internal/orderstatus` | 2 | 30 | — |
| `internal/secrets` | 3 | 70 | — |
| `internal/plugins` | 41 | 925 | internal/secrets, internal/store |
| `internal/orderintake` | 17 | 617 | internal/ids, internal/orderstatus, internal/plugins, internal/store |
| `internal/productrules` | 9 | 213 | internal/plugins |
| `internal/storefront` | 13 | 347 | — |
| `internal/httpapi` | 156 | 4641 | internal/auth, internal/config, internal/courier, internal/ids, internal/orderintake, internal/orderstatus, internal/plugins, internal/productrules, internal/store, internal/storefront |
| `internal/plugins/ai` | 20 | 689 | internal/ids, internal/plugins, internal/store |
| `internal/plugins/codemode` | 55 | 2050 | internal/ids, internal/orderstatus, internal/plugins, internal/productrules, internal/store, internal/storefront |
| `internal/plugins/oils` | 3 | 71 | internal/plugins |
| `internal/plugins/pcparts` | 3 | 152 | internal/plugins |
| `internal/plugins/pcbuilder` | 6 | 223 | internal/plugins, internal/plugins/pcparts, internal/store |
| `internal/plugins/phone` | 26 | 758 | internal/ids, internal/plugins, internal/store |
| `internal/seed` | 5 | 746 | internal/auth, internal/ids, internal/store |
| `cmd/api` | 1 | 195 | internal/auditbatch, internal/config, internal/courier, internal/db, internal/httpapi, internal/plugins, internal/plugins/ai, internal/plugins/codemode, internal/plugins/oils, internal/plugins/pcbuilder, internal/plugins/pcparts, internal/plugins/phone, internal/seed, internal/store |
| `cmd/oracle-ids-auth` | 2 | 112 | internal/auth, internal/ids |
| `cmd/oracle-orderstatus-secrets` | 2 | 94 | internal/orderstatus, internal/secrets |
| `desktop/internal/api` | 86 | 1763 | — |
| `desktop` | 19 | 269 | desktop/internal/api |

## Subagent log

| # | Role | Assignment | Status | Outcome |
| ---: | --- | --- | --- | --- |
| 1 | architect | call graph, crate layout, ordering | done | `MIGRATION.md` + `MIGRATION.json`, 27 packages / 628 functions |
| 2 | benchmark engineer | baseline harness and numbers | done | `bench/bench.py`, `bench/baseline.json`, Go micro-benchmarks |
| 3 | implementer | `ids`, `auth` | running | — |
| 4 | implementer | `config` | running | — |
| 5 | implementer | `orderstatus`, `secrets` | running | — |
| 6 | implementer | `storefront` | running | — |
| 7 | implementer | `courier` | done | 42 functions, 365 differential cases |
| 8 | implementer | `store` orders/metrics/images/audit | running | files written, tests in progress |
| 9 | implementer | `store` products/categories | done | 18 functions, 56 differential comparisons |
| 10 | differential tester | store basics | running | — |
| 11 | implementer | `storefront` | done | 13 functions, 1,280 proptest cases |
| 12 | implementer | plugin core + oils + pcparts | running | — |
| 13 | implementer | `seed` | running | — |
| 14 | reviewer | auth/config/id/secrets/status | done | 4 approve, auth changes sent back |
| 15 | reviewer | courier/store/respond/db | running | — |
| 16 | reviewer | storefront/intake/audit/bench | running | — |
| 17 | auth specialist | tower-sessions + Turso | done | `auth_turso` login/me/logout/expiry/rotation pass |
| 18 | implementer | HTTP core + auth handlers | running | — |
| 19 | implementer | auth review fixes | running | — |

Status values: `queued`, `running`, `review`, `done`, `failed`, `rebriefed`.

## Known behaviour differences

Differences are only acceptable when behaviour is unobservable at the HTTP/API boundary or is a documented security improvement. Each entry needs a justification.

| Item | Difference | Justification |
| --- | --- | --- | --- |
| Sessions | Rust stores tower-sessions records in `web_sessions`; the Go oracle keeps `sessions`. Sessions are not shared between the two services. | Replacing the hand-rolled token/session code is the point of D2. A cutover needs one re-login; the old table is left untouched for the oracle. |
| Argon2 `m < 8*p` | Go clamps memory up to `8*threads` and verifies such hashes; Rust's `argon2` crate rejects the parameters, so `verify_password` returns false. | `HashPassword` always emits `m=65536,t=3,p=4`, so no account created by this codebase is affected. Accepting impossible parameters is a security tightening. Pinned by a differential fixture. |
| Login session cleanup | Go deletes every session for the same user agent and purges expired rows at login; tower-sessions rotates only the current session. | Both invalidate the session ID on login. The Rust layer exposes `delete_expired` for a scheduled cleanup; the API wiring schedules it. |
| Non-UTF-8 env | Rust `env::var_os` values are decoded lossily; Go preserves raw bytes. | Not reachable through the JSON API; config values are UTF-8 in every deployment path. |
| Courier transport wording | reqwest error strings differ from Go `net/http`; both map to the same typed transport error. | Never rendered to API clients. |
| Storefront Unicode errors | Error-text escaping uses Unicode 16.0 category tables vs Go's 17.0 for code points added in 17.0. | Accept/validate behaviour is unchanged; only the text of an error for a newly assigned code point can differ. |

## Checklist

### `internal/store` (93 functions)

- [ ] `internal/store.FormatTime` — `backend/internal/store/types.go:141`
- [ ] `internal/store.InsertAudit` *(method)* — `backend/internal/store/audit.go:8`
- [ ] `internal/store.FlushAuditBatch` *(method)* — `backend/internal/store/audit_flush.go:15`
- [ ] `internal/store.CreateProductImage` *(method)* — `backend/internal/store/images.go:17`
- [ ] `internal/store.ParseTime` — `backend/internal/store/types.go:152`
- [ ] `internal/store.ProductImage` *(method)* — `backend/internal/store/images.go:29`
- [ ] `internal/store.Metrics` *(method)* — `backend/internal/store/metrics.go:60`
- [ ] `internal/store.CountOrders` *(method)* — `backend/internal/store/orders.go:17`
- [ ] `internal/store.NextOrderNumber` *(method)* — `backend/internal/store/orders.go:23`
- [ ] `internal/store.FormatOptionalTime` — `backend/internal/store/types.go:145`
- [ ] `internal/store.NormalizePluginID` — `backend/internal/store/products.go:374`
- [ ] `internal/store.defaultCurrency` — `backend/internal/store/products.go:381`
- [ ] `internal/store.encodeCourierEvents` — `backend/internal/store/types.go:242`
- [ ] `internal/store.encodeStringMap` — `backend/internal/store/types.go:220`
- [ ] `internal/store.CreateOrder` *(method)* — `backend/internal/store/orders.go:39`
- [ ] `internal/store.decodeCourierEvents` — `backend/internal/store/types.go:253`
- [ ] `internal/store.scanOrder` — `backend/internal/store/store.go:105`
- [ ] `internal/store.OrderByTokenHash` *(method)* — `backend/internal/store/orders.go:79`
- [ ] `internal/store.orderByID` *(method)* — `backend/internal/store/orders.go:95`
- [ ] `internal/store.OrderByID` *(method)* — `backend/internal/store/orders.go:85`
- [ ] `internal/store.OrderByIDTx` *(method)* — `backend/internal/store/orders.go:91`
- [ ] `internal/store.decodeStringMap` — `backend/internal/store/types.go:231`
- [ ] `internal/store.scanOrderItem` — `backend/internal/store/orders.go:164`
- [ ] `internal/store.orderItems` *(method)* — `backend/internal/store/orders.go:142`
- [ ] `internal/store.OrderItems` *(method)* — `backend/internal/store/orders.go:101`
- [ ] `internal/store.OrderItemsTx` *(method)* — `backend/internal/store/orders.go:105`
- [ ] `internal/store.OrderItemsByOrderIDs` *(method)* — `backend/internal/store/orders.go:112`
- [ ] `internal/store.OrdersByUser` *(method)* — `backend/internal/store/orders.go:176`
- [ ] `internal/store.AdminOrders` *(method)* — `backend/internal/store/orders.go:197`
- [ ] `internal/store.UpdateOrderStatus` *(method)* — `backend/internal/store/orders.go:239`
- [ ] `internal/store.UpdateShipment` *(method)* — `backend/internal/store/orders.go:257`
- [ ] `internal/store.UpdateTracking` *(method)* — `backend/internal/store/orders.go:277`
- [ ] `internal/store.PluginStates` *(method)* — `backend/internal/store/plugin_states.go:11`
- [ ] `internal/store.SetPluginState` *(method)* — `backend/internal/store/plugin_states.go:31`
- [ ] `internal/store.SaleWindow` *(method)* — `backend/internal/store/pricing.go:25`
- [ ] `internal/store.PricePositive` *(method)* — `backend/internal/store/pricing.go:35`
- [ ] `internal/store.PriceValid` *(method)* — `backend/internal/store/pricing.go:41`
- [ ] `internal/store.EndsAfterStarts` *(method)* — `backend/internal/store/pricing.go:47`
- [ ] `internal/store.Problems` *(method)* — `backend/internal/store/pricing.go:55`
- [ ] `internal/store.Valid` *(method)* — `backend/internal/store/pricing.go:80`
- [ ] `internal/store.Active` *(method)* — `backend/internal/store/pricing.go:86`
- [ ] `internal/store.SaleActive` *(method)* — `backend/internal/store/pricing.go:95`
- [ ] `internal/store.EffectivePriceCents` *(method)* — `backend/internal/store/pricing.go:100`
- [ ] `internal/store.scanCategories` — `backend/internal/store/products.go:49`
- [ ] `internal/store.Categories` *(method)* — `backend/internal/store/products.go:21`
- [ ] `internal/store.PublicCategories` *(method)* — `backend/internal/store/products.go:35`
- [ ] `internal/store.CategoryBySlugTx` *(method)* — `backend/internal/store/products.go:115`
- [ ] `internal/store.CategoryBySlug` *(method)* — `backend/internal/store/products.go:62`
- [ ] `internal/store.UpsertCategory` *(method)* — `backend/internal/store/products.go:69`
- [ ] `internal/store.ReconcileProductCategories` *(method)* — `backend/internal/store/products.go:96`
- [ ] `internal/store.CountProducts` *(method)* — `backend/internal/store/products.go:127`
- [ ] `internal/store.decodeJSON` — `backend/internal/store/types.go:184`
- [ ] `internal/store.decodeStringListMap` — `backend/internal/store/types.go:209`
- [ ] `internal/store.scanProduct` — `backend/internal/store/store.go:45`
- [ ] `internal/store.ListProducts` *(method)* — `backend/internal/store/products.go:133`
- [ ] `internal/store.ProductBySlug` *(method)* — `backend/internal/store/products.go:203`
- [ ] `internal/store.ProductByIDTx` *(method)* — `backend/internal/store/products.go:220`
- [ ] `internal/store.ProductByID` *(method)* — `backend/internal/store/products.go:216`
- [ ] `internal/store.DecrementStock` *(method)* — `backend/internal/store/products.go:233`
- [ ] `internal/store.IncrementStock` *(method)* — `backend/internal/store/products.go:248`
- [ ] `internal/store.RelatedProducts` *(method)* — `backend/internal/store/products.go:262`
- [ ] `internal/store.boolInt` — `backend/internal/store/products.go:362`
- [ ] `internal/store.encodeJSON` — `backend/internal/store/types.go:173`
- [ ] `internal/store.encodeStringListMap` — `backend/internal/store/types.go:198`
- [ ] `internal/store.CreateProduct` *(method)* — `backend/internal/store/products.go:289`
- [ ] `internal/store.UpdateProduct` *(method)* — `backend/internal/store/products.go:313`
- [ ] `internal/store.SetProductActive` *(method)* — `backend/internal/store/products.go:344`
- [ ] `internal/store.CreateSession` *(method)* — `backend/internal/store/sessions.go:10`
- [ ] `internal/store.SessionUserByHash` *(method)* — `backend/internal/store/sessions.go:23`
- [ ] `internal/store.DeleteSessionByHash` *(method)* — `backend/internal/store/sessions.go:51`
- [ ] `internal/store.DeleteSessionsForUserAgent` *(method)* — `backend/internal/store/sessions.go:56`
- [ ] `internal/store.DeleteExpiredSessions` *(method)* — `backend/internal/store/sessions.go:66`
- [ ] `internal/store.PluginSettings` *(method)* — `backend/internal/store/settings.go:9`
- [ ] `internal/store.SavePluginSettings` *(method)* — `backend/internal/store/settings.go:20`
- [ ] `internal/store.storefrontSettings` *(method)* — `backend/internal/store/settings.go:42`
- [ ] `internal/store.StorefrontSettings` *(method)* — `backend/internal/store/settings.go:32`
- [ ] `internal/store.StorefrontSettingsTx` *(method)* — `backend/internal/store/settings.go:38`
- [ ] `internal/store.SaveStorefrontSettings` *(method)* — `backend/internal/store/settings.go:53`
- [ ] `internal/store.New` — `backend/internal/store/store.go:15`
- [ ] `internal/store.DB` *(method)* — `backend/internal/store/store.go:19`
- [ ] `internal/store.InTx` *(method)* — `backend/internal/store/store.go:27`
- [ ] `internal/store.NullTime` — `backend/internal/store/types.go:165`
- [ ] `internal/store.CountUsers` *(method)* — `backend/internal/store/users.go:10`
- [ ] `internal/store.defaultPreset` — `backend/internal/store/users.go:157`
- [ ] `internal/store.CreateUser` *(method)* — `backend/internal/store/users.go:16`
- [ ] `internal/store.scanUser` *(method)* — `backend/internal/store/users.go:94`
- [ ] `internal/store.UserByEmail` *(method)* — `backend/internal/store/users.go:29`
- [ ] `internal/store.UserByID` *(method)* — `backend/internal/store/users.go:36`
- [ ] `internal/store.ListUsers` *(method)* — `backend/internal/store/users.go:45`
- [ ] `internal/store.UpdateUserRole` *(method)* — `backend/internal/store/users.go:67`
- [ ] `internal/store.CountAdmins` *(method)* — `backend/internal/store/users.go:85`
- [ ] `internal/store.UpdateTheme` *(method)* — `backend/internal/store/users.go:107`
- [ ] `internal/store.ListCustomers` *(method)* — `backend/internal/store/users.go:125`

### `internal/auditbatch` (1 functions)

- [ ] `internal/auditbatch.Run` — `backend/internal/auditbatch/batcher.go:18`

### `internal/config` (9 functions)

- [x] `internal/config.IsProduction` *(method)* — `backend/internal/config/config.go:218`
- [x] `internal/config.getbool` — `backend/internal/config/config.go:240`
- [x] `internal/config.getenv` — `backend/internal/config/config.go:233`
- [x] `internal/config.getint` — `backend/internal/config/config.go:252`
- [x] `internal/config.Load` — `backend/internal/config/config.go:42`
- [x] `internal/config.DSN` *(method)* — `backend/internal/config/config.go:187`
- [x] `internal/config.CourierCredentialValues` *(method)* — `backend/internal/config/config.go:206`
- [x] `internal/config.IsDevelopment` *(method)* — `backend/internal/config/config.go:222`
- [x] `internal/config.AdminSessionTTL` *(method)* — `backend/internal/config/config.go:226`

### `internal/courier` (44 functions)

- [ ] `internal/courier.Error` *(method)* — `backend/internal/courier/courier.go:104`
- [ ] `internal/courier.get` *(method)* — `backend/internal/courier/courier.go:157`
- [ ] `internal/courier.stale` *(method)* — `backend/internal/courier/courier.go:163`
- [ ] `internal/courier.set` *(method)* — `backend/internal/courier/courier.go:169`
- [ ] `internal/courier.newDZShip` — `backend/internal/courier/dzship.go:25`
- [ ] `internal/courier.New` — `backend/internal/courier/courier.go:177`
- [ ] `internal/courier.isOffline` *(method)* — `backend/internal/courier/courier.go:381`
- [ ] `internal/courier.markOffline` *(method)* — `backend/internal/courier/courier.go:390`
- [ ] `internal/courier.notifyFallback` *(method)* — `backend/internal/courier/courier.go:369`
- [ ] `internal/courier.resolveMap` — `backend/internal/courier/courier.go:290`
- [ ] `internal/courier.Wilayas` *(method)* — `backend/internal/courier/courier.go:204`
- [ ] `internal/courier.Wilaya` *(method)* — `backend/internal/courier/courier.go:209`
- [ ] `internal/courier.communeCache` *(method)* — `backend/internal/courier/courier.go:347`
- [ ] `internal/courier.apiError` — `backend/internal/courier/dzship.go:231`
- [ ] `internal/courier.doJSON` *(method)* — `backend/internal/courier/dzship.go:144`
- [ ] `internal/courier.communes` *(method)* — `backend/internal/courier/dzship.go:51`
- [ ] `internal/courier.fallbackCommunesFor` — `backend/internal/courier/snapshot.go:79`
- [ ] `internal/courier.communes` *(method)* — `backend/internal/courier/snapshot.go:59`
- [ ] `internal/courier.Communes` *(method)* — `backend/internal/courier/courier.go:222`
- [ ] `internal/courier.Commune` *(method)* — `backend/internal/courier/courier.go:229`
- [ ] `internal/courier.deskCache` *(method)* — `backend/internal/courier/courier.go:358`
- [ ] `internal/courier.credentialsOrNil` *(method)* — `backend/internal/courier/dzship.go:130`
- [ ] `internal/courier.options` *(method)* — `backend/internal/courier/dzship.go:137`
- [ ] `internal/courier.desks` *(method)* — `backend/internal/courier/dzship.go:63`
- [ ] `internal/courier.localDesks` — `backend/internal/courier/snapshot.go:90`
- [ ] `internal/courier.desks` *(method)* — `backend/internal/courier/snapshot.go:67`
- [ ] `internal/courier.Desks` *(method)* — `backend/internal/courier/courier.go:243`
- [ ] `internal/courier.Desk` *(method)* — `backend/internal/courier/courier.go:250`
- [ ] `internal/courier.createOrder` *(method)* — `backend/internal/courier/dzship.go:80`
- [ ] `internal/courier.sanitizeTracking` — `backend/internal/courier/snapshot.go:132`
- [ ] `internal/courier.localShipment` — `backend/internal/courier/snapshot.go:120`
- [ ] `internal/courier.createOrder` *(method)* — `backend/internal/courier/snapshot.go:71`
- [ ] `internal/courier.isUnavailable` — `backend/internal/courier/courier.go:399`
- [ ] `internal/courier.sandboxUnavailable` *(method)* — `backend/internal/courier/courier.go:377`
- [ ] `internal/courier.resolveCall` — `backend/internal/courier/courier.go:324`
- [ ] `internal/courier.CreateOrder` *(method)* — `backend/internal/courier/courier.go:263`
- [ ] `internal/courier.track` *(method)* — `backend/internal/courier/dzship.go:111`
- [ ] `internal/courier.localTracking` — `backend/internal/courier/snapshot.go:145`
- [ ] `internal/courier.track` *(method)* — `backend/internal/courier/snapshot.go:75`
- [ ] `internal/courier.Track` *(method)* — `backend/internal/courier/courier.go:280`
- [ ] `internal/courier.wilayas` *(method)* — `backend/internal/courier/dzship.go:40`
- [ ] `internal/courier.loadWilayaSnapshot` — `backend/internal/courier/snapshot.go:27`
- [ ] `internal/courier.loadCommuneSnapshot` — `backend/internal/courier/snapshot.go:35`
- [ ] `internal/courier.wilayas` *(method)* — `backend/internal/courier/snapshot.go:51`

### `migrations` (0 functions)


### `internal/db` (5 functions)

- [ ] `internal/db.Open` — `backend/internal/db/db.go:17`
- [ ] `internal/db.migrationApplied` — `backend/internal/db/db.go:92`
- [ ] `internal/db.splitStatements` — `backend/internal/db/db.go:103`
- [ ] `internal/db.Migrate` — `backend/internal/db/db.go:35`
- [ ] `internal/db.MigratePlugins` — `backend/internal/db/plugin_migrations.go:20`

### `internal/auth` (6 functions)

- [ ] `internal/auth.HashPassword` — `backend/internal/auth/password.go:26`
- [ ] `internal/auth.VerifyPassword` — `backend/internal/auth/password.go:39`
- [ ] `internal/auth.DummyHash` — `backend/internal/auth/password.go:102`
- [ ] `internal/auth.VerifyDummy` — `backend/internal/auth/password.go:107`
- [ ] `internal/auth.NewToken` — `backend/internal/auth/token.go:14`
- [ ] `internal/auth.HashToken` — `backend/internal/auth/token.go:24`

### `internal/ids` (1 functions)

- [x] `internal/ids.New` — `backend/internal/ids/ids.go:9`

### `internal/orderstatus` (2 functions)

- [x] `internal/orderstatus.Known` — `backend/internal/orderstatus/orderstatus.go:23`
- [x] `internal/orderstatus.CanTransition` — `backend/internal/orderstatus/orderstatus.go:28`

### `internal/secrets` (3 functions)

- [x] `internal/secrets.New` — `backend/internal/secrets/box.go:25`
- [x] `internal/secrets.Encrypt` *(method)* — `backend/internal/secrets/box.go:29`
- [x] `internal/secrets.Decrypt` *(method)* — `backend/internal/secrets/box.go:46`

### `internal/plugins` (41 functions)

- [ ] `internal/plugins.IsAdmin` *(method)* — `backend/internal/plugins/plugin.go:176`
- [ ] `internal/plugins.writeJSON` — `backend/internal/plugins/plugin.go:244`
- [ ] `internal/plugins.JSON` *(method)* — `backend/internal/plugins/plugin.go:180`
- [ ] `internal/plugins.Problem` *(method)* — `backend/internal/plugins/plugin.go:188`
- [ ] `internal/plugins.Error` *(method)* — `backend/internal/plugins/plugin.go:184`
- [ ] `internal/plugins.Settings` *(method)* — `backend/internal/plugins/plugin.go:196`
- [ ] `internal/plugins.NewProblem` — `backend/internal/plugins/plugin.go:207`
- [ ] `internal/plugins.Get` *(method)* — `backend/internal/plugins/registry.go:130`
- [ ] `internal/plugins.DecodeBody` — `backend/internal/plugins/plugin.go:211`
- [ ] `internal/plugins.NewRegistry` — `backend/internal/plugins/registry.go:20`
- [ ] `internal/plugins.validMethod` — `backend/internal/plugins/registry.go:508`
- [ ] `internal/plugins.validateCategories` — `backend/internal/plugins/registry.go:433`
- [ ] `internal/plugins.validateProductType` — `backend/internal/plugins/registry.go:407`
- [ ] `internal/plugins.validateSettings` — `backend/internal/plugins/registry.go:479`
- [ ] `internal/plugins.validateStorefrontPages` — `backend/internal/plugins/registry.go:453`
- [ ] `internal/plugins.Register` *(method)* — `backend/internal/plugins/registry.go:27`
- [ ] `internal/plugins.All` *(method)* — `backend/internal/plugins/registry.go:124`
- [ ] `internal/plugins.ValidateDependencies` *(method)* — `backend/internal/plugins/registry.go:137`
- [ ] `internal/plugins.SetStates` *(method)* — `backend/internal/plugins/registry.go:183`
- [ ] `internal/plugins.SetEnabled` *(method)* — `backend/internal/plugins/registry.go:195`
- [ ] `internal/plugins.enabledLocked` *(method)* — `backend/internal/plugins/registry.go:214`
- [ ] `internal/plugins.Enabled` *(method)* — `backend/internal/plugins/registry.go:207`
- [ ] `internal/plugins.effectiveEnabledLocked` *(method)* — `backend/internal/plugins/registry.go:235`
- [ ] `internal/plugins.EffectiveEnabled` *(method)* — `backend/internal/plugins/registry.go:228`
- [ ] `internal/plugins.dependentsLocked` *(method)* — `backend/internal/plugins/registry.go:266`
- [ ] `internal/plugins.Dependents` *(method)* — `backend/internal/plugins/registry.go:259`
- [ ] `internal/plugins.EnabledDependents` *(method)* — `backend/internal/plugins/registry.go:291`
- [ ] `internal/plugins.blockedByLocked` *(method)* — `backend/internal/plugins/registry.go:313`
- [ ] `internal/plugins.BlockedBy` *(method)* — `backend/internal/plugins/registry.go:306`
- [ ] `internal/plugins.Manifests` *(method)* — `backend/internal/plugins/registry.go:334`
- [ ] `internal/plugins.Migrations` *(method)* — `backend/internal/plugins/registry.go:347`
- [ ] `internal/plugins.ProductType` *(method)* — `backend/internal/plugins/registry.go:364`
- [ ] `internal/plugins.CheckoutHook` *(method)* — `backend/internal/plugins/registry.go:372`
- [ ] `internal/plugins.OrderStatusHooks` *(method)* — `backend/internal/plugins/registry.go:381`
- [ ] `internal/plugins.EnrichProduct` *(method)* — `backend/internal/plugins/registry.go:391`
- [ ] `internal/plugins.SettingsWithTime` *(method)* — `backend/internal/plugins/settings.go:21`
- [ ] `internal/plugins.Settings` *(method)* — `backend/internal/plugins/settings.go:16`
- [ ] `internal/plugins.EncodeSettings` *(method)* — `backend/internal/plugins/settings.go:44`
- [ ] `internal/plugins.MergeSettings` — `backend/internal/plugins/settings.go:58`
- [ ] `internal/plugins.MaskSettings` — `backend/internal/plugins/settings.go:131`
- [ ] `internal/plugins.SettingsComplete` — `backend/internal/plugins/settings.go:151`

### `internal/orderintake` (17 functions)

- [ ] `internal/orderintake.normalizeOptions` — `backend/internal/orderintake/lines.go:54`
- [ ] `internal/orderintake.optionsKey` — `backend/internal/orderintake/lines.go:65`
- [ ] `internal/orderintake.BuildLines` — `backend/internal/orderintake/lines.go:20`
- [ ] `internal/orderintake.OptionSchema` — `backend/internal/orderintake/options.go:11`
- [ ] `internal/orderintake.selectedOptionFields` — `backend/internal/orderintake/options.go:23`
- [ ] `internal/orderintake.optionLabel` — `backend/internal/orderintake/options.go:67`
- [ ] `internal/orderintake.New` — `backend/internal/orderintake/orderintake.go:35`
- [ ] `internal/orderintake.CheckoutChannel` — `backend/internal/orderintake/orderintake.go:102`
- [ ] `internal/orderintake.CounterChannel` — `backend/internal/orderintake/orderintake.go:120`
- [ ] `internal/orderintake.refuseFields` — `backend/internal/orderintake/problems.go:21`
- [ ] `internal/orderintake.shippingCents` *(method)* — `backend/internal/orderintake/orderintake.go:410`
- [ ] `internal/orderintake.Place` *(method)* — `backend/internal/orderintake/orderintake.go:144`
- [ ] `internal/orderintake.refuse` — `backend/internal/orderintake/problems.go:17`
- [ ] `internal/orderintake.ChangeStatus` *(method)* — `backend/internal/orderintake/orderintake.go:335`
- [ ] `internal/orderintake.Error` *(method)* — `backend/internal/orderintake/problems.go:13`
- [ ] `internal/orderintake.ShippingCents` — `backend/internal/orderintake/shipping.go:13`
- [ ] `internal/orderintake.NoShipping` — `backend/internal/orderintake/shipping.go:21`

### `internal/productrules` (9 functions)

- [ ] `internal/productrules.NormalizeAttributes` — `backend/internal/productrules/attributes.go:50`
- [ ] `internal/productrules.PlainText` — `backend/internal/productrules/productrules.go:74`
- [ ] `internal/productrules.validOptionValue` — `backend/internal/productrules/attributes.go:127`
- [ ] `internal/productrules.ValidateAttributes` — `backend/internal/productrules/attributes.go:75`
- [ ] `internal/productrules.ValidName` — `backend/internal/productrules/productrules.go:36`
- [ ] `internal/productrules.ValidPrice` — `backend/internal/productrules/productrules.go:42`
- [ ] `internal/productrules.ValidStock` — `backend/internal/productrules/productrules.go:47`
- [ ] `internal/productrules.ValidImageID` — `backend/internal/productrules/productrules.go:52`
- [ ] `internal/productrules.ValidImagePath` — `backend/internal/productrules/productrules.go:59`

### `internal/storefront` (13 functions)

- [ ] `internal/storefront.Default` — `backend/internal/storefront/theme.go:64`
- [ ] `internal/storefront.normalizePalette` — `backend/internal/storefront/theme.go:157`
- [ ] `internal/storefront.Normalize` — `backend/internal/storefront/theme.go:124`
- [ ] `internal/storefront.Parse` — `backend/internal/storefront/theme.go:111`
- [ ] `internal/storefront.Encode` — `backend/internal/storefront/theme.go:167`
- [ ] `internal/storefront.contains` — `backend/internal/storefront/theme.go:332`
- [ ] `internal/storefront.hasControl` — `backend/internal/storefront/theme.go:320`
- [ ] `internal/storefront.validImagePath` — `backend/internal/storefront/theme.go:313`
- [ ] `internal/storefront.luminance` — `backend/internal/storefront/theme.go:295`
- [ ] `internal/storefront.ContrastRatio` — `backend/internal/storefront/theme.go:282`
- [ ] `internal/storefront.validatePalette` — `backend/internal/storefront/theme.go:247`
- [ ] `internal/storefront.Validate` — `backend/internal/storefront/theme.go:176`
- [ ] `internal/storefront.ParseLenient` — `backend/internal/storefront/theme.go:341`

### `internal/httpapi` (156 functions)

- [ ] `internal/httpapi.newReadCache` — `backend/internal/httpapi/cache.go:35`
- [ ] `internal/httpapi.get` *(method)* — `backend/internal/httpapi/cache.go:42`
- [ ] `internal/httpapi.put` *(method)* — `backend/internal/httpapi/cache.go:57`
- [ ] `internal/httpapi.clear` *(method)* — `backend/internal/httpapi/cache.go:83`
- [ ] `internal/httpapi.size` *(method)* — `backend/internal/httpapi/cache.go:90`
- [ ] `internal/httpapi.WriteHeader` *(method)* — `backend/internal/httpapi/cache.go:104`
- [ ] `internal/httpapi.WriteHeader` *(method)* — `backend/internal/httpapi/middleware.go:74`
- [ ] `internal/httpapi.Write` *(method)* — `backend/internal/httpapi/cache.go:113`
- [ ] `internal/httpapi.Write` *(method)* — `backend/internal/httpapi/middleware.go:83`
- [ ] `internal/httpapi.requestID` — `backend/internal/httpapi/middleware.go:33`
- [ ] `internal/httpapi.cacheRead` *(method)* — `backend/internal/httpapi/cache.go:125`
- [ ] `internal/httpapi.invalidateReadCacheOnWrite` *(method)* — `backend/internal/httpapi/cache.go:166`
- [ ] `internal/httpapi.sweepLocked` *(method)* — `backend/internal/httpapi/captcha.go:134`
- [ ] `internal/httpapi.cleanupLoop` *(method)* — `backend/internal/httpapi/captcha.go:119`
- [ ] `internal/httpapi.cleanup` *(method)* — `backend/internal/httpapi/ratelimit.go:90`
- [ ] `internal/httpapi.cleanupLoop` *(method)* — `backend/internal/httpapi/ratelimit.go:77`
- [ ] `internal/httpapi.newCaptchaStore` — `backend/internal/httpapi/captcha.go:43`
- [ ] `internal/httpapi.currentAuth` — `backend/internal/httpapi/middleware.go:311`
- [ ] `internal/httpapi.newRateLimiter` — `backend/internal/httpapi/ratelimit.go:27`
- [ ] `internal/httpapi.Handler` *(method)* — `backend/internal/httpapi/router.go:82`
- [ ] `internal/httpapi.problem` — `backend/internal/httpapi/respond.go:26`
- [ ] `internal/httpapi.writeJSON` — `backend/internal/httpapi/respond.go:44`
- [ ] `internal/httpapi.writeProblem` — `backend/internal/httpapi/respond.go:55`
- [ ] `internal/httpapi.writeError` — `backend/internal/httpapi/respond.go:63`
- [ ] `internal/httpapi.pluginHandler` *(method)* — `backend/internal/httpapi/router.go:209`
- [ ] `internal/httpapi.mountPluginRoutes` *(method)* — `backend/internal/httpapi/router.go:183`
- [ ] `internal/httpapi.allow` *(method)* — `backend/internal/httpapi/ratelimit.go:38`
- [ ] `internal/httpapi.clientIP` — `backend/internal/httpapi/respond.go:120`
- [ ] `internal/httpapi.rateLimit` *(method)* — `backend/internal/httpapi/ratelimit.go:104`
- [ ] `internal/httpapi.routes` *(method)* — `backend/internal/httpapi/router.go:95`
- [ ] `internal/httpapi.New` — `backend/internal/httpapi/router.go:36`
- [ ] `internal/httpapi.drawCaptchaGlyph` — `backend/internal/httpapi/captcha.go:235`
- [ ] `internal/httpapi.absInt` — `backend/internal/httpapi/captcha.go:268`
- [ ] `internal/httpapi.drawCaptchaLine` — `backend/internal/httpapi/captcha.go:250`
- [ ] `internal/httpapi.captchaImage` — `backend/internal/httpapi/captcha.go:181`
- [ ] `internal/httpapi.evictOldestLocked` *(method)* — `backend/internal/httpapi/captcha.go:142`
- [ ] `internal/httpapi.randomCaptchaCode` — `backend/internal/httpapi/captcha.go:160`
- [ ] `internal/httpapi.randomCaptchaToken` — `backend/internal/httpapi/captcha.go:173`
- [ ] `internal/httpapi.issue` *(method)* — `backend/internal/httpapi/captcha.go:53`
- [ ] `internal/httpapi.verify` *(method)* — `backend/internal/httpapi/captcha.go:85`
- [ ] `internal/httpapi.close` *(method)* — `backend/internal/httpapi/captcha.go:156`
- [ ] `internal/httpapi.captchaErrorMessage` — `backend/internal/httpapi/captcha.go:310`
- [ ] `internal/httpapi.handleCheckoutChallenge` *(method)* — `backend/internal/httpapi/captcha.go:321`
- [ ] `internal/httpapi.nonNil` — `backend/internal/httpapi/dto.go:260`
- [ ] `internal/httpapi.nonNilStringListMap` — `backend/internal/httpapi/dto.go:125`
- [ ] `internal/httpapi.productDTOFromStore` — `backend/internal/httpapi/dto.go:58`
- [ ] `internal/httpapi.optionSchema` *(method)* — `backend/internal/httpapi/product_options.go:13`
- [ ] `internal/httpapi.decorateProduct` *(method)* — `backend/internal/httpapi/dto.go:112`
- [ ] `internal/httpapi.toProduct` *(method)* — `backend/internal/httpapi/dto.go:95`
- [ ] `internal/httpapi.toProducts` *(method)* — `backend/internal/httpapi/dto.go:103`
- [ ] `internal/httpapi.toUser` — `backend/internal/httpapi/dto.go:139`
- [ ] `internal/httpapi.nonNilStringMap` — `backend/internal/httpapi/dto.go:267`
- [ ] `internal/httpapi.toOrderItems` — `backend/internal/httpapi/dto.go:240`
- [ ] `internal/httpapi.toOrder` — `backend/internal/httpapi/dto.go:196`
- [ ] `internal/httpapi.fmtTime` — `backend/internal/httpapi/dto.go:274`
- [ ] `internal/httpapi.problemFields` — `backend/internal/httpapi/respond.go:30`
- [ ] `internal/httpapi.runeLen` — `backend/internal/httpapi/validate.go:13`
- [ ] `internal/httpapi.handleAdminOrders` *(method)* — `backend/internal/httpapi/handlers_admin.go:19`
- [ ] `internal/httpapi.writeIntakeError` — `backend/internal/httpapi/respond.go:69`
- [ ] `internal/httpapi.handleOrderStatus` *(method)* — `backend/internal/httpapi/handlers_admin.go:60`
- [ ] `internal/httpapi.handleCustomers` *(method)* — `backend/internal/httpapi/handlers_admin.go:93`
- [ ] `internal/httpapi.handleAdminTeam` *(method)* — `backend/internal/httpapi/handlers_admin.go:127`
- [ ] `internal/httpapi.handleUpdateTeamRole` *(method)* — `backend/internal/httpapi/handlers_admin.go:149`
- [ ] `internal/httpapi.knownPreset` — `backend/internal/httpapi/presets.go:7`
- [ ] `internal/httpapi.handleGetPreferences` *(method)* — `backend/internal/httpapi/handlers_admin.go:220`
- [ ] `internal/httpapi.handlePutPreferences` *(method)* — `backend/internal/httpapi/handlers_admin.go:233`
- [ ] `internal/httpapi.handleAdminProducts` *(method)* — `backend/internal/httpapi/handlers_admin_products.go:68`
- [ ] `internal/httpapi.Error` *(method)* — `backend/internal/httpapi/respond.go:22`
- [ ] `internal/httpapi.isUniqueViolation` — `backend/internal/httpapi/handlers_auth.go:240`
- [ ] `internal/httpapi.normalizeProductAttributes` — `backend/internal/httpapi/product_options.go:103`
- [ ] `internal/httpapi.parseSaleWindow` — `backend/internal/httpapi/handlers_admin_products.go:497`
- [ ] `internal/httpapi.slugTaken` *(method)* — `backend/internal/httpapi/handlers_admin_products.go:486`
- [ ] `internal/httpapi.trimAll` — `backend/internal/httpapi/validate.go:83`
- [ ] `internal/httpapi.validSlug` — `backend/internal/httpapi/validate.go:28`
- [ ] `internal/httpapi.validateProduct` — `backend/internal/httpapi/handlers_admin_products.go:558`
- [ ] `internal/httpapi.attributeFields` — `backend/internal/httpapi/product_options.go:74`
- [ ] `internal/httpapi.optionKeyFields` — `backend/internal/httpapi/product_options.go:60`
- [ ] `internal/httpapi.productOptionFields` — `backend/internal/httpapi/product_options.go:48`
- [ ] `internal/httpapi.validateProductAttributes` *(method)* — `backend/internal/httpapi/product_options.go:20`
- [ ] `internal/httpapi.handleCreateProduct` *(method)* — `backend/internal/httpapi/handlers_admin_products.go:80`
- [ ] `internal/httpapi.hasAny` — `backend/internal/httpapi/handlers_admin_products.go:549`
- [ ] `internal/httpapi.filterFieldProblems` — `backend/internal/httpapi/handlers_admin_products.go:534`
- [ ] `internal/httpapi.handleUpdateProduct` *(method)* — `backend/internal/httpapi/handlers_admin_products.go:186`
- [ ] `internal/httpapi.handleArchiveProduct` *(method)* — `backend/internal/httpapi/handlers_admin_products.go:450`
- [ ] `internal/httpapi.normalizedAlgerianMobile` — `backend/internal/httpapi/validate.go:39`
- [ ] `internal/httpapi.validAlgerianMobile` — `backend/internal/httpapi/validate.go:67`
- [ ] `internal/httpapi.handleCounterSale` *(method)* — `backend/internal/httpapi/handlers_admin_sales.go:28`
- [ ] `internal/httpapi.newSession` *(method)* — `backend/internal/httpapi/handlers_auth.go:218`
- [ ] `internal/httpapi.setSessionCookie` *(method)* — `backend/internal/httpapi/middleware.go:220`
- [ ] `internal/httpapi.validEmail` — `backend/internal/httpapi/validate.go:17`
- [ ] `internal/httpapi.handleRegister` *(method)* — `backend/internal/httpapi/handlers_auth.go:21`
- [ ] `internal/httpapi.handleLogin` *(method)* — `backend/internal/httpapi/handlers_auth.go:106`
- [ ] `internal/httpapi.clearSessionCookie` *(method)* — `backend/internal/httpapi/middleware.go:233`
- [ ] `internal/httpapi.handleLogout` *(method)* — `backend/internal/httpapi/handlers_auth.go:170`
- [ ] `internal/httpapi.handleMe` *(method)* — `backend/internal/httpapi/handlers_auth.go:181`
- [ ] `internal/httpapi.handleAccountOrders` *(method)* — `backend/internal/httpapi/handlers_auth.go:191`
- [ ] `internal/httpapi.handleCategories` *(method)* — `backend/internal/httpapi/handlers_catalog.go:14`
- [ ] `internal/httpapi.handleProducts` *(method)* — `backend/internal/httpapi/handlers_catalog.go:41`
- [ ] `internal/httpapi.handleProduct` *(method)* — `backend/internal/httpapi/handlers_catalog.go:66`
- [ ] `internal/httpapi.optionalUser` *(method)* — `backend/internal/httpapi/middleware.go:318`
- [ ] `internal/httpapi.resolveDestination` *(method)* — `backend/internal/httpapi/handlers_shipping.go:69`
- [ ] `internal/httpapi.close` *(method)* — `backend/internal/httpapi/ratelimit.go:100`
- [ ] `internal/httpapi.Close` *(method)* — `backend/internal/httpapi/router.go:86`
- [ ] `internal/httpapi.verifyTurnstile` — `backend/internal/httpapi/turnstile.go:25`
- [ ] `internal/httpapi.handleCheckout` *(method)* — `backend/internal/httpapi/handlers_checkout.go:46`
- [ ] `internal/httpapi.handleOrderByToken` *(method)* — `backend/internal/httpapi/handlers_checkout.go:223`
- [ ] `internal/httpapi.newCSRFToken` — `backend/internal/httpapi/handlers_client.go:64`
- [ ] `internal/httpapi.validCSRFToken` — `backend/internal/httpapi/handlers_client.go:72`
- [ ] `internal/httpapi.handleCSRF` *(method)* — `backend/internal/httpapi/handlers_client.go:20`
- [ ] `internal/httpapi.handleHealth` *(method)* — `backend/internal/httpapi/handlers_client.go:54`
- [ ] `internal/httpapi.handlePluginManifests` *(method)* — `backend/internal/httpapi/handlers_framework.go:47`
- [ ] `internal/httpapi.pluginSettings` *(method)* — `backend/internal/httpapi/handlers_framework.go:307`
- [ ] `internal/httpapi.adminPluginDTO` *(method)* — `backend/internal/httpapi/handlers_framework.go:76`
- [ ] `internal/httpapi.handleAdminPlugins` *(method)* — `backend/internal/httpapi/handlers_framework.go:68`
- [ ] `internal/httpapi.pluginFromPath` *(method)* — `backend/internal/httpapi/handlers_framework.go:299`
- [ ] `internal/httpapi.pluginNames` *(method)* — `backend/internal/httpapi/handlers_framework.go:177`
- [ ] `internal/httpapi.handlePluginState` *(method)* — `backend/internal/httpapi/handlers_framework.go:103`
- [ ] `internal/httpapi.handleGetPluginSettings` *(method)* — `backend/internal/httpapi/handlers_framework.go:190`
- [ ] `internal/httpapi.changedSettingKeys` — `backend/internal/httpapi/handlers_framework.go:311`
- [ ] `internal/httpapi.handlePutPluginSettings` *(method)* — `backend/internal/httpapi/handlers_framework.go:220`
- [ ] `internal/httpapi.walkThemeFields` — `backend/internal/httpapi/handlers_framework.go:329`
- [ ] `internal/httpapi.changedThemeFields` — `backend/internal/httpapi/handlers_framework.go:322`
- [ ] `internal/httpapi.loadTheme` *(method)* — `backend/internal/httpapi/handlers_framework.go:353`
- [ ] `internal/httpapi.handleStorefrontTheme` *(method)* — `backend/internal/httpapi/handlers_framework.go:366`
- [ ] `internal/httpapi.handleAdminStorefrontTheme` *(method)* — `backend/internal/httpapi/handlers_framework.go:375`
- [ ] `internal/httpapi.handlePutStorefrontTheme` *(method)* — `backend/internal/httpapi/handlers_framework.go:387`
- [ ] `internal/httpapi.handleAdminProductImage` *(method)* — `backend/internal/httpapi/handlers_images.go:21`
- [ ] `internal/httpapi.handleProductImage` *(method)* — `backend/internal/httpapi/handlers_images.go:95`
- [ ] `internal/httpapi.handleMetrics` *(method)* — `backend/internal/httpapi/handlers_metrics.go:71`
- [ ] `internal/httpapi.writeDeliveryMapError` *(method)* — `backend/internal/httpapi/handlers_shipping.go:308`
- [ ] `internal/httpapi.handleShippingWilayas` *(method)* — `backend/internal/httpapi/handlers_shipping.go:27`
- [ ] `internal/httpapi.parseWilayaCode` — `backend/internal/httpapi/handlers_shipping.go:357`
- [ ] `internal/httpapi.handleShippingCommunes` *(method)* — `backend/internal/httpapi/handlers_shipping.go:36`
- [ ] `internal/httpapi.handleShippingDesks` *(method)* — `backend/internal/httpapi/handlers_shipping.go:50`
- [ ] `internal/httpapi.truncateRunes` — `backend/internal/httpapi/validate.go:72`
- [ ] `internal/httpapi.writeCourierError` *(method)* — `backend/internal/httpapi/handlers_shipping.go:326`
- [ ] `internal/httpapi.courierCall` — `backend/internal/httpapi/handlers_shipping.go:316`
- [ ] `internal/httpapi.currentUserID` — `backend/internal/httpapi/handlers_shipping.go:375`
- [ ] `internal/httpapi.orderProductList` — `backend/internal/httpapi/handlers_shipping.go:367`
- [ ] `internal/httpapi.handleCreateShipment` *(method)* — `backend/internal/httpapi/handlers_shipping.go:116`
- [ ] `internal/httpapi.handleRefreshTracking` *(method)* — `backend/internal/httpapi/handlers_shipping.go:239`
- [ ] `internal/httpapi.securityHeaders` *(method)* — `backend/internal/httpapi/middleware.go:38`
- [ ] `internal/httpapi.withRequestID` *(method)* — `backend/internal/httpapi/middleware.go:52`
- [ ] `internal/httpapi.redactedPath` — `backend/internal/httpapi/middleware.go:113`
- [ ] `internal/httpapi.logRequests` *(method)* — `backend/internal/httpapi/middleware.go:90`
- [ ] `internal/httpapi.recoverPanic` *(method)* — `backend/internal/httpapi/middleware.go:120`
- [ ] `internal/httpapi.limitBody` *(method)* — `backend/internal/httpapi/middleware.go:139`
- [ ] `internal/httpapi.requireChallengeHeader` *(method)* — `backend/internal/httpapi/middleware.go:152`
- [ ] `internal/httpapi.isLocalDevelopmentOrigin` — `backend/internal/httpapi/middleware.go:207`
- [ ] `internal/httpapi.enforceCSRF` *(method)* — `backend/internal/httpapi/middleware.go:169`
- [ ] `internal/httpapi.requireUser` *(method)* — `backend/internal/httpapi/middleware.go:246`
- [ ] `internal/httpapi.requireAdmin` *(method)* — `backend/internal/httpapi/middleware.go:279`
- [ ] `internal/httpapi.requireStaff` *(method)* — `backend/internal/httpapi/middleware.go:296`
- [ ] `internal/httpapi.adminMutationLimit` *(method)* — `backend/internal/httpapi/ratelimit.go:122`
- [ ] `internal/httpapi.decodeJSON` — `backend/internal/httpapi/respond.go:78`
- [ ] `internal/httpapi.validPhone` — `backend/internal/httpapi/validate.go:32`

### `internal/plugins/ai` (20 functions)

- [ ] `internal/plugins/ai.New` — `backend/internal/plugins/ai/ai.go:44`
- [ ] `internal/plugins/ai.Manifest` *(method)* — `backend/internal/plugins/ai/ai.go:48`
- [ ] `internal/plugins/ai.ValidateSettings` *(method)* — `backend/internal/plugins/ai/ai.go:97`
- [ ] `internal/plugins/ai.Migrations` *(method)* — `backend/internal/plugins/ai/ai.go:113`
- [ ] `internal/plugins/ai.Routes` *(method)* — `backend/internal/plugins/ai/ai.go:129`
- [ ] `internal/plugins/ai.Error` *(method)* — `backend/internal/plugins/ai/client.go:35`
- [ ] `internal/plugins/ai.analysisInstruction` — `backend/internal/plugins/ai/prompts.go:11`
- [ ] `internal/plugins/ai.formatDZD` — `backend/internal/plugins/ai/prompts.go:129`
- [ ] `internal/plugins/ai.buildProductPrompt` — `backend/internal/plugins/ai/prompts.go:82`
- [ ] `internal/plugins/ai.buildStorePrompt` — `backend/internal/plugins/ai/prompts.go:22`
- [ ] `internal/plugins/ai.complete` *(method)* — `backend/internal/plugins/ai/client.go:138`
- [ ] `internal/plugins/ai.enabledFromSettings` — `backend/internal/plugins/ai/client.go:109`
- [ ] `internal/plugins/ai.configFromSettings` — `backend/internal/plugins/ai/client.go:90`
- [ ] `internal/plugins/ai.Detail` *(method)* — `backend/internal/plugins/ai/client.go:49`
- [ ] `internal/plugins/ai.StatusClass` *(method)* — `backend/internal/plugins/ai/client.go:42`
- [ ] `internal/plugins/ai.logWarn` — `backend/internal/plugins/ai/ai.go:333`
- [ ] `internal/plugins/ai.logUpstreamFailure` — `backend/internal/plugins/ai/ai.go:340`
- [ ] `internal/plugins/ai.newClient` — `backend/internal/plugins/ai/client.go:60`
- [ ] `internal/plugins/ai.handleAnalyze` *(method)* — `backend/internal/plugins/ai/ai.go:151`
- [ ] `internal/plugins/ai.handleRuns` *(method)* — `backend/internal/plugins/ai/ai.go:291`

### `internal/plugins/codemode` (55 functions)

- [ ] `internal/plugins/codemode.New` — `backend/internal/plugins/codemode/codemode.go:42`
- [ ] `internal/plugins/codemode.insertActionAudit` — `backend/internal/plugins/codemode/apply.go:305`
- [ ] `internal/plugins/codemode.logWarn` — `backend/internal/plugins/codemode/codemode.go:630`
- [ ] `internal/plugins/codemode.productFingerprint` — `backend/internal/plugins/codemode/plan.go:658`
- [ ] `internal/plugins/codemode.applyOrderStatus` *(method)* — `backend/internal/plugins/codemode/apply.go:236`
- [ ] `internal/plugins/codemode.applyProductArchive` *(method)* — `backend/internal/plugins/codemode/apply.go:141`
- [ ] `internal/plugins/codemode.Manifest` *(method)* — `backend/internal/plugins/codemode/codemode.go:50`
- [ ] `internal/plugins/codemode.normalizeAttributes` — `backend/internal/plugins/codemode/plan.go:553`
- [ ] `internal/plugins/codemode.validateProductAttributes` — `backend/internal/plugins/codemode/plan.go:566`
- [ ] `internal/plugins/codemode.validateProductDraft` *(method)* — `backend/internal/plugins/codemode/plan.go:487`
- [ ] `internal/plugins/codemode.applyProductCreate` *(method)* — `backend/internal/plugins/codemode/apply.go:177`
- [ ] `internal/plugins/codemode.isNull` — `backend/internal/plugins/codemode/plan.go:266`
- [ ] `internal/plugins/codemode.rawObject` — `backend/internal/plugins/codemode/plan.go:255`
- [ ] `internal/plugins/codemode.parseProductPatch` — `backend/internal/plugins/codemode/plan.go:422`
- [ ] `internal/plugins/codemode.applyProductUpdate` *(method)* — `backend/internal/plugins/codemode/apply.go:78`
- [ ] `internal/plugins/codemode.firstFieldMessage` — `backend/internal/plugins/codemode/plan.go:712`
- [ ] `internal/plugins/codemode.themeObject` — `backend/internal/plugins/codemode/plan.go:409`
- [ ] `internal/plugins/codemode.applyThemePatch` — `backend/internal/plugins/codemode/plan.go:335`
- [ ] `internal/plugins/codemode.applyThemeUpdate` *(method)* — `backend/internal/plugins/codemode/apply.go:50`
- [ ] `internal/plugins/codemode.applyAction` *(method)* — `backend/internal/plugins/codemode/apply.go:20`
- [ ] `internal/plugins/codemode.Error` *(method)* — `backend/internal/plugins/codemode/client.go:35`
- [ ] `internal/plugins/codemode.StatusClass` *(method)* — `backend/internal/plugins/codemode/client.go:42`
- [ ] `internal/plugins/codemode.Detail` *(method)* — `backend/internal/plugins/codemode/client.go:49`
- [ ] `internal/plugins/codemode.enabledFromSettings` — `backend/internal/plugins/codemode/client.go:78`
- [ ] `internal/plugins/codemode.configFromSettings` — `backend/internal/plugins/codemode/client.go:58`
- [ ] `internal/plugins/codemode.newClient` — `backend/internal/plugins/codemode/client.go:94`
- [ ] `internal/plugins/codemode.Error` *(method)* — `backend/internal/plugins/codemode/plan.go:126`
- [ ] `internal/plugins/codemode.plan` *(method)* — `backend/internal/plugins/codemode/client.go:138`
- [ ] `internal/plugins/codemode.ValidateSettings` *(method)* — `backend/internal/plugins/codemode/codemode.go:87`
- [ ] `internal/plugins/codemode.Migrations` *(method)* — `backend/internal/plugins/codemode/codemode.go:105`
- [ ] `internal/plugins/codemode.Routes` *(method)* — `backend/internal/plugins/codemode/codemode.go:126`
- [ ] `internal/plugins/codemode.parseAction` — `backend/internal/plugins/codemode/plan.go:191`
- [ ] `internal/plugins/codemode.parsePlan` — `backend/internal/plugins/codemode/plan.go:152`
- [ ] `internal/plugins/codemode.handlePlans` *(method)* — `backend/internal/plugins/codemode/codemode.go:145`
- [ ] `internal/plugins/codemode.recentOrders` — `backend/internal/plugins/codemode/codemode.go:196`
- [ ] `internal/plugins/codemode.buildPrompt` — `backend/internal/plugins/codemode/prompt.go:76`
- [ ] `internal/plugins/codemode.configFor` *(method)* — `backend/internal/plugins/codemode/codemode.go:539`
- [ ] `internal/plugins/codemode.encodePlan` — `backend/internal/plugins/codemode/plan.go:144`
- [ ] `internal/plugins/codemode.loadTheme` *(method)* — `backend/internal/plugins/codemode/codemode.go:561`
- [ ] `internal/plugins/codemode.logUpstreamFailure` — `backend/internal/plugins/codemode/codemode.go:637`
- [ ] `internal/plugins/codemode.newPlanID` — `backend/internal/plugins/codemode/plan.go:132`
- [ ] `internal/plugins/codemode.invalidPlan` — `backend/internal/plugins/codemode/plan.go:128`
- [ ] `internal/plugins/codemode.planBaseline` *(method)* — `backend/internal/plugins/codemode/plan.go:672`
- [ ] `internal/plugins/codemode.planDTO` — `backend/internal/plugins/codemode/plan.go:134`
- [ ] `internal/plugins/codemode.promptPlugins` *(method)* — `backend/internal/plugins/codemode/prompt.go:49`
- [ ] `internal/plugins/codemode.validatePlanReferences` *(method)* — `backend/internal/plugins/codemode/plan.go:589`
- [ ] `internal/plugins/codemode.validatePlanShape` *(method)* — `backend/internal/plugins/codemode/plan.go:270`
- [ ] `internal/plugins/codemode.handlePreview` *(method)* — `backend/internal/plugins/codemode/codemode.go:219`
- [ ] `internal/plugins/codemode.claimPlan` — `backend/internal/plugins/codemode/codemode.go:616`
- [ ] `internal/plugins/codemode.encodeResults` — `backend/internal/plugins/codemode/plan.go:722`
- [ ] `internal/plugins/codemode.ensureEnabled` *(method)* — `backend/internal/plugins/codemode/codemode.go:523`
- [ ] `internal/plugins/codemode.failPlan` *(method)* — `backend/internal/plugins/codemode/codemode.go:606`
- [ ] `internal/plugins/codemode.loadPlanRecord` *(method)* — `backend/internal/plugins/codemode/codemode.go:596`
- [ ] `internal/plugins/codemode.loadThemeTx` *(method)* — `backend/internal/plugins/codemode/codemode.go:580`
- [ ] `internal/plugins/codemode.handleApply` *(method)* — `backend/internal/plugins/codemode/codemode.go:351`

### `internal/plugins/oils` (3 functions)

- [ ] `internal/plugins/oils.New` — `backend/internal/plugins/oils/oils.go:14`
- [ ] `internal/plugins/oils.Manifest` *(method)* — `backend/internal/plugins/oils/oils.go:18`
- [ ] `internal/plugins/oils.Routes` *(method)* — `backend/internal/plugins/oils/oils.go:69`

### `internal/plugins/pcparts` (3 functions)

- [ ] `internal/plugins/pcparts.New` — `backend/internal/plugins/pcparts/pcparts.go:17`
- [ ] `internal/plugins/pcparts.Manifest` *(method)* — `backend/internal/plugins/pcparts/pcparts.go:21`
- [ ] `internal/plugins/pcparts.Routes` *(method)* — `backend/internal/plugins/pcparts/pcparts.go:150`

### `internal/plugins/pcbuilder` (6 functions)

- [ ] `internal/plugins/pcbuilder.toPartsProduct` — `backend/internal/plugins/pcbuilder/parts.go:150`
- [ ] `internal/plugins/pcbuilder.buildPartsResponse` — `backend/internal/plugins/pcbuilder/parts.go:92`
- [ ] `internal/plugins/pcbuilder.handleParts` *(method)* — `backend/internal/plugins/pcbuilder/parts.go:54`
- [ ] `internal/plugins/pcbuilder.New` — `backend/internal/plugins/pcbuilder/pcbuilder.go:18`
- [ ] `internal/plugins/pcbuilder.Manifest` *(method)* — `backend/internal/plugins/pcbuilder/pcbuilder.go:22`
- [ ] `internal/plugins/pcbuilder.Routes` *(method)* — `backend/internal/plugins/pcbuilder/pcbuilder.go:36`

### `internal/plugins/phone` (26 functions)

- [ ] `internal/plugins/phone.countDevices` *(method)* — `backend/internal/plugins/phone/devices.go:250`
- [ ] `internal/plugins/phone.phoneProductProblem` — `backend/internal/plugins/phone/devices.go:304`
- [ ] `internal/plugins/phone.VariantLabel` — `backend/internal/plugins/phone/variant.go:28`
- [ ] `internal/plugins/phone.scanDevice` — `backend/internal/plugins/phone/devices.go:266`
- [ ] `internal/plugins/phone.queryDevices` *(method)* — `backend/internal/plugins/phone/devices.go:232`
- [ ] `internal/plugins/phone.writeInternal` — `backend/internal/plugins/phone/devices.go:309`
- [ ] `internal/plugins/phone.handleListDevices` *(method)* — `backend/internal/plugins/phone/devices.go:56`
- [ ] `internal/plugins/phone.New` — `backend/internal/plugins/phone/phone.go:52`
- [ ] `internal/plugins/phone.ValidIMEI` — `backend/internal/plugins/phone/variant.go:40`
- [ ] `internal/plugins/phone.VariantKey` — `backend/internal/plugins/phone/variant.go:17`
- [ ] `internal/plugins/phone.allowedOption` — `backend/internal/plugins/phone/variant.go:61`
- [ ] `internal/plugins/phone.imeiExists` *(method)* — `backend/internal/plugins/phone/devices.go:294`
- [ ] `internal/plugins/phone.syncProductStock` *(method)* — `backend/internal/plugins/phone/devices.go:284`
- [ ] `internal/plugins/phone.handleCreateDevice` *(method)* — `backend/internal/plugins/phone/devices.go:91`
- [ ] `internal/plugins/phone.handleDeleteDevice` *(method)* — `backend/internal/plugins/phone/devices.go:174`
- [ ] `internal/plugins/phone.handleOrderDevices` *(method)* — `backend/internal/plugins/phone/devices.go:219`
- [ ] `internal/plugins/phone.ValidateCheckoutLine` *(method)* — `backend/internal/plugins/phone/hooks.go:15`
- [ ] `internal/plugins/phone.OnOrderCreated` *(method)* — `backend/internal/plugins/phone/hooks.go:51`
- [ ] `internal/plugins/phone.OnOrderStatusChanged` *(method)* — `backend/internal/plugins/phone/hooks.go:96`
- [ ] `internal/plugins/phone.EnrichProduct` *(method)* — `backend/internal/plugins/phone/hooks.go:120`
- [ ] `internal/plugins/phone.legacyConfigs` — `backend/internal/plugins/phone/phone.go:142`
- [ ] `internal/plugins/phone.repairLegacyAttributes` *(method)* — `backend/internal/plugins/phone/phone.go:82`
- [ ] `internal/plugins/phone.Init` *(method)* — `backend/internal/plugins/phone/phone.go:56`
- [ ] `internal/plugins/phone.Manifest` *(method)* — `backend/internal/plugins/phone/phone.go:159`
- [ ] `internal/plugins/phone.Routes` *(method)* — `backend/internal/plugins/phone/phone.go:195`
- [ ] `internal/plugins/phone.Migrations` *(method)* — `backend/internal/plugins/phone/phone.go:204`

### `internal/seed` (5 functions)

- [ ] `internal/seed.seedPluginCatalog` — `backend/internal/seed/plugins.go:69`
- [ ] `internal/seed.seedCatalog` — `backend/internal/seed/seed.go:177`
- [ ] `internal/seed.seedOrders` — `backend/internal/seed/seed.go:367`
- [ ] `internal/seed.seedUsers` — `backend/internal/seed/seed.go:269`
- [ ] `internal/seed.Run` — `backend/internal/seed/seed.go:32`

### `cmd/api` (1 functions)

- [ ] `cmd/api.main` — `backend/cmd/api/main.go:29`

### `cmd/oracle-ids-auth` (2 functions)

- [ ] `cmd/oracle-ids-auth.handle` — `backend/cmd/oracle-ids-auth/main.go:35`
- [ ] `cmd/oracle-ids-auth.main` — `backend/cmd/oracle-ids-auth/main.go:74`

### `cmd/oracle-orderstatus-secrets` (2 functions)

- [ ] `cmd/oracle-orderstatus-secrets.handle` — `backend/cmd/oracle-orderstatus-secrets/main.go:64`
- [ ] `cmd/oracle-orderstatus-secrets.main` — `backend/cmd/oracle-orderstatus-secrets/main.go:26`

### `desktop/internal/api` (86 functions)

- [ ] `desktop/internal/api.envEnabled` — `desktop/internal/api/session.go:231`
- [ ] `desktop/internal/api.canonicalOrigin` — `desktop/internal/api/urls.go:160`
- [ ] `desktop/internal/api.containsControl` — `desktop/internal/api/urls.go:211`
- [ ] `desktop/internal/api.newError` — `desktop/internal/api/types.go:72`
- [ ] `desktop/internal/api.validationError` — `desktop/internal/api/types.go:81`
- [ ] `desktop/internal/api.invalidAPIURL` — `desktop/internal/api/urls.go:70`
- [ ] `desktop/internal/api.normalizeAPIURL` — `desktop/internal/api/urls.go:15`
- [ ] `desktop/internal/api.sessionScope` — `desktop/internal/api/session.go:98`
- [ ] `desktop/internal/api.defaultSecretStore` — `desktop/internal/api/session.go:214`
- [ ] `desktop/internal/api.deviceHash` — `desktop/internal/api/client.go:104`
- [ ] `desktop/internal/api.Load` *(method)* — `desktop/internal/api/session.go:43`
- [ ] `desktop/internal/api.validCookieValue` — `desktop/internal/api/session.go:263`
- [ ] `desktop/internal/api.Load` *(method)* — `desktop/internal/api/session.go:114`
- [ ] `desktop/internal/api.Load` *(method)* — `desktop/internal/api/session.go:153`
- [ ] `desktop/internal/api.Load` *(method)* — `desktop/internal/api/session.go:184`
- [ ] `desktop/internal/api.loadSession` *(method)* — `desktop/internal/api/session.go:276`
- [ ] `desktop/internal/api.normalizeHost` — `desktop/internal/api/urls.go:119`
- [ ] `desktop/internal/api.parseHostIP` — `desktop/internal/api/urls.go:153`
- [ ] `desktop/internal/api.isLoopbackHost` — `desktop/internal/api/urls.go:130`
- [ ] `desktop/internal/api.isLocalHost` — `desktop/internal/api/urls.go:139`
- [ ] `desktop/internal/api.validateTransportAddress` — `desktop/internal/api/urls.go:123`
- [ ] `desktop/internal/api.apiHost` — `desktop/internal/api/urls.go:83`
- [ ] `desktop/internal/api.decodeSetting` — `desktop/internal/api/client.go:204`
- [ ] `desktop/internal/api.loadLegacySettings` — `desktop/internal/api/client.go:184`
- [ ] `desktop/internal/api.invalidOrigin` — `desktop/internal/api/urls.go:74`
- [ ] `desktop/internal/api.normalizeOrigin` — `desktop/internal/api/urls.go:45`
- [ ] `desktop/internal/api.writePrivateFile` — `desktop/internal/api/session.go:390`
- [ ] `desktop/internal/api.saveSettings` *(method)* — `desktop/internal/api/client.go:212`
- [ ] `desktop/internal/api.loadSettings` *(method)* — `desktop/internal/api/client.go:109`
- [ ] `desktop/internal/api.newClientWithStoreFactory` — `desktop/internal/api/client.go:60`
- [ ] `desktop/internal/api.NewClient` — `desktop/internal/api/client.go:47`
- [ ] `desktop/internal/api.newClientWithSecrets` — `desktop/internal/api/client.go:54`
- [ ] `desktop/internal/api.settingsConsentAllowed` — `desktop/internal/api/urls.go:111`
- [ ] `desktop/internal/api.Info` *(method)* — `desktop/internal/api/client.go:220`
- [ ] `desktop/internal/api.clearImageCacheLocked` *(method)* — `desktop/internal/api/client.go:394`
- [ ] `desktop/internal/api.networkConsentError` — `desktop/internal/api/urls.go:252`
- [ ] `desktop/internal/api.consentHostForUpdate` — `desktop/internal/api/urls.go:94`
- [ ] `desktop/internal/api.Delete` *(method)* — `desktop/internal/api/session.go:80`
- [ ] `desktop/internal/api.Delete` *(method)* — `desktop/internal/api/session.go:141`
- [ ] `desktop/internal/api.Delete` *(method)* — `desktop/internal/api/session.go:171`
- [ ] `desktop/internal/api.Delete` *(method)* — `desktop/internal/api/session.go:203`
- [ ] `desktop/internal/api.deleteStoredSession` *(method)* — `desktop/internal/api/session.go:344`
- [ ] `desktop/internal/api.cookieFromSetCookie` — `desktop/internal/api/session.go:249`
- [ ] `desktop/internal/api.Save` *(method)* — `desktop/internal/api/session.go:62`
- [ ] `desktop/internal/api.Save` *(method)* — `desktop/internal/api/session.go:133`
- [ ] `desktop/internal/api.Save` *(method)* — `desktop/internal/api/session.go:167`
- [ ] `desktop/internal/api.Save` *(method)* — `desktop/internal/api/session.go:193`
- [ ] `desktop/internal/api.persistSession` *(method)* — `desktop/internal/api/session.go:326`
- [ ] `desktop/internal/api.absorbCookies` *(method)* — `desktop/internal/api/session.go:289`
- [ ] `desktop/internal/api.isTimeout` — `desktop/internal/api/request.go:352`
- [ ] `desktop/internal/api.networkFailure` — `desktop/internal/api/request.go:344`
- [ ] `desktop/internal/api.active` *(method)* — `desktop/internal/api/session.go:245`
- [ ] `desktop/internal/api.requestHeadersLocked` *(method)* — `desktop/internal/api/request.go:213`
- [ ] `desktop/internal/api.responseTooLarge` — `desktop/internal/api/request.go:336`
- [ ] `desktop/internal/api.settingsChangedError` — `desktop/internal/api/request.go:340`
- [ ] `desktop/internal/api.execute` *(method)* — `desktop/internal/api/request.go:168`
- [ ] `desktop/internal/api.invalidPath` — `desktop/internal/api/urls.go:207`
- [ ] `desktop/internal/api.validateAPIPath` — `desktop/internal/api/urls.go:171`
- [ ] `desktop/internal/api.targetURL` *(method)* — `desktop/internal/api/urls.go:220`
- [ ] `desktop/internal/api.Health` *(method)* — `desktop/internal/api/health.go:39`
- [ ] `desktop/internal/api.resolveOrigin` *(method)* — `desktop/internal/api/client.go:361`
- [ ] `desktop/internal/api.settingsResult` *(method)* — `desktop/internal/api/client.go:314`
- [ ] `desktop/internal/api.UpdateSettings` *(method)* — `desktop/internal/api/client.go:239`
- [ ] `desktop/internal/api.WindowBounds` *(method)* — `desktop/internal/api/client.go:333`
- [ ] `desktop/internal/api.SaveWindowBounds` *(method)* — `desktop/internal/api/client.go:347`
- [ ] `desktop/internal/api.firewallGuide` — `desktop/internal/api/firewall.go:9`
- [ ] `desktop/internal/api.FirewallGuide` *(method)* — `desktop/internal/api/firewall.go:5`
- [ ] `desktop/internal/api.ensureOrigin` *(method)* — `desktop/internal/api/health.go:8`
- [ ] `desktop/internal/api.invalidImage` — `desktop/internal/api/image.go:109`
- [ ] `desktop/internal/api.storeImage` *(method)* — `desktop/internal/api/image.go:77`
- [ ] `desktop/internal/api.image` *(method)* — `desktop/internal/api/image.go:32`
- [ ] `desktop/internal/api.Image` *(method)* — `desktop/internal/api/image.go:24`
- [ ] `desktop/internal/api.isCSRFEndpoint` — `desktop/internal/api/request.go:60`
- [ ] `desktop/internal/api.clearCSRF` *(method)* — `desktop/internal/api/session.go:373`
- [ ] `desktop/internal/api.decodeJSON` — `desktop/internal/api/request.go:325`
- [ ] `desktop/internal/api.encodeRequestBody` — `desktop/internal/api/request.go:151`
- [ ] `desktop/internal/api.activeCSRF` *(method)* — `desktop/internal/api/session.go:353`
- [ ] `desktop/internal/api.storeCSRF` *(method)* — `desktop/internal/api/session.go:362`
- [ ] `desktop/internal/api.ensureCSRF` *(method)* — `desktop/internal/api/request.go:247`
- [ ] `desktop/internal/api.errorCode` — `desktop/internal/api/request.go:313`
- [ ] `desktop/internal/api.errorFromResult` — `desktop/internal/api/request.go:287`
- [ ] `desktop/internal/api.request` *(method)* — `desktop/internal/api/request.go:75`
- [ ] `desktop/internal/api.Request` *(method)* — `desktop/internal/api/request.go:49`
- [ ] `desktop/internal/api.ClearSession` *(method)* — `desktop/internal/api/session.go:379`
- [ ] `desktop/internal/api.Error` *(method)* — `desktop/internal/api/types.go:43`
- [ ] `desktop/internal/api.validateServerAddress` — `desktop/internal/api/urls.go:78`

### `desktop` (19 functions)

- [ ] `desktop.profileDirectory` — `desktop/main.go:112`
- [ ] `desktop.NewApp` — `desktop/main.go:100`
- [ ] `desktop.context` *(method)* — `desktop/main.go:133`
- [ ] `desktop.windowBounds` *(method)* — `desktop/main.go:224`
- [ ] `desktop.main` — `desktop/main.go:33`
- [ ] `desktop.securityHeaders` — `desktop/main.go:81`
- [ ] `desktop.startup` *(method)* — `desktop/main.go:127`
- [ ] `desktop.clampWindowSize` — `desktop/main.go:198`
- [ ] `desktop.screenPositionAllowed` — `desktop/main.go:151`
- [ ] `desktop.domReady` *(method)* — `desktop/main.go:139`
- [ ] `desktop.beforeClose` *(method)* — `desktop/main.go:180`
- [ ] `desktop.Info` *(method)* — `desktop/main.go:228`
- [ ] `desktop.Health` *(method)* — `desktop/main.go:232`
- [ ] `desktop.Request` *(method)* — `desktop/main.go:236`
- [ ] `desktop.Image` *(method)* — `desktop/main.go:240`
- [ ] `desktop.UpdateSettings` *(method)* — `desktop/main.go:244`
- [ ] `desktop.ClearSession` *(method)* — `desktop/main.go:248`
- [ ] `desktop.FirewallGuide` *(method)* — `desktop/main.go:252`
- [ ] `desktop.OpenExternal` *(method)* — `desktop/main.go:256`


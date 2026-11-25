# Auth in the Rust port

The Go API hand-rolled its session bookkeeping: random bearer tokens, SHA-256
token hashes in `sessions`, and Argon2id password hashes. The Rust port replaces
the session layer with maintained crates and keeps the password hashing scheme
byte-compatible so existing accounts keep working.

## The choice

| Concern | Crate | Version | Why |
| --- | --- | --- | --- |
| Password hashing | [`argon2`](https://crates.io/crates/argon2) (RustCrypto) | 0.6.0 | The reference Rust Argon2 implementation: 81M downloads, released 2026-08-27, audited RustCrypto code, PHC string support. Same Argon2id parameters as the Go code. |
| Sessions | [`tower-sessions`](https://crates.io/crates/tower-sessions) | 0.15.0 | Server-side sessions as Tower middleware with an axum 0.8 extractor. 5.0M downloads, released 2026-02-01, maintained by the axum ecosystem author. Opaque random session IDs, pluggable store. |
| Session storage | local adapter (`crates/azahar-httpapi/src/session_store.rs`) | — | `tower-sessions-libsql-store` 0.4.0 pins `libsql` 0.6 and `tower-sessions-core` 0.14 while this workspace uses `libsql` 0.10 and `tower-sessions` 0.15. The `SessionStore` trait is four async methods, so a small adapter over our own connection is safer than downgrading the database stack. Records use `rmp-serde` 1.3.1 (MessagePack). No cryptography is hand-rolled. |
| Database client | [`libsql`](https://crates.io/crates/libsql) | 0.10.0 | Official Turso client. Accepts the same `http(s)://` DSN and `authToken` query parameter as the Go driver. 2.7M downloads, released 2026-10-13. |
| Web framework | [`axum`](https://crates.io/crates/axum) | 0.8.9 | Maintained by the tokio project; axum 0.8 is what `tower-sessions` 0.15 targets. |

`axum-login` 0.19.0 was evaluated and rejected. It adds an `AuthnBackend` layer
on top of the same `tower-sessions`, but the existing contract needs an absolute
session lifetime that differs for staff and customers, and login must rotate the
session id and delete the old record. Those are direct session decisions, so the
port uses `tower-sessions` directly rather than wrapping it in another
abstraction that does not expose them.

## Password hashes

`HashPassword` and `VerifyPassword` keep the Go format so the oracle and the
Rust service can read the same `users.password_hash` rows:

```text
$argon2id$v=19$m=65536,t=3,p=4$<base64 raw std salt>$<base64 raw std key>
```

- Argon2id, version 19, 64 MiB memory, 3 iterations, 4 lanes, 16-byte salt,
  32-byte key.
- Verification accepts only exactly six `$`-separated fields, the `argon2id`
  algorithm, version 19, and the parameter bounds the Go code enforced
  (`m <= 1 GiB`, `t <= 20`, `p <= 64`, salt >= 8 bytes, key 16..=128 bytes).
- The derived key is compared with `subtle::ConstantTimeEq`; verification does not short-circuit on the first differing byte.
- `VerifyDummy` runs a real hash check against a cached dummy hash so an unknown
  email costs the same as a wrong password.

Go-verifiable hashes with a memory cost below Argon2's defined minimum
(`m < 8*p`) are rejected by the Rust `argon2` crate; Go clamps them instead.
`HashPassword` never emits such parameters, so no account created here is
affected, and the rejection is pinned by a differential fixture and listed in
`MIGRATION.md`. Differential tests generate hashes on both sides and verify each
other's output, including malformed and hostile PHC strings.

## Sessions

- Session data lives in the Rust-owned `web_sessions` table
  (`id TEXT PRIMARY KEY`, `data BLOB NOT NULL`, `expiry INTEGER NOT NULL` in
  Unix seconds, indexed by `web_sessions_expiry`). `data` is the MessagePack
  encoding of `tower_sessions::session::Record` produced by `rmp-serde` 1.3.1;
  it is not JSON and never contains password material. The Go `sessions` table
  is untouched while the oracle still runs; once the Go API is removed the old
  table can be dropped. This is logged in `MIGRATION.md` as a known difference:
  sessions are not shared between the two services, so a user signed in on the
  Go service must sign in again on the Rust service.
- The cookie is `azahar_session`, `HttpOnly`, `SameSite=Lax`, path `/`. The
  `Secure` flag is a boolean passed to `session_layer`, wired by the caller to
  `COOKIE_SECURE` (or production). The value is the session ID issued by
  `tower-sessions`; no password material or token hash is stored client-side.
- `tower-sessions` cycles the session ID on login, which prevents session
  fixation. Logout calls `Session::flush`, which clears the data, deletes the
  row, and lets the middleware send `Max-Age=0` for the cookie.
- Expiry is absolute, not sliding. `tower-sessions` applies one
  `Expiry::AtDateTime` value from the layer configuration, computed when the
  layer is built; a process that runs for days would otherwise give every
  session the same deadline. `login_with_ttl` therefore calls
  `Session::set_expiry(Expiry::AtDateTime(now + ttl))` at login, and handlers
  must use it rather than the plain `login` helper. The TTL comes from
  `ttl_for_role`: customers keep `SESSION_TTL_HOURS`, staff (every role other
  than `customer`) get `min(SESSION_TTL, 12h)`, matching
  `Config::AdminSessionTTL`. `tower-sessions` takes `time::OffsetDateTime` for
  `Expiry` (time 0.3.55); the layer converts the `chrono::Duration` TTL at the
  boundary.
- Expired rows are never accepted: `load` filters `expiry > now`. Cleanup is
  `LibsqlSessionStore::delete_expired(now)`, a plain async method because the
  `ExpiredDeletion` trait has no clock parameter; callers should run it
  periodically or after login. Login itself does not implicitly purge rows.
- The Rust session row does not store the user agent or IP. The Go login could
  delete other sessions belonging to the same user agent; the Rust port rotates
  only the session that performs the login. This is a known behaviour
  difference and does not change the HTTP contract.
- CSRF stays a double-submit cookie (`azahar_csrf` + `X-CSRF-Token`) enforced
  before handlers, because the SvelteKit frontend already sends it and the
  cookie is readable by design.

## Turso

The adapter is handed the connection from the store (`store.conn().clone()`),
which is the same remote `libsql::Database` opened by `azahar_db::open`; it does
not create a second database handle and never talks to the embedded local
replica. `azahar_db::open` moves an `authToken` query parameter from the DSN
into the client token slot, so production Turso and the local `sqld` container
use one code path. The session table is created by migration
`011_web_sessions.sql`, applied by the shared migration runner.

## What the migration does not change

- Account roles and authorisation rules (`customer`, `employee`, `admin`) stay
  as they are.
- Password rules (length, validation messages) stay in the HTTP layer.
- Rate limits on `/api/auth/login` and `/api/auth/register` stay in the HTTP
  layer, keyed by client IP.
- No OAuth provider is added; the product has no third-party login today.

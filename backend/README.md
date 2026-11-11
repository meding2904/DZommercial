# Azahar API

Go API for the Azahar demo storefront. It serves the public catalog, accounts, checkout,
order lookup by token and the admin dashboard against libSQL/Turso.

## Layout

```
cmd/api                 HTTP server and graceful shutdown
internal/config         environment parsing and production checks
internal/db             libsql connection, embedded migrations, schema_migrations
internal/auth           Argon2id hashing, session and order tokens
internal/httpapi        chi router, middleware, handlers, validation
internal/store          queries and row mapping
internal/seed           idempotent demo seed
migrations              SQL schema
scripts/smoke.sh        start the API and run the six contract checks
```

## Run locally

The local libSQL server is expected at `http://127.0.0.1:8880`.

```sh
cd backend
go run ./cmd/api
```

Defaults in development:

| Variable | Default |
|---|---|
| `APP_ENV` | `development` |
| `API_ADDR` | `:8080` |
| `APP_ORIGIN` | `http://localhost:5173` |
| `TURSO_DATABASE_URL` | `http://127.0.0.1:8880` |
| `TURSO_AUTH_TOKEN` | empty |
| `COOKIE_SECURE` | false (true in production) |
| `TRUST_PROXY` | false |
| `SEED_DEMO` | true in development, false otherwise |
| `ADMIN_EMAIL` | `admin@azahar.example` |
| `ADMIN_PASSWORD` | `azahar-dev-admin-2026` |
| `CUSTOMER_EMAIL` | `client@azahar.example` |
| `CUSTOMER_PASSWORD` | `azahar-dev-client-2026` |
| `COURIER_API_URL` | `https://freeship.dzbuild.com` |
| `COURIER_KEY` | `sandbox` |
| `COURIER_CREDENTIALS` | empty (JSON object for a real courier) |
| `COURIER_FROM_WILAYA` | `16` |
| `COURIER_TIMEOUT_SECONDS` | `12` |
| `SESSION_TTL_HOURS` | `168` (admin sessions are capped at 12 hours) |

The startup log prints the demo credentials only when `APP_ENV=development`.

In production the API refuses to start without `TURSO_DATABASE_URL`, `TURSO_AUTH_TOKEN`,
`APP_ORIGIN`, `ADMIN_EMAIL` and `ADMIN_PASSWORD` set explicitly. The development admin
password is rejected, the admin password must be at least 14 characters, and `SEED_DEMO`
defaults to false. If the seed is enabled, the development customer password is rejected.
`APP_ORIGIN` is trimmed of a trailing slash. The token is appended to the database URL as
`authToken` so `libsql://...` and the local `http://127.0.0.1:8880` both work.

The courier integration talks to `COURIER_API_URL` with `COURIER_KEY` and the JSON object in
`COURIER_CREDENTIALS`. The default `sandbox` key validates orders but creates no real parcels.
`COURIER_API_URL` must use https in production. `COURIER_FROM_WILAYA` is the origin wilaya sent
to the courier and must be between 1 and 58. If the reference service is unreachable, the
bundled wilaya and commune snapshot is used; development with the sandbox key also falls back
to a local sandbox shipment. Real courier keys never use the local fallback.

### Dev credentials

- Admin: `admin@azahar.example` / `azahar-dev-admin-2026`
- Customer: `client@azahar.example` / `azahar-dev-client-2026`

## Schema notes

Tables: `users`, `sessions`, `categories`, `products`, `orders`, `order_items`,
`audit_log`, `schema_migrations`, plus `order_sequences` for yearly order numbers.

- IDs are prefixed random text: `usr_`, `ses_`, `cat_`, `prd_`, `ord_`, `itm_`, `aud_`.
- Timestamps are RFC 3339 UTC text. Date filters compare those strings and are therefore
  lexicographic and stable.
- Booleans are INTEGER (`0`/`1`). `products.active` and `products.featured` are converted
  in the store layer.
- `products.concerns`, `products.ingredients` and `products.gallery` are JSON arrays stored
  as text. The API always returns arrays, never null.
- Only a SHA-256 hex hash of a session or order token is stored.
- `schema_migrations` records each applied file. Migrations are embedded from
  `backend/migrations` and applied in file order inside a transaction.
- Indexes cover product slug/active, order created_at/status, session token_hash/expires_at,
  order_items.order_id and audit_log.created_at.

## Demo seed

`SEED_DEMO=true` seeds three categories, the eight DESIGN.md products, an admin, a demo
customer, twelve sample customers and 48 orders over the last 45 days. Status counts are
6 pending, 6 confirmed, 5 packed, 8 shipped, 20 delivered and 3 cancelled. Six orders are
guest orders. Each section runs only when its table is empty, so restarts never duplicate
rows. The seeded order numbers are `AZ-2026-0001` through `AZ-2026-0048` on a fresh
database.

The catalog is seeded in this fixed order as `prd_01` to `prd_08`:

1. `orange-blossom-cleansing-oil`
2. `rosehip-and-olive-face-serum`
3. `rose-clay-mask`
4. `fig-and-sesame-body-balm`
5. `prickly-pear-face-oil`
6. `cedar-and-myrrh-hand-cream`
7. `green-fig-body-oil`
8. `rose-petal-bath-soak`

The public product list still follows the documented `sort` rules (`featured` by default).
`GET /api/products/orange-blossom-cleansing-oil` returns the first seeded product.

## Test and verify

```sh
cd backend
gofmt -w .
go vet ./...
go test ./...
```

Run the real libSQL integration test (safe, creates and removes its own category and
product):

```sh
AZAHAR_TEST_DATABASE_URL=http://127.0.0.1:8880 go test ./internal/store -run TestProductLifecycleAgainstLibSQL -v
```

Unit tests cover Argon2id hashing and verification, token generation and hashing,
validation helpers, order status transitions and the rate limiter.

### Smoke test

The script builds the API, starts it on `:8080`, runs the checks and stops only the process
it started. It leaves one real pending order and decrements that product by one, because
checkout is part of the smoke test.

```sh
cd backend
./scripts/smoke.sh
```

Manual equivalent. Run this with the API already running on `:8080`:

```sh
BASE=http://127.0.0.1:8080
JAR=$(mktemp)

# 1. Public catalog returns eight products
curl -s "$BASE/api/products"

# 2. Admin login returns 200 and a session cookie
curl -s -D /tmp/azahar-login-headers -c "$JAR" \
  -X POST "$BASE/api/auth/login" \
  -H 'Content-Type: application/json' \
  -H 'X-Requested-With: azahar-web' \
  -H 'Origin: http://localhost:5173' \
  -d '{"email":"admin@azahar.example","password":"azahar-dev-admin-2026"}'

# 3. Metrics returns 200 and a 30 day series
curl -s -b "$JAR" "$BASE/api/admin/metrics?days=30"

# 4. Checkout returns 201 and an order token
curl -s -X POST "$BASE/api/checkout" \
  -H 'Content-Type: application/json' \
  -H 'X-Requested-With: azahar-web' \
  -H 'Origin: http://localhost:5173' \
  -d '{"name":"Amina B.","email":"amina@example.com","phone":"0550 00 00 00","address":"12 rue des Oliviers","city":"Algiers","notes":"","items":[{"product_id":"prd_01","quantity":1}]}'

# 5. Public order lookup by token
curl -s "$BASE/api/orders/REPLACE_WITH_TOKEN"

# 6. Unauthenticated admin request returns 401
curl -s -o /dev/null -w '%{http_code}\n' "$BASE/api/admin/metrics"
```

## API notes and small deviations

The field names in `docs/API.md` are kept exactly. A few additions and choices are worth
knowing:

- `Product` includes `compare_at_cents` as `null` when absent. Public products omit
  `created_at`/`updated_at`; admin products include them.
- `Order` includes the documented address and totals plus a `customer` object
  (`id` when signed in, name, email, phone) and flat `name`, `email`, `phone` fields. The
  `customer` object is additive, not a replacement.
- The token is echoed by `GET /api/orders/{token}` as well as returned by checkout. The
  token is in the URL at that point and only its hash is stored. It is omitted from
  `/api/account/orders` and every admin response.
- `/api/account/orders` is the order shape without `token` and without `address`.
- An invalid `/api/admin/metrics?days=` value returns `422`. Unknown `sort` values fall
  back to `featured` as documented.
- Metrics `orders_count`, `aov_cents` and `units_sold` exclude cancelled orders, matching
  the revenue rule. `status_split` still includes cancelled orders and every status with a
  zero count. `recent_orders` is the latest eight orders inside the selected range.
- JSON bodies require `Content-Type: application/json` (`415` otherwise), unknown fields
  are rejected, and bodies over 1 MiB return `413`.
- `GET /api/auth/logout` does not exist. Logout is `POST` and returns `204` even when no
  session cookie is present.
- No CORS headers are set. The SvelteKit server is the only browser origin.

## License

The API source is licensed under the Apache License 2.0. See `../LICENSE` and `../NOTICE`.
Third-party software keeps its own license.

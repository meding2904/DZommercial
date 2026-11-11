# Azahar security notes

This is a demo storefront, but it is built as if it will take orders. The controls below are
implemented in the Go API and the SvelteKit edge. Anything marked "before production" is a
hard requirement, not a suggestion.

## Threat model

- A public visitor can read the catalog, create an account, place a cash-on-delivery order
  and view that order with its unguessable token.
- A signed-in customer can read only their own orders and preferences.
- An admin can read the financial dashboard, manage products and advance order status.
- The database is Turso. The API holds the only database credentials.
- The browser never sees the database URL, the auth token or the admin password hash.

## Authentication

- Passwords are hashed with Argon2id, 64 MiB, 3 iterations, 4 threads, 32 byte key, 16 byte
  salt. The encoded hash stores its own parameters, so they can be raised later.
- Password length is 10 to 200 characters. Email is trimmed and lowercased.
- Login errors are generic: "Email or password is incorrect." The handler still verifies a
  dummy hash when the email is unknown, so response time does not reveal account existence.
- Registration returns `409` for an existing email. It does not log the password.
- A successful login deletes the previous session for that browser and creates a new one,
  which prevents fixation.
- Sessions are 32 random bytes. Only a SHA-256 hash is stored in `sessions.token_hash`.
  Sessions expire after 7 days, or 12 hours for an admin login. Expired rows are deleted on
  login and on `/api/auth/me`.
- The cookie is `HttpOnly`, `SameSite=Lax`, `Path=/` and `Secure` when
  `COOKIE_SECURE=true` or `APP_ENV=production`. It is never readable from JavaScript.

## Hidden admin gate

- The store has no link to the admin. The admin UI is not in the navigation, the footer or the
  mobile menu.
- Kiri generates a 48 character hex `ADMIN_ACCESS_TOKEN` at startup and prints the gate as
  `${APP_ORIGIN}/ops-<token>`. The token is private server configuration. It is not in the
  client bundle, the HTML or the public environment.
- Gate checks run on the decoded and normalised path, not the raw URL. Percent escapes,
  backslashes and `.`/`..` segments are resolved first, so `/%61dmin`, `/admin%5Clogin` and
  `/api/admin%5Cmetrics` are treated as admin paths. The proxy also checks the final URL it
  will fetch, not only the route parameter.
- Opening the exact gate path sets an `azahar_ops` cookie and redirects to `/admin/login`.
  The cookie is `HttpOnly`, `SameSite=Strict`, `Path=/`, `Secure` in production, and expires
  after 12 hours.
- `/admin`, `/admin/*` and the proxied `/api/admin/*` return 404 unless the gate cookie is
  present. The admin session is still required. The gate is an extra layer, not a replacement
  for authentication or RBAC.
- A missing, short or development `ADMIN_ACCESS_TOKEN` disables the gate everywhere,
  including `/api/admin/*`. It never accepts the documented `devtoken` value in production.
- `/api/auth/login` stays reachable so the gated login page can create the admin session.
  An attacker who knows the admin password but not the gate still cannot use the admin UI or
  the admin API through the web origin. The API port must never be published in production.
- In production, `ADMIN_ACCESS_TOKEN` must be at least 32 characters and must not be the
  development default. If it is missing or weak, the gate and admin paths return 404 and the
  rest of the store keeps working.
- The token rotates on every Kiri start, so the previous gate link stops working. Keep the
  printed gate out of logs, screenshots and tickets. The admin session cookie is separate and
  is cleared by sign out.

## CSRF and origin checks

- Safe methods are `GET`, `HEAD` and `OPTIONS`.
- Every unsafe request requires `X-Requested-With: azahar-web`. A cross-site HTML form cannot
  set a custom header.
- Every unsafe request with an `Origin` header must match `APP_ORIGIN`. The SvelteKit proxy
  forwards the browser's origin. In development, a missing origin is allowed only when
  `APP_ENV=development`.
- The API does not enable CORS. Cross-origin reads are blocked by the browser.

## Authorization

- `requireUser` loads the session and the user on every protected request.
- `requireAdmin` checks `role = "admin"` after `requireUser`.
- Product, order and preference mutations are admin-only. A customer can read only their own
  orders. The public order endpoint is gated by a 256-bit token, not by an ID.
- Admin actions write to `audit_log`: product create/update/archive, order status change and
  preference change.

## Input and data integrity

- JSON bodies are limited to 1 MiB and decoded with `DisallowUnknownFields`. The web proxy
  enforces the same cap before buffering a request body.
- All queries use placeholders. No SQL is built by string concatenation.
- Slugs match `^[a-z0-9]+(?:-[a-z0-9]+)*$`. Emails are validated with `net/mail`. Phone
  must be an Algerian mobile: 05, 06 or 07, with spaces, `+`, `-` or parentheses allowed.
- `wilaya_code` is 1 to 58. `commune_name` and `stop_desk_id` are checked against the
  courier reference data when it is reachable; the canonical French spelling is stored.
- Address, notes and stop-desk fields have length caps.
- Checkout re-reads every product price and stock inside a transaction. The client never
  sends a price or a total. Stock is decremented with a conditional update, so two
  simultaneous orders cannot sell the last unit twice.
- Order numbers are generated from a sequence and are not the capability. The token is.
- A request with more than 20 lines or more than 10 units per line is rejected.
- Product image paths must start with `/images/` and contain no `..`, or be an `https://`
  URL. This keeps the admin form from injecting `javascript:` or local file paths.

## Courier integration

- All courier calls happen on the Go server. Courier credentials come from
  `COURIER_CREDENTIALS` and are never sent to the browser, included in order responses or
  written to logs.
- The client only calls the configured `COURIER_API_URL`; user input cannot choose a host,
  path, tenant or credential. Redirects are not followed.
- Reference data (wilayas, communes and desks) is public and cached for a day. Parcel
  creation and tracking are admin-only and write to `audit_log`.
- A bundled snapshot of the public wilaya and commune lists is used when the live reference
  service is unreachable, so checkout does not depend on the network for address lists.
- The default `sandbox` courier validates orders but creates no real parcel. Production must
  set a real courier key and its credentials before shipping real orders. A development
  process using the sandbox key may fall back to a local sandbox (`SBX-` tracking) when the
  service is unreachable; real courier keys never use that fallback.
- The COD amount is the order total in whole dinars. `freeShipping` is sent as true so the
  courier does not add its own delivery fee on top of the amount the store collects.
- `COURIER_API_URL` must use https in production. A courier 429 is returned as 429 with
  `Retry-After`; other upstream failures are returned as 502 without credentials or internal
  addresses.

## Abuse limits

In memory token buckets, keyed by client IP:

- Login and register: 10 requests per 15 minutes.
- Checkout: 20 requests per 10 minutes.
- General API: 120 requests per minute.
- Admin mutations: 60 requests per minute.

The buckets are cleaned every 10 minutes. They are per process. Before running more than one
API replica, move the limiter to a shared store. `TRUST_PROXY=true` makes the limiter use the
first `X-Forwarded-For` hop. Leave it false unless a trusted proxy sets that header.

The courier service has its own limits: 200 parcels per hour per IP, 1 000 per day, 60
tracking calls per minute and 60 desk lists per minute. Reference data is cached for a day.
Parcel creation and tracking are admin actions, so they stay well inside those limits.

## Headers

API:

- `Content-Type: application/json; charset=utf-8`
- `Cache-Control: no-store`
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: no-referrer`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`
- `Strict-Transport-Security` in production

SvelteKit document:

- `Content-Security-Policy` with a nonce for scripts, `'self'` for connect, img, font and
  form actions, `data:` for the inlined font subsets, `'unsafe-inline'` for styles only,
  `frame-ancestors 'none'`, `base-uri 'self'`, `object-src 'none'`.
- `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Permissions-Policy`.
- HSTS in production.

## Secrets and deployment

- `.env` is ignored. `.env.example` holds names only.
- The API refuses to start in production unless `TURSO_DATABASE_URL`, `TURSO_AUTH_TOKEN`,
  `APP_ORIGIN`, `ADMIN_EMAIL` and `ADMIN_PASSWORD` are set explicitly. The development admin
  password is rejected even though it is long enough.
- `SEED_DEMO` defaults to false in production. If it is enabled, the development customer
  password is rejected; set a unique one or leave the seed off.
- The deploy scripts apply the same checks before starting containers, force secure cookies in
  production, and print demo credentials only in development.
- Containers run as a non-root user, with a read-only root filesystem where possible.
- TLS terminates at the reverse proxy. The API port is not published to the public internet
  in production. Only the web container is exposed.
- Turso credentials live in the API environment, never in the web container.
- Logs redact the session cookie, the auth token and the password fields. Request logs keep
  method, path, status, duration and a request ID.

## Before production

- Replace the demo product photography and sample orders.
- Set `COURIER_KEY` and `COURIER_CREDENTIALS` for a real courier. The `sandbox` key creates
  no parcels and must not be used for live orders.
- Add email verification, password reset and a change-password flow.
- Move rate limiting to a shared store if the API runs more than one replica.
- Connect a real payment provider. Cash on delivery is the only method now.
- Add a real support inbox and privacy contact.
- Run a dependency and container scan, and set up log retention.
- Put the web and API behind a TLS reverse proxy and enable HSTS with a real domain.

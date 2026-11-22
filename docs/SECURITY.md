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
- The development Kiri start publishes the API on `127.0.0.1:8080` so the desktop admin can
  reach it directly. Production does not publish the API port; the container is reachable only
  on the private network. Point the desktop app at a TLS reverse proxy in that case.
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
  forwards the browser's origin. In development, local ingress origins (`localhost`,
  `127.0.0.1`, `::1` and `*.localhost`, which includes Kiri's `*.kiri.localhost`) are also
  accepted so local tooling can reach the app; production keeps the exact match. A missing
  origin is allowed only when `APP_ENV=development`.
- The SvelteKit hook sets a random `azahar_csrf` cookie on document requests. The cookie is
  readable by the page, `SameSite=Lax`, `Path=/` and `Secure` in production. It is a
  64-character lowercase hex token; the hook rotates any value that does not match that shape.
- A native client has no document, so `GET /api/csrf` issues the same cookie and returns the
  token. The desktop admin stores it in memory and sends it as `X-CSRF-Token`. Unsafe calls
  still need `X-Requested-With: azahar-web` and an `Origin` equal to `APP_ORIGIN`; the
  desktop reads the expected origin from `GET /api/health` when the user leaves it on auto.
- The desktop session cookie is stored in the OS keyring under a per-profile, per-server
  account, with a `0600` fallback when the keyring is unavailable or does not answer within
  a short timeout. Signing out removes the current server's entry. Changing the server
  switches the store and cannot replay the old server's session.
- Every unsafe request must also send `X-CSRF-Token` with exactly the cookie value. The API
  compares the two in constant time. A request without a matching pair returns `403`.
- The API does not enable CORS. Cross-origin reads are blocked by the browser.
- The desktop admin does not load the API in a webview. Its Go process makes the requests,
  keeps the cookie, and exposes only typed bindings to a renderer with `connect-src 'none'`.
- The Wails asset server adds a CSP header with `frame-ancestors 'none'`, `nosniff`, a
  no-referrer policy and cross-origin isolation headers. The renderer blocks non-http(s)
  navigation and drag-and-drop, and opens validated links in the system browser.
- The desktop client calls only `/api/` paths on the configured server. Methods are
  allowlisted, request bodies are capped at 2 MiB, response bodies at 16 MiB, and redirects
  are not followed. Images are fetched by Go and returned as bounded data URLs.
- Unsafe desktop requests require an origin. The client reads it from `GET /api/health` when
  the user leaves it on auto, falls back to the local origin only for a loopback API, and
  fails closed with a clear error for a remote server without a known origin.
- A non-loopback server needs explicit network consent tied to that host. Changing to a
  different host requires a fresh consent. A public host must use HTTPS. The app asks the
  user to allow it through the operating-system firewall and never changes firewall rules
  or requests administrator rights itself.

## Captcha, bot detection and terms

- `GET /api/checkout/challenge` issues a one-time local captcha. It requires
  `X-Requested-With: azahar-web`, so a cross-site image or form cannot burn a visitor's
  challenge budget. The code is five characters from an alphabet without `I`, `O`, `0` or
  `1`. The image is a rendered PNG, not text, and is returned as a data URL.
- A challenge expires after 10 minutes, is deleted on the first wrong answer or after one
  successful verification, and is bound to the client address when one is available. It must
  be at least two seconds old, so an instant form fill fails. A full store evicts the oldest
  challenge instead of failing new requests.
- The local captcha is a development fallback. Production requires Cloudflare Turnstile:
  `TURNSTILE_SECRET_KEY` on the API and `PUBLIC_TURNSTILE_SITE_KEY` on the web app. Checkout
  verifies the Turnstile token server-side and fails closed with `503 captcha_unavailable`
  when the verification service cannot be reached.
- The checkout form includes a hidden `website` honeypot. A filled value returns
  `403 bot_detected` before any order work.
- Checkout requires `terms_accepted: true` and the current `terms_version`. The accepted
  version and time are written to `audit_log` as `order.terms_accepted`, attributed to the
  signed-in user when there is one.
- The terms page is sample content for the demo. It is not legal advice and does not create a
  binding sale.

## Authorization

- `requireUser` loads the session and the user on every protected request.
- `requireStaff` allows `admin` and `employee` into the operational admin area. `requireAdmin`
  then guards every mutation that changes the catalogue, storefront, plugins or settings.
- Employees can read the dashboard, products, orders and customers, update order status and
  courier tracking, and change their own admin colour style. They cannot create, edit or
  archive products, upload images, change the storefront theme, manage plugins or cancel an
  order. Admin-only routes return `403` to an employee, and the admin UI hides those surfaces.
  See `docs/PERMISSIONS.md` for the matrix.
- A customer can read only their own orders. The public order endpoint is gated by a 256-bit
  token, not by an ID.
- Storefront theme writes and plugin settings are admin-only. The public theme and plugin
  manifest reads contain no secrets.
- Team role changes are admin-only and audited. An admin cannot remove their own admin role,
  and the last administrator cannot be demoted. The session user is reloaded on every
  request, so a demotion takes effect immediately.
- Plugin admin routes live under `/api/plugins/{id}/admin` and run the same `requireUser`,
  `requireAdmin` and mutation limit as the core admin. The SvelteKit proxy also requires the
  private gate cookie for that path shape.
- Admin actions are audited through `audit_outbox`, batched into `audit_log`: product
  create/update/archive, order status change, preference change, storefront theme change,
  plugin settings change and plugin enable/disable. Plugin settings audit entries list the
  changed keys, never the values.

## Audit durability

- Every audited action inserts one `audit_outbox` row inside the same database transaction
  as the change it records. The row commits or rolls back with the action, so a successful
  change can never lose its audit entry and a failed change cannot leave one behind. Business
  writes remain in their own transaction; the batching applies to the append-only audit
  stream.
- A background batcher copies up to 200 queued rows into `audit_log` with `INSERT OR IGNORE`
  and deletes those rows from `audit_outbox` in one transaction. A burst of audit entries
  becomes one `audit_log` transaction instead of one write per entry.
- The batcher flushes at startup, then every `AUDIT_BATCH_INTERVAL_MS` (default 2000,
  clamped to 500–60000). On failure the rows stay queued and are retried on the next tick.
  Rows committed before a restart or a temporary database outage are replayed on startup.
- `audit_log` can trail the action that produced it by at most one batch interval. The
  outbox is a write-through queue, not a second source of truth: once flushed, the entry is
  in `audit_log` and removed from the queue in the same transaction.

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
- Product image paths must be a bundled `/images/` path or an uploaded `/api/images/{id}`.
  This keeps the admin form from injecting `javascript:`, remote trackers or local file
  paths. Uploads are admin-only, multipart, capped at 5 MB, and accepted only when the bytes
  sniff as JPEG, PNG or WebP. The upload id is random and the served bytes are immutable.
- Plugin product options are validated twice: the admin may store only allowed values, and
  checkout compares the customer's choice with the product's stored values. Unknown fields
  and unlisted values are rejected. Core products reject options entirely.
- A limited-time sale is only active between `sale_starts_at` (inclusive) and
  `sale_ends_at` (exclusive), and only when the sale price is above zero and below the
  regular price. Checkout recomputes the effective price from the server clock and writes it
  into the order item; a client-supplied cart price is ignored.
- Storefront theme values are an allowlist: six-digit hex colours, bundled font ids, a
  bundled image path within `/images/`, bounded text and numeric ranges. No free-form CSS
  reaches the page. Both palettes must pass the contrast minimums in `docs/PLUGINS.md`.

## Plugins, themes and the AI provider

- Plugin settings are encrypted with AES-256-GCM before they reach the database. The key is
  derived from `PLUGIN_SECRET_KEY`; production refuses to start without one of at least 16
  characters. Secret values are masked in every response and never written to logs.
- A key change makes old plugin settings unreadable. The admin console asks for them again
  and replaces the row; the old ciphertext is not silently reinterpreted as plaintext.
- The AI provider base URL is operator-owned (`AI_BASE_URL`), not an admin field, so a
  compromised admin browser cannot point the server at an internal address. It must use
  https in production.
- The AI request carries only aggregate sales facts or product facts. Customer names, emails,
  phones, addresses, order numbers and tokens never enter the prompt. The API key travels
  only in the `Authorization` header and is not logged. Upstream responses are capped at
  2 MiB and 8000 characters, and the upstream body is never returned to the browser.
- Plugin routes are registered at startup. Duplicate ids, duplicate routes, admin routes that
  try to add the `/admin` prefix again, malformed option keys and unknown setting types fail
  the process instead of reaching a request.
- Plugin categories and storefront page paths are claimed once. Category slugs and page paths
  are unique across plugins; a product category must belong to the product's plugin.
- Plugin enable/disable state is admin-only, CSRF-protected and audited. Dependency graphs are
  validated at startup. Disabling a plugin with enabled dependents returns a warning until the
  admin confirms; the dependents become blocked, not silently enabled. Disabled routes return
  `404`, and the public catalog, storefront navigation and checkout all apply the same
  effective state. Data, settings and order-status hooks stay intact so existing orders remain
  correct.
- Product image uploads are admin-only, multipart, capped at 5 MB, and accepted only when the
  bytes sniff as JPEG, PNG or WebP. Images are stored in the database and served with
  immutable public cache headers only for successful responses.
- Plugin migrations are recorded per plugin and version. A plugin cannot edit another
  plugin's tables through the framework; it owns its own SQL.

## Code mode

- Code mode is not an MCP server and does not execute shell commands or arbitrary code. The
  model returns a typed JSON plan; the server accepts only five action types: theme update,
  product update, product archive, product create and order status.
- A preview plan is stored server-side and applied by id. The client cannot tamper with the
  actions. Applying claims the plan atomically so it cannot run twice, and all actions run in
  one transaction; a failure rolls back the plan. A plan is bound to the admin who previewed
  it. The baseline stores the exact product fields, order status and theme the model saw, and
  apply compares them inside the same write transaction, so a checkout, stock change or theme
  edit made after the preview is rejected as stale.
- Theme patches pass the storefront allowlist and contrast validator. Product creation checks
  the installed plugin, category ownership, slug uniqueness, image path and option attributes.
  Every applied action writes an audit entry with the plan id.
- Code mode deliberately stops at the storefront theme, the product catalogue and order
  status. Customer details, payments, plugin secrets and navigation are out of scope, so a
  plan cannot move money or exfiltrate credentials.
- The provider key is encrypted, never returned to the browser and never logged. Plans are
  admin-only; the AI cannot read customer PII through this plugin.

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
- Captcha challenge: 30 requests per 10 minutes.
- General API: 120 requests per minute.
- Admin mutations: 60 requests per minute.

The buckets are cleaned every 10 minutes and each limiter caps its map at 10,000 keys,
evicting the oldest when full. They are per process. Before running more than one API
replica, move the limiter to a shared store. `TRUST_PROXY=true` makes the limiter use the
first `X-Forwarded-For` hop. Leave it false unless a trusted proxy sets that header.

Pending orders hold their stock, and phone orders hold concrete IMEIs, from checkout until an
admin confirms or cancels them. This release has no automatic pending-order TTL, so confirm or
cancel pending orders promptly. Captcha and the checkout limit reduce deliberate stock holding
but do not remove it.

SvelteKit server-side catalog and order loads forward `getClientAddress()` as
`X-Forwarded-For`, so SSR traffic uses the visitor's bucket instead of one shared web-server
bucket. If the web app runs behind a reverse proxy, adapter-node must set `ADDRESS_HEADER`
and `XFF_DEPTH` so `getClientAddress()` sees the real address.

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

- `Content-Security-Policy` with a nonce for scripts, `'self'` for connect, font and
  form actions, `'self'` and `data:` for images, `'unsafe-inline'` for styles only,
  `frame-ancestors 'none'`, `base-uri 'self'`, `object-src 'none'`. When Turnstile is
  configured, `script-src`, `connect-src` and `frame-src` also allow
  `https://challenges.cloudflare.com`.
- `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Permissions-Policy`.
- HSTS in production.

## Secrets and deployment

- `.env` is ignored. `.env.example` holds names only.
- The API refuses to start unless `APP_ENV` is set explicitly to `development` or
  `production`. In production it also requires `TURSO_DATABASE_URL`, `TURSO_AUTH_TOKEN`,
  `APP_ORIGIN`, `ADMIN_EMAIL`, `ADMIN_PASSWORD`, `TURNSTILE_SECRET_KEY` and
  `PLUGIN_SECRET_KEY`. The development admin password is rejected even though it is long
  enough.
- The web app must also receive `PUBLIC_TURNSTILE_SITE_KEY` in production. A one-sided setup
  fails checkout rather than falling back to the weaker local captcha. Cloudflare test keys
  are rejected in production on both sides.
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
- Set `PLUGIN_SECRET_KEY` to a long random value and keep it stable. Changing it makes saved
  plugin settings unreadable.
- If the AI plugin is used, set a real provider key in Admin, Plugins, AI analyst and confirm
  `AI_BASE_URL` points at the provider you intend. Leave the plugin disabled otherwise.
- Set `COURIER_KEY` and `COURIER_CREDENTIALS` for a real courier. The `sandbox` key creates
  no parcels and must not be used for live orders.
- Add email verification, password reset and a change-password flow.
- Move rate limiting and captcha challenges to a shared store if the API runs more than one
  replica.
- Have a lawyer review the terms, privacy notice and consumer information for the law that
  applies before taking real orders. The sample terms are not legal advice.
- Connect a real payment provider. Cash on delivery is the only method now.
- Add a real support inbox and privacy contact.
- Run a dependency and container scan, and set up log retention.
- Put the web and API behind a TLS reverse proxy and enable HSTS with a real domain.

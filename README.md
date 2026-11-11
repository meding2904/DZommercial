# Azahar

A demo skincare storefront and a financial dashboard, built as one product.

- Public store: catalog, product pages, cart, cash-on-delivery checkout, order confirmation,
  customer accounts, policy pages, custom accessible dropdown filters, a persisted light and
  dark theme with a circular wipe from the toggle, and a custom 404 that keeps the house chrome.
- Admin: revenue and order analytics, order status workflow, product management, customer
  list, custom accessible status, category and image dropdowns, and four persisted colour presets.
  The admin is hidden behind a random gate printed by Kiri, never linked from the store.
- Backend: Go 1.27, chi, Turso/libSQL.
- Frontend: SvelteKit 3, Svelte 5 runes, hand-built SVG charts.
- Deployment: Kiri, the Docker-compatible container runtime already installed here.

Azahar is a fictional house. Products, prices, story and orders are sample data. The
photographs are Pexels stand-ins, credited in `CREDITS.md`. No testimonials, certifications,
clinical results or customer statistics are shown.

## Run it with Kiri

The repo ships a Kiri script that builds the API and web images and starts three containers
on a private network: the web app, the Go API and a local Turso/libSQL server.

```bash
./deploy/kiri-up.sh
```

Open http://localhost:3000.

| Role | URL | Email | Password |
|---|---|---|---|
| Admin | the gate printed by Kiri at startup | `admin@azahar.example` | `azahar-dev-admin-2026` |
| Customer | `/account` | `client@azahar.example` | `azahar-dev-client-2026` |

The admin gate looks like `http://localhost:3000/ops-<random>`. Open it once, then sign in at
the hidden `/admin/login`. Visiting `/admin` directly returns 404 without the gate cookie.
Kiri generates a new token and prints a new gate each time the stack starts.

The admin lives at `/admin`. Only the web container is published. The API and database stay
on the `azahar-net` network.

Stop the stack, keeping the database volume:

```bash
./deploy/kiri-down.sh
```

Kiri does not ship a `compose` command here, so the script uses `kiri build`, `kiri run`,
`kiri network` and `kiri volume` directly.

If Kiri's image builder is unavailable on the host (the buildkit helper fails to start),
`kiri-up.sh` falls back to the local-artifact runner automatically. You can also call it
directly. It builds the Go binary and the SvelteKit output on the host, then runs both in
Kiri containers with read-only bind mounts:

```bash
./deploy/kiri-local.sh
```

That path still runs the API, web app and Turso/libSQL server as Kiri containers on the
private `azahar-net` network. The Dockerfiles stay in `deploy/` for hosts where the Kiri
builder is healthy.

## Local development

Three terminals, or use the Makefile targets.

```bash
make dev-db     # local Turso/libSQL server on :8880
make dev-api    # Go API on :8080, migrations and demo seed
make dev-web    # SvelteKit on :5173
```

For local development without Kiri, the gate defaults to
`http://localhost:5173/ops-devtoken`. Set `ADMIN_ACCESS_TOKEN` to a random value to choose
a different gate. The admin UI and admin API return 404 until the gate cookie is set.

The web app proxies `/api` to the Go API, so the browser talks to one origin and session
cookies stay same-site.

Environment variables are listed in `.env.example`. The development defaults work without a
file. Production refuses to start without `TURSO_DATABASE_URL`, `TURSO_AUTH_TOKEN`,
`APP_ORIGIN`, `ADMIN_EMAIL` and `ADMIN_PASSWORD` set explicitly, rejects the development
admin password, and defaults `SEED_DEMO` to false. The admin gate checks normalised paths,
so encoded paths cannot skip it.

## Turso

The API uses the official pure-Go libSQL client:

- Local demo: `TURSO_DATABASE_URL=http://127.0.0.1:8880` or `http://azahar-db:8080`.
- Turso Cloud: `TURSO_DATABASE_URL=libsql://<database>-<org>.turso.io` with
  `TURSO_AUTH_TOKEN`.

Migrations are SQL files embedded in the API and applied at startup. The demo seeder is
idempotent and runs only when `SEED_DEMO=true` and the tables are empty. It creates 48 sample
orders across the last 45 days so the dashboard has a real series to read. The admin labels
this "Demo dataset".

## Tests and checks

```bash
make check    # svelte-check + go vet + go test
make build    # SvelteKit build + go build
make smoke    # public API smoke calls while the API is running
```

The API also has `backend/scripts/smoke.sh` for the full auth and checkout path, documented
in `backend/README.md`.

## Structure

```
backend/        Go API, migrations, seed, tests
frontend/       SvelteKit storefront and admin
deploy/         Dockerfiles and Kiri scripts
docs/           API contract and security notes
DESIGN.md       brand direction, dials, palette, motion, presets
CREDITS.md      photo sources and demo disclosure
.opencode/      antislop skills used during the build
```

## Security

The build implements Argon2id passwords, hashed session tokens, HttpOnly SameSite cookies,
origin and custom-header CSRF checks, per-IP rate limits, strict input validation, parameterized
SQL, transactional stock checks, RBAC, audit logging, CSP and the usual security headers.
The admin UI is not linked from the store. A random gate URL printed by Kiri sets an HttpOnly,
SameSite=Strict gate cookie for 12 hours; `/admin` and `/api/admin/*` return 404 without it.
The gate is an extra layer, not a replacement for the admin session. The threat model and
production checklist are in `docs/SECURITY.md`.

Cash on delivery is the only payment method. No card data is collected.

## Design

The storefront uses the taste skill's dealt palette and type pairing for a wellness product:
plaster and rose, Schibsted Grotesk, JetBrains Mono for figures. The public dial is ENERGY 2 /
RHYTHM 2 / MOTION 2: one entrance, two dealt moments, quiet hover and cart micro-interactions.
The dark theme keeps the same voice at night, with a first-party pre-paint script and a
circular theme reveal that grows from the header button to fill the screen. The View
Transitions API reveals the actual elements in the new theme, so the page does not reload.
Browsers without it get a solid ground wipe.

The dashboard is its own room: ENERGY 3 / RHYTHM 3 / MOTION 2. The revenue chart and the
attention rail are the focal points. The four colour presets change every token, including the
chart series, and are stored per admin user.

`DESIGN.md` records the reasons for the palette, type, layout, motion and preset decisions.

## Before production

- Replace the Pexels stand-ins with a real product shoot.
- Turn off the demo seed and remove the sample orders.
- Connect a real payment provider if card payment is required.
- Add email verification, password reset and a change-password flow.
- Keep `ADMIN_ACCESS_TOKEN` random and out of logs, tickets and screenshots.
- Never publish the API port; the web gate does not replace direct API authentication.
- Move rate limiting to a shared store if the API runs more than one replica.
- Serve over HTTPS, set `COOKIE_SECURE=true` and use a real domain.
- Replace `hello@azahar.example` with a real support address.

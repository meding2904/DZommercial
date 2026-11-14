# Azahar

Azahar is a demo online shop for a fictional skincare house, with a private admin area. It
is one product: a customer can browse the catalogue and place a cash-on-delivery order, and
the owner can see the takings, pack orders, create courier parcels and edit products.

This is a demonstration. The products, prices, story, orders and photographs are sample data.
No card payment is taken and the default courier setting does not create a real parcel.

## What you can do

### In the shop

- Browse the eight skincare formulas by category, concern or search.
- Open a product page with the description, ingredients, size, price and stock.
- Put products in the bag and change the quantity.
- Check out with a name, email, phone, wilaya and commune.
- Choose home delivery or a stop desk. A stop desk is a courier office where the customer
  collects the parcel.
- Place a cash-on-delivery order and get a private confirmation page. The checkout asks you to
  accept the terms and pass a short security check before the order is written.
- Create an account and see your own orders.
- Switch between the light and dark theme. The choice is saved in the browser.
- Read the shipping, returns, privacy and terms pages.

### In the admin

- See revenue, order counts, average order value, top products and low stock for the last
  7, 30 or 90 days.
- Search and filter orders by status, number, customer, phone or tracking number.
- Open an order and move it through the status steps.
- Create the parcel with the courier and refresh its tracking.
- Create, edit and archive products, with stock and price in dinars.
- See the customer list with orders and lifetime spend.
- Change the admin colour preset. The choice is saved per admin user.

The admin is not linked from the shop. It is reached through a private gate link printed
when the demo starts.

## Start the demo

You need this project folder, a terminal and Kiri. Kiri is the small container runtime already
installed on this machine. A terminal is the text window where you type commands.

1. Open a terminal in the project folder.
2. Run:

   ```bash
   ./deploy/kiri-up.sh
   ```

3. Wait for the build to finish. The first build can take a few minutes.
4. When the command prints `Azahar is running at http://localhost:3000`, open that address in
   a browser.

The command also prints the admin gate and the demo logins. Keep the terminal open, or note
the gate link.

If Kiri's image builder is unavailable, the script switches to the local build automatically.
You can also start that path directly:

```bash
./deploy/kiri-local.sh
```

Only the web app is published on port 3000. The API and database run in separate containers
on a private network. You do not need to open or configure them for normal use.

## Sign in

| Role | Where | Email | Password |
|---|---|---|---|
| Customer | http://localhost:3000/account | `client@azahar.example` | `azahar-dev-client-2026` |
| Admin | the gate link printed at startup, then `/admin/login` | `admin@azahar.example` | `azahar-dev-admin-2026` |

### The admin gate

The admin gate is a private link like:

```text
http://localhost:3000/ops-<random characters>
```

Open that exact link once. It sets a private cookie and redirects to the admin sign-in page.
After that, `/admin` works in that browser for about 12 hours.

If you open `/admin` without the gate, the site returns "Not found" on purpose. The admin is
not hidden because it is broken; this is the first layer of protection.

The gate changes every time the stack starts. If you lose the link, stop the stack and start
it again. The old link stops working and a new one is printed.

The demo passwords are public so anyone can try the demo. Change them before using the shop
for anything real.

## Stop, restart and reset

Stop the shop but keep the products, accounts and orders:

```bash
./deploy/kiri-down.sh
```

Start it again with `./deploy/kiri-up.sh`. The admin gate changes, but the data stays.

To wipe the demo data and start from a fresh seed:

```bash
./deploy/kiri-down.sh
kiri volume rm azahar-db
./deploy/kiri-up.sh
```

That removes the database volume. Products, accounts and orders return to the sample data
when the shop starts again. This cannot be undone.

## A tour of the shop

Start at `http://localhost:3000`.

1. Open **Shop the shelf**.
2. Use the category, concern and sort controls. On a phone, the menu opens from the header.
3. Open a product. Choose a quantity and add it to the bag.
4. Open the bag, then **Continue to checkout**.
5. Enter a name, email and an Algerian mobile number. For example, `0550 12 34 56`.
6. Choose a wilaya, then a commune. The commune list depends on the wilaya.
7. Choose **Home delivery** and enter a street address, or choose **Stop desk** and pick a
   courier desk. The desk sets the commune.
8. Place the order. The confirmation page is private to that order link. Keep it.
9. The order also appears under **Account** if you were signed in when you placed it.

Shipping is 600 DZD. Orders over 8,000 DZD ship free. The courier collects the total in cash
when the parcel arrives.

## A tour of the admin

1. Open the gate link from the terminal, then sign in with the admin email and password.
2. **Overview** shows the last 30 days. Use the 7, 30 and 90 day buttons.
3. **Orders** lists every order. Search by order number, name, email, phone or tracking
   number.
4. Open an order to see the customer, delivery details, items, totals and courier state.
5. Use **Next status** to move the order through `pending`, `confirmed`, `packed`, `shipped`
   and `delivered`, or cancel it.
6. **Courier** shows the tracking number and status. Use **Create shipment** after the order
   is ready, then **Refresh tracking** to pull the latest state.
7. **Products** lets you create or edit a product. Prices are entered in dinars. Use
   **Archive** to take a product off the shelf without deleting its old orders.
8. **Customers** shows the customer list, order count and lifetime spend.
9. The colour presets at the bottom of the admin rail change the admin theme. The choice is
   saved for that admin user.

The order status and the courier status are separate. Creating a parcel does not move the
order to `shipped`. Mark the order shipped when the courier collects it, then refresh tracking
as it travels.

## The courier step

Checkout uses the free [dzship API](https://freeship.dzbuild.com/) to list wilayas, communes
and stop desks. The admin uses it to create the parcel and fetch tracking. Courier calls are
made by the server, so courier credentials are never visible in the browser.

The default setting is the **sandbox** courier. It checks the same fields as a real courier
but creates no parcel. This is the right setting for trying the demo.

### Why the demo shows `SBX-` tracking numbers

The containers in this local setup cannot reach the internet. The shop tries the courier
service first. When that connection is not available:

- Checkout uses a built-in copy of the wilaya and commune list, so you can still place an
  order.
- In development with the sandbox courier, **Create shipment** and **Refresh tracking** use a
  local sandbox. The tracking number starts with `SBX-`, for example `SBX-AZ-2026-0041`.
- A real courier key never uses the local sandbox. It shows the courier error instead.

This fallback is deliberate. It keeps the demo usable on a machine with no outbound network.
On a normal server with internet access, the same code calls dzship directly.

### Use a real courier

You need an account with a courier supported by dzship, such as Yalidine, ZR Express, Maystro,
NOEST or one of the Ecotrack couriers. Then start the shop with the courier key and the
credentials supplied by that courier:

```bash
COURIER_KEY=yalidine \
COURIER_CREDENTIALS='{"apiId":"your-id","apiToken":"your-token"}' \
COURIER_FROM_WILAYA=16 \
./deploy/kiri-up.sh
```

- `COURIER_KEY` is the courier name from the dzship courier list.
- `COURIER_CREDENTIALS` is a JSON object. The exact field names depend on the courier.
- `COURIER_FROM_WILAYA` is the wilaya the parcels leave from. 16 is Alger.

The courier account and the server both need internet access. If the courier rejects the
parcel, the admin shows the message so you can correct the phone, commune or desk.

The shop's shipping price stays at its own flat rate. The courier collects the order total as
cash on delivery.

## Settings

Most people can keep the defaults. Settings are read from the environment when the shop
starts. You can pass one before the command, for example:

```bash
ADMIN_PASSWORD='a-long-private-password' ./deploy/kiri-up.sh
```

| Setting | Default | What it does |
|---|---|---|
| `APP_ENV` | `development` | Use `production` for a public HTTPS deployment |
| `APP_ORIGIN` | `http://localhost:3000` | The public address of the shop |
| `ADMIN_EMAIL` | `admin@azahar.example` | Admin sign-in email |
| `ADMIN_PASSWORD` | demo password | Admin sign-in password |
| `CUSTOMER_EMAIL` | `client@azahar.example` | Demo customer email |
| `CUSTOMER_PASSWORD` | demo password | Demo customer password |
| `ADMIN_ACCESS_TOKEN` | generated at start | The secret in the admin gate link |
| `COURIER_KEY` | `sandbox` | Courier name used for parcels |
| `COURIER_CREDENTIALS` | empty | JSON credentials for that courier |
| `COURIER_FROM_WILAYA` | `16` | Origin wilaya for parcels |
| `COURIER_API_URL` | `https://freeship.dzbuild.com` | Courier service address |
| `COURIER_TIMEOUT_SECONDS` | `12` | How long to wait for the courier |
| `SEED_DEMO` | true in development | Load the sample products, users and orders |
| `TURSO_DATABASE_URL` | local container | Database address |
| `TURSO_AUTH_TOKEN` | empty | Database token for a hosted database |
| `TURNSTILE_SECRET_KEY` | empty | Cloudflare Turnstile secret. Required in production |
| `PUBLIC_TURNSTILE_SITE_KEY` | empty | Cloudflare Turnstile site key. Required in production |

`.env.example` lists every setting. The Kiri scripts use the values in your shell, not a
hidden file.

For a public deployment, set `APP_ENV=production`, use HTTPS, set `APP_ORIGIN` to the real
address, set a real `ADMIN_PASSWORD`, use a real courier, and set both Cloudflare Turnstile
keys. Production uses Turnstile instead of the local development captcha. The API refuses to
start in production without the required settings, and it rejects the public demo password.
The full checklist is in `docs/SECURITY.md`.

## Common tasks

**Change a price or stock.** Admin, Products, open the product, change the price in dinars or
the stock count, then save.

**Add a product.** Admin, Products, create. Use a lowercase slug with hyphens, a category, a
price in dinars and an image path that starts with `/images/` or an `https://` address.

**Take a product off the shelf.** Admin, Products, Archive. Old orders keep their items.

**Find an order.** Admin, Orders, then search by order number, name, email, phone or tracking
number.

**Move an order forward.** Open the order, choose the next status, then save. The allowed
steps are shown; delivered and cancelled are final.

**Create the courier parcel.** Open the order, check the delivery details, then **Create
shipment**. The tracking number and status appear in the same panel.

**Change the admin gate.** Stop and start the stack. A new gate is printed and the old one
stops working. You do not need to edit anything.

**Change the shop theme.** Use the sun and moon button in the shop header. The admin has its
own colour presets at the bottom of the rail.

## Troubleshooting

| Problem | What to try |
|---|---|
| The page does not load | Check that the start command is still running. Run `kiri ps` and `./scripts/healthcheck.sh`. |
| The port 3000 is already in use | Close the other program using port 3000, then start the shop again. |
| The build fails | Run `./deploy/kiri-local.sh`. It builds on the host and skips the Kiri image builder. |
| The admin link returns "Not found" | Use the exact gate link from the latest start. The token changes on every restart. |
| The admin sign-in says "access denied" | Use the admin email, not the customer email. The admin account must have the admin role. |
| The checkout says the delivery map is unavailable | Wait a moment and use **Try again**. The courier service may be slow or unreachable. |
| There are no stop desks for a wilaya | Choose home delivery, or choose a different wilaya. Some couriers do not publish a desk list. |
| Create shipment fails | For the sandbox, check the API logs. For a real courier, check the courier key, credentials and internet access. |
| Login says "Too many requests" | Login and registration are limited to 10 tries per 15 minutes. Wait and try again. |
| I lost the admin gate | Stop and start the stack. A new gate is printed. The old link stops working. |
| I want the sample data back | Stop the stack, run `kiri volume rm azahar-db`, then start again. This deletes all data. |

Useful commands:

```bash
kiri ps                         # what is running
kiri logs azahar-api            # API messages and errors
kiri logs azahar-web            # web app messages
kiri logs azahar-db             # database messages
./scripts/healthcheck.sh        # quick check of the public pages and API
```

## Security in plain language

- Passwords are stored as one-way hashes, never as plain text.
- Sign-in sessions use a private cookie that browser scripts cannot read.
- The admin gate is a second, hidden link. It is an extra layer, not a replacement for the
  admin password.
- Forms are protected against cross-site request forgery.
- Login, checkout and the API have request limits.
- The API uses parameterised database queries.
- The site sets a content security policy and other browser protections.
- Courier credentials stay on the server and are not sent to the browser.
- The order confirmation link contains a private token. Do not post it publicly.

The demo passwords and the sample data are public. Do not use them for real customer data.
The detailed threat model is in `docs/SECURITY.md`. The last penetration test report is in
`docs/PENTEST.md`.

## For developers

The shop is a Go API, a SvelteKit frontend and a Turso/libSQL database, run in Kiri containers.

```bash
make check       # type-check the frontend, run the security tests, vet and test the Go code
make build       # build the frontend and the Go API
make smoke       # call the public API endpoints
make dev-db      # local database on :8880
make dev-api     # Go API on :8080
make dev-web     # SvelteKit development server on :5173
```

In local development without Kiri, the admin gate defaults to
`http://localhost:5173/ops-devtoken`. Set `ADMIN_ACCESS_TOKEN` to use a different value.

The API contract is in `docs/API.md`. The Go API notes are in `backend/README.md`. Database
migrations are in `backend/migrations/` and run at startup. The demo seeder is idempotent and
runs only when `SEED_DEMO=true` and the tables are empty.

Project layout:

```text
backend/        Go API, migrations, seed, tests
frontend/       SvelteKit shop and admin
deploy/         Dockerfiles and Kiri scripts
docs/           API contract, security notes, penetration test
DESIGN.md       brand direction, palette, motion, admin presets
CREDITS.md      photo sources and demo disclosure
scripts/        health check and admin-gate proof of concept
.opencode/      third-party antislop tooling used during the build
```

The shop is cash on delivery only. No card details are collected. Creating an account,
checking out, and using the admin are all local to this demo.

## License

The source code and written documentation in this repository are licensed under the Apache
License 2.0. See `LICENSE` and `NOTICE`.

The photographs are Pexels stand-ins and are **not** covered by the Apache License. They stay
under the Pexels license. See `CREDITS.md`.

The font packages are third-party software under the SIL Open Font License. The files under
`.opencode/skills` are third-party antislop tooling and keep their upstream terms. Software
dependencies keep their own licenses.

If you reuse this project, keep `LICENSE` and `NOTICE` with it.

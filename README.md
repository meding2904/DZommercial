# Azahar

**A modular storefront framework.** The skincare shop in this repository is a demo built on
it, not the product itself.

The framework provides the commerce core: catalogue, bag, cash-on-delivery checkout, accounts,
orders, courier parcels and the admin ledger. A plugin adds the parts a particular trade needs,
and the brand layer (copy, fonts, palette, imagery) belongs to the admin.

The demo is one configuration of those pieces: a fictional skincare house. A customer can
browse the catalogue and place an order; the owner can see the takings, pack orders, create
courier parcels, edit products, change the shop's design and configure plugins.

The framework code lives in `backend/`, `frontend/` and `desktop/`. The catalogue, accounts,
orders and photographs here are demo data.

Six plugins ship with the framework:

- **Oil shelf**: the skincare product type. Size, concerns, materials and instructions are
  plugin fields, not core columns.
- **Phone catalog**: paired memory and storage configurations plus colour, with every
  handset tracked by IMEI.
- **PC parts**: a catalogue of CPUs, graphics cards, motherboards, memory, storage, power
  supplies, cases and cooling.
- **PC builder**: a public "Build your PC" page that checks sockets, memory type, wattage,
  form factor and GPU length, then adds the selected parts to the bag.
- **AI analyst**: an admin-only analysis of store or product performance through an
  OpenAI-compatible endpoint, configured with an encrypted API key.
- **Code mode**: an admin AI console that plans storefront and catalogue changes, shows the
  plan, then applies it through validated actions with an audit trail.

The framework also ships with limited-time sale prices, five storefront style presets, an
employee role with a read-only catalogue, counter sales and order fulfilment, and a durable
audit outbox that flushes batched writes every two seconds. See `docs/PERMISSIONS.md` and
`docs/API.md`.

This is a demonstration. No card payment is taken and the default courier setting does not
create a real parcel.

## What you can do in the demo

### In the shop

- Browse the eight skincare formulas by category, concern or search.
- Open a product page with the description, ingredients, size, price and stock.
- Put products in the bag and change the quantity.
- Check out with a name, email, phone, wilaya and commune.
- Choose home delivery or a stop desk. A stop desk is a courier office where the customer
  collects the parcel.
- Place a cash-on-delivery order and get a private confirmation page. Open the receipt from
  that page and print it. The checkout asks you to accept the terms and pass a short security
  check before the order is written.
- Create an account and see your own orders.
- Switch between the light and dark theme. The choice is saved in the browser.
- Open **Build your PC** from the shop navigation, choose compatible parts for each slot and
  add the whole build to the bag.
- Read the shipping, returns, privacy and terms pages.

### In the admin

- See revenue, order counts, average order value, top products and low stock for the last
  7, 30 or 90 days.
- Search and filter orders by status, number, customer, phone or tracking number.
- Open an order and move it through the status steps.
- Create the parcel with the courier and refresh its tracking.
- Create, edit and archive products, with stock and price in dinars. The product form changes
  with the selected plugin: oils get size, concerns, materials and instructions; phones get
  paired memory/storage configurations and colour. Categories belong to the selected plugin,
  so the category menu follows it. New product images are uploaded by the admin, not chosen
  from a fixed list.
- See the customer list with orders and lifetime spend.
- Open **Team** to promote a customer to employee or administrator, or demote a staff
  account. The last administrator and your own admin role are protected. Employees can read
  the catalogue, take counter sales in the desktop app and fulfil orders, but cannot edit
  products, storefront, team or plugins.
- Open **Storefront** to change the brand name, copy, hero and story images, fonts, text
  scale, corner radius and the light and dark palettes. The editor refuses colours that
  fail the contrast minimums. Only an admin can change this.
- Open **Plugins** to see installed modules, save plugin settings and use plugin admin
  screens. Enable or disable an installed module here. Disabling a plugin that an enabled
  plugin depends on names the dependents and asks for confirmation; those plugins become
  blocked until the dependency is enabled again. **Code mode** lets an admin describe a
  storefront or catalogue change in plain language, review the proposed actions, then apply
  them with an audit trail. The AI plugin stores its API key encrypted and never sends
  customer data to the model. The phone plugin registers IMEIs for paired memory/storage
  configurations and shows reserved and sold units.
- Change the admin colour style and light or dark mode. The style is saved per admin user;
  the mode is stored on the device.

The admin is not linked from the shop. It is reached through a private gate link printed
when the demo starts.

## Start the demo

You need this project folder, a terminal and Kiri. Kiri is the small container runtime the
demo runs in; install it from https://kiri.dev if it is missing. A terminal is the text
window where you type commands.

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

Only the web app is published on port 3000. In development the API is also published on
`127.0.0.1:8080` so the desktop admin can reach it directly; the database stays on the private
network. A production start does not publish the API port.

## Sign in to the demo

| Role | Where | Email | Password |
|---|---|---|---|
| Customer | http://localhost:3000/account | `client@azahar.example` | `azahar-dev-client-2026` |
| Employee | the gate link printed at startup, then `/admin/login` | `staff@azahar.example` | `azahar-dev-staff-2026` |
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

## Desktop admin app

The repository also ships a Wails app for the owner's counter. It is framework code, not part
of the demo catalogue, and works with any deployment. It is the admin panel only, not a second
storefront: the ledger, a counter sale, orders, products and stock, customers and staff. A Go
backend talks to the API directly, and the static renderer has no network access of its own.
The SvelteKit web app is not in the path.

`desktop/` holds the Wails project. It uses the same API, roles and audit trail as the web
admin. The storefront editor, plugin settings and the AI screens remain in the web admin.

### Run it

Start the API and the web app first, from the repository root:

```bash
./deploy/kiri-up.sh
```

The development start publishes the API on `http://127.0.0.1:8080`. Then open a second terminal:

```bash
make desktop-deps   # first time only, installs the Wails CLI
make desktop
```

On Linux, install GTK 3 and WebKit2GTK 4.1 development packages first. The Makefile detects
WebKitGTK 4.1 and adds the `webkit2_41` build tag. If the app starts without a window on a
Wayland desktop, the WebKitGTK DMA-BUF renderer cannot use your GPU driver; start it with
`WEBKIT_DISABLE_DMABUF_RENDERER=1` and the window appears through the fallback path. This is a
WebKitGTK driver fallback, not an app setting.

Sign in with the admin account. The app opens the local API by default. If the API is on
another machine, open **Connection** on the sign-in screen.

### Connect to your own server

1. Enter the server root, for example `https://admin.example.com` or
   `http://192.168.1.20:8080`. Use the root, without a path.
2. Leave **Request origin** empty to read it from `GET /api/health`, or enter the exact
   `APP_ORIGIN` the API was started with.
3. For any address that is not loopback, tick **Allow this app to contact the server on the
   network**.
4. Save and retry. Use **Firewall help** if the operating system is blocking the connection.

A public host must use HTTPS. Plain HTTP is accepted only for loopback and private or local
hosts, because the admin password and the session cookie cross this connection. The app never
turns off TLS verification.

The app makes outbound requests from Go, not from the webview. Windows and macOS may ask
whether the app may use the network. On Linux, allow outbound traffic for the `azahar-admin`
binary in your firewall. If the API runs on another machine, its incoming port is usually the
one that needs a rule. The app does not change firewall rules and never asks for administrator
rights.

### What the owner can do

- **Ledger**: takings, previous-period comparison, orders, units, average order, revenue by
  day, the work queue, ranked products, order mix, low stock and recent orders.
- **Counter sale**: take cash for a walk-in, choose a phone configuration when a product needs
  one, record the sale, print the receipt. Stock drops and the order is written as a delivered
  `pickup` sale in one transaction.
- **Orders**: filter and search, open an order, move it through the allowed status steps,
  create the courier parcel and refresh tracking. Print a receipt for any online or counter
  order.
- **Products and stock**: price, stock, sale window and active state. The stock stepper writes
  through the API, so the web shop sees the same number.
- **Customers and staff**: the customer ledger, and role changes for administrators.

An employee can use the counter and fulfil orders. The API refuses catalogue and stock edits
for an employee, and the desktop app disables those controls rather than only hiding them.

### Build the binary

```bash
make desktop-build
make desktop-package
```

The native binary is written to `desktop/build/bin/azahar-admin`. Wails does not produce an
AppImage; the binary and the system GTK/WebKit runtime are the package. `make desktop-package`
writes a Linux tarball with the binary, desktop entry, icon and licence files. The Go client
keeps the session in the OS keyring when available, with a per-profile, per-server `0600`
fallback, and the webview only sees typed Wails calls.

The desktop app does not use the web admin gate. The gate protects the browser surface; the API
still enforces the session and the admin or employee role on every request. In production, do
not publish the API port. Put the API behind TLS and point the desktop app at that address.

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

## A tour of the demo shop

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

## A tour of the demo admin

1. Open the gate link from the terminal, then sign in with the admin email and password.
2. **Overview** shows the last 30 days. Use the 7, 30 and 90 day buttons.
3. **Orders** lists every order. Search by order number, name, email, phone or tracking
   number.
4. Open an order to see the customer, delivery details, items, totals and courier state.
   Use **Print receipt** for a clean paper copy.
5. Use **Next status** to move the order through `pending`, `confirmed`, `packed`, `shipped`
   and `delivered`, or cancel it.
6. **Courier** shows the tracking number and status. Use **Create shipment** after the order
   is ready, then **Refresh tracking** to pull the latest state.
7. **Products** lets you create or edit a product. Choose a plugin type; the form shows only
   that type's fields. Upload the main image and any gallery images. Prices are entered in
   dinars. Use **Archive** to take a product off the shelf without deleting its old orders.
8. **Customers** shows the customer list, order count and lifetime spend.
9. **Storefront** changes the shop's brand name, copy, hero and story images, fonts, radius,
   body text scale and both palettes. The preview and the contrast badges follow the form.
   Save writes the theme for every customer; the server rejects a palette that fails the
   minimum contrast.
10. **Plugins** lists the installed modules. Enable or disable a module here; if an enabled
    plugin depends on the one you are disabling, the screen names the dependents and asks
    for confirmation. A dependent shows as blocked until its dependency is enabled again.
    Open the AI plugin to save its API key, model and language, then run a store or product
    analysis. Open the phone plugin to register IMEIs for a paired memory/storage
    configuration, see what is in stock, reserved or sold, and find the IMEI reserved for an
    order. Open **Code mode** to describe a change, review the plan and apply it.
11. The colour styles at the bottom of the admin rail change the admin palette. Every style
    has its own light and dark mode; the sun and moon switch in the header changes the mode.
    The style is saved for that admin user; the mode is stored on the device.

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
| `APP_ENV` | `development` with Kiri | Use `production` for a public HTTPS deployment. The API refuses to start without it |
| `APP_ORIGIN` | `http://localhost:3000` | The public address of the shop |
| `ADMIN_EMAIL` | `admin@azahar.example` | Admin sign-in email |
| `ADMIN_PASSWORD` | demo password | Admin sign-in password |
| `CUSTOMER_EMAIL` | `client@azahar.example` | Demo customer email |
| `CUSTOMER_PASSWORD` | demo password | Demo customer password |
| `EMPLOYEE_EMAIL` | `staff@azahar.example` | Demo employee email |
| `EMPLOYEE_PASSWORD` | demo password | Demo employee password |
| `AUDIT_BATCH_INTERVAL_MS` | `2000` | How often queued audit rows are flushed, 500–60000 |
| `SESSION_TTL_HOURS` | `168` | Session lifetime in hours, up to 720. Admin and employee sessions are capped at 12 hours |
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
| `PLUGIN_SECRET_KEY` | development key | Encrypts plugin settings at rest. Required in production, 16 characters or more |
| `AI_BASE_URL` | `https://api.openai.com/v1` | OpenAI-compatible endpoint used by the AI plugin. Must use https in production |
| `AI_TIMEOUT_SECONDS` | `25` | How long to wait for the AI provider |

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

**Add a product.** Admin, Products, create. Choose the plugin type, fill only the fields
that type asks for, upload the main image and any gallery images, then save.

**Take a product off the shelf.** Admin, Products, Archive. Old orders keep their items.

**Find an order.** Admin, Orders, then search by order number, name, email, phone or tracking
number.

**Move an order forward.** Open the order, choose the next status, then save. The allowed
steps are shown; delivered and cancelled are final.

**Print a receipt.** Open the order in the admin, then **Print receipt**. The customer can
open the same receipt from the private confirmation page. The desktop app prints receipts
for counter sales too.

**Create the courier parcel.** Open the order, check the delivery details, then **Create
shipment**. The tracking number and status appear in the same panel.

**Change the admin gate.** Stop and start the stack. A new gate is printed and the old one
stops working. You do not need to edit anything.

**Change the shop theme.** Use the sun and moon button in the shop header. The admin has four
colour styles at the bottom of the rail, each with its own light and dark mode; use the sun
and moon button in the admin header to switch the mode.

**Change the shop brand.** Admin, Storefront. Edit the name, copy, hero and story images,
fonts, scale, radius and palettes, then Save. The shop reads the saved theme on every page.
Only an admin can open this screen.

**Sell a phone.** Create a product, set its type to Phone, and add one or more paired
memory/storage configurations and colours. Then open Admin, Plugins, Phone catalog, choose the
product, pick a pair and colour, and register an IMEI for each unit. Checkout reserves an IMEI
in the order and refuses a pair with no registered unit.

**Sell PC parts and open the builder.** Create products with the PC part type and put each in
a PC parts category such as CPU, graphics card or motherboard. Fill the optional socket,
memory type, form factor, wattage, TDP and length fields. The **Build your PC** link appears
in the shop navigation from the PC builder plugin.

**Let AI operate the admin.** Admin, Plugins, Code mode. Save an API key and model, describe a
storefront or catalogue change, review the proposed actions, then apply them. Plans are
audited; the AI cannot run shell commands or touch secrets.

**Configure the AI analyst.** Admin, Plugins, AI analyst. Save the API key, model and answer
language. Run "Analyse the store" for 7, 30 or 90 days, or pick a product. The key is stored
encrypted and never returned to the browser.

**Add a plugin.** Read `docs/PLUGINS.md`. A plugin declares a manifest, optional product
type, routes, migrations, settings and checkout hooks. Register it in
`backend/cmd/api/main.go`; the storefront renders its product options with no new component.

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
- Plugin settings are encrypted at rest with `PLUGIN_SECRET_KEY`. Secret values never
  return to the browser after they are saved.
- The AI analyst sends only product facts or aggregate sales figures. Customer names,
  emails, phones, addresses and order tokens are never sent.
- Storefront colours and fonts come from a validated allowlist, not from arbitrary CSS.
- The order confirmation link contains a private token. Do not post it publicly.

The demo passwords and the sample data are public. Do not use them for real customer data.
The detailed threat model is in `docs/SECURITY.md`. The last penetration test report is in
`docs/PENTEST.md`.

## For developers

The framework is a Go API, a SvelteKit 3 frontend and a Turso/libSQL database, run in Kiri
containers. The demo shop is one deployment of it.

```bash
make check       # frontend type-check and tests, Go vet and tests, Wails app tests
make build       # build the frontend and the Go API
make smoke       # call the public API endpoints
make dev-db      # local database on :8880
make dev-api     # Go API on :8080
make dev-web     # SvelteKit development server on :5173
make desktop-deps   # install the Wails CLI
make desktop     # run the Wails admin app against the local API
make desktop-build  # native Wails binary in desktop/build/bin/
make desktop-package  # Linux tarball with binary, desktop entry and icon
make loadtest    # short read probe (100 requests; LOADTEST_ARGS="-c 50 -n 5000" for more)
```

In local development without Kiri, the admin gate defaults to
`http://localhost:5173/ops-devtoken`. Set `ADMIN_ACCESS_TOKEN` to use a different value.

The API contract is in `docs/API.md`. The plugin and theme guide is in `docs/PLUGINS.md`.
The load probe and the last local results are in `scripts/loadtest/README.md`. The Go API
notes are in `backend/README.md`. Database migrations are in
`backend/migrations/` and run at startup; plugin migrations run after them. The demo seeder
is idempotent and runs only when `SEED_DEMO=true` and the tables are empty, so demo data
never mixes with an existing database.

The core never imports a plugin package. `backend/internal/plugins` defines the extension
surface, `backend/internal/storefront` defines and validates the brand layer, and the shipped
plugins live under `backend/internal/plugins/oils`, `pcparts`, `pcbuilder`, `codemode`,
`phone` and `ai`. The storefront renders any plugin product type from its `option_schema`;
plugin pages and admin screens use small registries, not core imports.

Project layout:

```text
backend/        Go API, migrations, seed, tests, plugin SDK and first-party plugins
frontend/       SvelteKit storefront, admin and plugin UI
desktop/        Wails admin app, Go API client, session and network settings
deploy/         Dockerfiles and Kiri scripts
docs/           API contract, permissions, plugin and theme guide, security notes, penetration test
DESIGN.md       brand direction, palette, motion, admin styles
CREDITS.md      photo sources, font sources and demo disclosure
scripts/        health check, admin-gate proof of concept, load probe and screenshot helper
.opencode/      third-party antislop tooling used during the build
```

The framework ships with cash on delivery only. No card details are collected. The demo
shop, accounts and admin all run locally.

## Upcoming

- WhatsApp Business API integration for order and delivery updates. Not implemented in this
  demo.

## License

The source code and written documentation in this repository are licensed under the Apache
License 2.0. See `LICENSE` and `NOTICE`.

The photographs are Pexels stand-ins and are **not** covered by the Apache License. They stay
under the Pexels license. See `CREDITS.md`.

The font packages are third-party software under the SIL Open Font License. The files under
`.opencode/skills` are third-party antislop tooling and keep their upstream terms. Software
dependencies keep their own licenses.

If you reuse this project, keep `LICENSE` and `NOTICE` with it.

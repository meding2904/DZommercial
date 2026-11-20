# The storefront framework

Azahar is a small commerce framework with a working skincare shop on top. The core does
catalog, cart, checkout, accounts, orders, shipping and the admin ledger. Everything a
particular trade needs beyond that is a plugin, and the shop's brand layer is an
admin-owned theme.

Nothing in the core catalog, checkout or admin imports a plugin package. The API talks to
the `internal/plugins` interface, and the storefront renders the option schema that
interface publishes.

## What is in the box

| Part | Path | Job |
|---|---|---|
| Core API | `backend/internal/httpapi` | Auth, catalog, checkout, orders, admin, theme and plugin console |
| Plugin SDK | `backend/internal/plugins` | Manifest, routes, migrations, hooks, encrypted settings |
| Oils plugin | `backend/internal/plugins/oils` | Size, concerns, materials and instructions for a formula |
| Phone plugin | `backend/internal/plugins/phone` | Paired memory/storage configurations and colour plus per-unit IMEI stock |
| PC parts plugin | `backend/internal/plugins/pcparts` | CPUs, graphics cards, boards, memory, storage, power, cases and cooling |
| PC builder plugin | `backend/internal/plugins/pcbuilder` | The public Build your PC page and its parts API |
| Code mode plugin | `backend/internal/plugins/codemode` | Admin AI planning and audited storefront/catalogue actions |
| AI plugin | `backend/internal/plugins/ai` | Admin-only store and product analysis through an OpenAI-compatible endpoint |
| Theme model | `backend/internal/storefront` | Validated brand copy, fonts, palette and imagery |
| Storefront theme UI | `frontend/src/routes/admin/(panel)/storefront` | The admin-only design editor |
| Plugin console | `frontend/src/routes/admin/(panel)/plugins` | Installed modules, settings and plugin admin screens |
| Generic option UI | `frontend/src/lib/components/store/ProductOptions.svelte` | Renders any plugin product type as radio chips |
| Plugin admin registry | `frontend/src/lib/plugins/admin/registry.ts` | Maps a plugin id to an optional admin component |
| Plugin order panel | `frontend/src/lib/plugins/admin/order-registry.ts` | Maps a plugin id to an optional panel inside the admin order dialog |

## The plugin contract

A plugin is a Go type with a manifest and a route list. Everything else is optional.

```go
type Plugin interface {
    Manifest() Manifest
    Routes() []Route
}

type Migrator interface {
    Migrations() []Migration
}

type CheckoutHook interface {
    ValidateCheckoutLine(ctx context.Context, tx *sql.Tx, line CheckoutLine) *Problem
    OnOrderCreated(ctx context.Context, tx *sql.Tx, order OrderRef, lines []CheckoutLine) error
}

type OrderStatusHook interface {
    OnOrderStatusChanged(ctx context.Context, tx *sql.Tx, orderID, from, to string) error
}

type ProductEnricher interface {
    EnrichProduct(ctx context.Context, product store.Product) (map[string]any, error)
}

type SettingsValidator interface {
    ValidateSettings(settings map[string]string) map[string]string
}

type EnvAware interface {
    Init(*Env)
}
```

A manifest carries the id, name, version, description, an optional product type, optional
settings fields and an optional admin path. The product type is what the storefront and the
product editor use:

```go
ProductType: &plugins.ProductType{
    Label:   "Phone",
    Summary: "Every handset is tracked by IMEI.",
    Fields: []plugins.OptionField{
        {Key: "config", Label: "Memory and storage", Type: "select", Required: true},
        {Key: "color", Label: "Color", Type: "select", Required: true},
    },
}
```

`OptionField.Type` is `select` for customer choices, `text` for one admin value,
`textarea` for a long admin value, and `list` for a list of admin values. Select fields
become radio chips on the product page; text, textarea and list fields become specs,
sections or lists. The product editor renders the matching control for each type.

### Categories

A plugin declares its categories in the manifest:

```go
Categories: []plugins.Category{
    {Slug: "face", Name: "Face", Description: "Face formulas.", SortOrder: 1},
    {Slug: "body", Name: "Body", Description: "Body formulas.", SortOrder: 2},
}
```

The core syncs those rows into the `categories` table with the plugin id on startup. Category
slugs are unique across plugins. A product's category must belong to the same plugin as the
product; the admin form only offers that plugin's categories, and the API rejects a mismatch.
`phone` owns `phones`, `oils` owns `face`, `body` and `ritual`.

### Storefront pages

A plugin can add a public navigation page:

```go
StorefrontPages: []plugins.StorefrontPage{
    {Path: "/build-your-pc", Label: "Build your PC", SortOrder: 10},
}
```

The store layout reads plugin manifests and adds those links to the header and mobile menu.
The path must be public, clean and not reserved by a core route. The frontend still needs a
matching route; the page can call the plugin's public API.

### Registration

`backend/cmd/api/main.go` builds the registry before the HTTP server:

```go
registry := plugins.NewRegistry()
for _, plugin := range []plugins.Plugin{
    oils.New(), pcparts.New(), pcbuilder.New(), codemode.New(registry), phone.New(), ai.New(),
} {
    if err := registry.Register(plugin); err != nil {
        logger.Error("plugin registration failed", "error", err)
        os.Exit(1)
    }
}
if err := registry.ValidateDependencies(); err != nil {
    logger.Error("plugin dependency check failed", "error", err)
    os.Exit(1)
}
```

Registration validates the id, routes, product type and setting schema. Duplicate ids,
duplicate routes, routes that include the `/admin` prefix and malformed field keys fail at
startup, not at request time.

A manifest can declare `Dependencies`. Dependencies are checked after every plugin is
registered: unknown ids, self-dependencies, duplicates and cycles stop the process. The
shipped PC builder depends on PC parts:

```go
Dependencies: []string{"pcparts"}
```

### Enable and disable

The admin Plugins screen can enable or disable any installed plugin. The flag is stored in
`plugin_states` and loaded at startup, so it survives restarts. Disabling a plugin with
enabled dependents shows those dependents and requires an explicit confirmation
(`confirm_dependents: true`). A dependent keeps its desired enabled state but becomes
`blocked` (`effective_enabled: false`) until the dependency is enabled again. Enabling a
plugin whose dependency is disabled returns `409 plugin_dependency`.

A disabled or blocked plugin keeps its data, settings and migrations. Its routes return 404,
its storefront pages disappear from navigation, its categories and products are hidden from
the public catalog, and checkout rejects its products. Order-status hooks still run, so
cancelling or delivering an existing order keeps stock and IMEIs correct. Re-enabling brings
the surfaces back. The state is per API process; when more than one replica is running,
restart the others after a change.

The HTTP server creates one `plugins.Env` and calls `Init` on every plugin that implements
`EnvAware`. `Env` carries the database, store, logger, encryption key, AI endpoint and
request helpers. A plugin never needs the HTTP server type.

### Routes

A public route is mounted at `/api/plugins/{id}{pattern}`. An admin route is mounted at
`/api/plugins/{id}/admin{pattern}` and runs behind the same session check, admin role check,
CSRF check and admin mutation limit as the core admin.

Handlers receive `*plugins.Context`:

```go
func (p *Plugin) handleList(c *plugins.Context) {
    c.JSON(http.StatusOK, map[string]any{"items": []string{}})
}

func (p *Plugin) handleCreate(c *plugins.Context) {
    payload, bad := plugins.DecodeBody[createPayload](c)
    if bad != nil {
        c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
        return
    }
    c.Error(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.")
}
```

`Problem` uses the same error envelope as the core. `DecodeBody` enforces JSON, unknown
field rejection and one object per request.

### Migrations

`Migrations()` returns SQL owned by the plugin. The core records each file in
`plugin_migrations` keyed by plugin id and version. Never edit an applied migration; add a
new version. Migrations run in one transaction per file and the runner splits on `;`, so do
not put semicolons inside comments or string literals.

### Checkout and order hooks

The checkout transaction gives a plugin two moments:

1. `ValidateCheckoutLine` after the product and its options are validated, before stock is
   decremented. Return a `*Problem` to stop the order with a field message.
2. `OnOrderCreated` after the order, items and stock decrement are written, still in the
   same transaction. A returned error rolls the whole order back.

`OnOrderStatusChanged` runs inside the admin status transaction. The phone plugin uses it to
release reserved IMEIs when an order is cancelled and mark them sold when it is delivered.
No webhook, no eventual consistency, no second write to reconcile.

`EnrichProduct` adds read-only data to the product DTO as `plugin_data`. The phone plugin
uses it for in-stock and reserved device counts. A failure here is ignored by the catalog,
so a plugin query problem cannot take the shelf down.

### Settings

Settings are declared in the manifest. The core owns the routes, validation, encryption and
audit:

- `GET /api/admin/plugins` lists manifests with `configured` and `settings_state`.
- `GET /api/admin/plugins/{id}/settings` returns non-secret values and `secrets_set`.
- `PUT /api/admin/plugins/{id}/settings` validates, encrypts and stores the values.

Secret values are encrypted with AES-GCM using `PLUGIN_SECRET_KEY` and are never returned to
the browser. A blank secret field means keep the saved value. A key change makes old values
unreadable; the admin console asks for them again and overwrites the old row.

Settings fields support `text`, `secret`, `number`, `boolean` and `select`. Plugin-specific
rules go in `ValidateSettings`; core rules cover required fields, length, type and allowed
options.

## Add a plugin

1. Create `backend/internal/plugins/coffee/coffee.go` with a type implementing `Manifest`
   and `Routes`.
2. Give it a product type if it needs customer choices, for example `roast` and `grind`.
3. Add migrations for its own tables. Prefix every table with the plugin id.
4. Register it in `cmd/api/main.go`.
5. If it needs an admin screen, add `AdminPath` and a component under
   `frontend/src/lib/plugins/admin/`, then add it to the registry.
6. If it needs to show data beside an order, add a component to `order-registry.ts`. Order
   items carry `plugin_id`, so the dialog only mounts the panels for that order.
7. Add tests with no database for validation and one integration test guarded by
   `AZAHAR_TEST_DATABASE_URL` for the transactional path.
8. Document its endpoints in `docs/API.md`.

The storefront needs no per-plugin product component. It reads `option_schema` from the
product and renders the chips, stores the choices, sends them to checkout and shows the
label on the order. Only an admin surface is plugin-specific.

## The theme layer

The storefront design is data, not source code. Only an admin can change it:

- `GET /api/storefront/theme` is public and returns the current theme.
- `GET /api/admin/storefront/theme` returns the theme and the shipped defaults.
- `PUT /api/admin/storefront/theme` validates and saves; every change is audited.

A theme carries:

- Identity: brand name, tagline, short description, contact email, announcement.
- Catalog: show or hide the concern filter.
- Home copy: hero heading, body, alt text, CTA, image, statement, shelf heading, shop title, house nav label.
- Story: about heading, body, alt text, image, closing heading, footer note, footer credit.
- Type: display font, body font, body text scale (0.85 to 1.15), corner radius (0 to 12).
- Light and dark palettes: ground, surface, ink, muted, accent, accent ink.

The fonts are bundled: Schibsted Grotesk, Source Serif 4, Fraunces, Inter, JetBrains Mono
and the system sans stack. No remote font request is made.

The server rejects unsafe or unreadable themes:

- Colours must be six-digit hex. Nothing is interpolated into CSS unchecked.
- Text fields have length limits and reject control characters.
- Images must be a bundled `/images/...` path. Theme text cannot point at a remote tracker.
- Body text must reach 7:1 on the ground, muted text 4.5:1, ink on surface 4.5:1, accent on
  ground 4.5:1 and accent ink on accent 4.5:1, in both light and dark.
- Unknown JSON fields are rejected, so a typo cannot silently disappear.

The store layout loads the theme on the server and writes its tokens into the document head
as CSS variables. The admin preview uses the same `contrastRatio` and font stacks, so the
editor shows the same numbers the server enforces.

The editor includes a **Store style** dropdown with five starting points: Plaster, Noir,
Olive, Tidal and Clay. A style applies only the light and dark palettes, the display/body
font pairing and the corner radius; brand copy and images stay untouched. Every color pair in
the presets passes the same contrast checks as a hand-edited palette.

All plugin admin selects use the shared `Dropdown` component, and the public Build your PC
slots use it too. There are no native `<select>` controls left in the storefront or admin.

- Shop title is admin copy and feeds the shop page H1, page title and footer link.

The shipped home page is hero, statement, shelf, categories, story and closing CTA. The old
fixed three-chapter editorial block and ingredient list were removed because they were brand
content that an admin could not edit. A brand that wants more editorial sections can add
them as a plugin surface or a route with its own theme fields.

## Oils plugin

The skincare catalogue is a plugin, not core. The Oil product type declares:

- `size`: one text value, shown as a spec row.
- `concerns`: a list, shown as tags and used by the shop concern filter.
- `ingredients`: a list, shown as tags.
- `how_to`: a textarea, shown as a paragraph section.

The admin product form shows only the selected plugin's fields. A phone product does not show
oil fields, and an oil product does not show memory/storage or colour. Existing demo products
are migrated to the Oil plugin and their core columns are copied into plugin attributes.

## Product images

Admins upload product images; they do not choose from a fixed list. The API stores the bytes
in `product_images` and serves them at `/api/images/{id}`. The upload is admin-only,
multipart, capped at 5 MB, and accepts JPEG, PNG or WebP by content sniffing. Product image
paths accept `/images/...` for bundled demo assets and `/api/images/...` for uploads.

## Limited-time sales

A product can carry a sale price and a start and end time. The sale is active from the start
(inclusive) until the end (exclusive); outside that window the regular price applies. The
sale price must be above zero and below the regular price, and the window must end after it
starts. The admin product form exposes the sale fields and `PATCH` accepts them.

The public product DTO carries `sale_price_cents`, `sale_starts_at`, `sale_ends_at`,
`effective_price_cents` and `on_sale`. Cards show a Sale tag, the product page shows the
original price and the end date, and checkout recomputes the effective price on the server.
The client cart price is display only and never becomes the order price.

## Phone plugin

- Product type: `config` ("Memory and storage", one paired value such as
  `12 GB RAM + 256 GB`) and `color`, each required. Storage and RAM are never independent
  choices, so an impossible pair cannot be ordered.
- `GET /api/plugins/phone/admin/devices?product_id=` lists devices and counts.
- `POST /api/plugins/phone/admin/devices` takes
  `{product_id, config, color, imei}` and registers one IMEI for that pair.
  The IMEI must be 15 digits and pass the Luhn checksum; it is unique.
- `DELETE /api/plugins/phone/admin/devices/{id}` removes an in-stock device only.
- `GET /api/plugins/phone/admin/orders/{orderID}/devices` lists the units reserved or sold
  for an order.
- Checkout checks in-stock IMEIs for the chosen pair, reserves them in the order
  transaction, releases them on cancellation and marks them sold on delivery.

The product `stock` stays the customer-facing count. The IMEI count is the harder check: a
handset cannot be sold unless a registered unit exists for that exact configuration. The
storefront disables a value when no registered unit exists for the current selection, and the
product page reads its variant counts from `plugin_data`. The phone plugin recomputes each
phone product's `stock` from its in-stock IMEIs at startup and whenever a device is added or
removed, so the two counts do not drift apart. To pack an order, open Plugins, Phone catalog:
each reserved unit shows its order number, configuration and IMEI.

Variants share the product price in this framework. Per-variant pricing is not part of the
core: a brand that needs 512 GB to cost more can model it as a separate product.

## PC parts and PC builder

`pcparts` owns the PC part product type and eight categories: `cpus`, `gpus`,
`motherboards`, `memory`, `storage`, `psus`, `cases` and `cooling`. Its optional text fields
are `socket`, `memory_type`, `form_factor`, `interface`, `tdp_watts`, `wattage_watts`,
`length_mm`, `max_gpu_length_mm` and `supported_form_factor`. They are product details, not
customer choices.

`pcbuilder` adds `GET /api/plugins/pcbuilder/parts` and a `/build-your-pc` storefront page
declared through `StorefrontPages`. The page picks one active `pcparts` product per slot and
checks:

- CPU socket against the motherboard socket.
- Memory type against the motherboard.
- PSU wattage against CPU and GPU TDP plus 150 W headroom.
- Motherboard form factor against the case support list.
- GPU length against the case clearance.
- Cooler socket against the CPU socket.

When the build is compatible, the page adds every selected part to the bag. A missing spec
produces a plain blocker instead of a guess.

## Code mode plugin

`codemode` is an admin AI console. It does not run shell commands and is not an MCP server.
The admin writes an instruction; the model returns one JSON plan with a summary and up to ten
strictly typed actions:

- `theme.update` with an allowlisted theme patch.
- `product.update` with name, subtitle, description, price, stock or active.
- `product.archive`.
- `product.create` with name, slug, price, category, plugin, stock, image and plugin
  attributes.
- `order.status` with an order id and a permitted next status. Cancelling restores stock and
  runs the same order hooks as the manual admin action.

The plan is stored as a preview. Applying it is a separate admin request; the server
revalidates the plan, claims it atomically so it cannot run twice, and applies every action in
one transaction. A failure rolls the plan back. Each action writes an audit entry containing
the plan id. Theme patches go through the same `storefront.Validate` contrast and allowlist
checks as the theme editor; product creation goes through the same plugin/category/attribute
rules.

Settings: API key (secret), model, enabled. Endpoints:

- `POST /api/plugins/codemode/admin/preview` `{instruction}`
- `POST /api/plugins/codemode/admin/apply` `{plan_id}`
- `GET /api/plugins/codemode/admin/plans?limit=`

The boundary is deliberate. Code mode manages the storefront theme, the product catalogue and
order status. It cannot change customer details, payments, plugin secrets or navigation. Those
surfaces stay manual or plugin-owned so an AI plan never becomes a payment or identity
operation.

## AI plugin

- Settings: API key (secret), model, answer language, enabled.
- `POST /api/plugins/ai/admin/analyze` accepts `{kind:"store", days:7|30|90}` or
  `{kind:"product", product_id}`.
- `GET /api/plugins/ai/admin/runs?limit=` returns recent analyses.

The provider endpoint is operator-owned `AI_BASE_URL`, defaulting to
`https://api.openai.com/v1`. It is not editable from the browser. The server sends only
aggregate sales facts or product facts, never customer names, emails, phones, addresses,
tokens or order numbers. The API key is only used in the `Authorization` header and is never
logged. Responses are capped at 2 MiB and 8000 characters.

A missing key, a disabled plugin, an unreachable provider and a malformed provider response
each get their own plain error. The browser never sees the upstream body.

## Test the framework

```bash
make check

# Transactional phone lifecycle against a local libSQL server:
cd backend
AZAHAR_TEST_DATABASE_URL=http://127.0.0.1:8880 go test ./internal/plugins/phone/...
```

The unit tests cover the registry, option validation, theme contrast, encryption, IMEI
checks, settings masking and the AI client. The integration test proves reservation,
cancellation release and delivery sale against a real database.

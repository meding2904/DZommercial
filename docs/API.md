# Azahar API contract

The Go service listens on `API_ADDR` (default `:8080`). The SvelteKit server proxies
`/api/*` to `INTERNAL_API_URL`, so the browser only talks to one origin.

All requests and responses are JSON. Money is an integer number of centimes. Dates are
RFC 3339 strings in UTC.

## Session and request rules

- Cookie: `azahar_session`, `HttpOnly`, `SameSite=Lax`, `Path=/`, `Secure` in production.
- CSRF cookie: `azahar_csrf`, a 64-character lowercase hex token, readable by the page,
  `SameSite=Lax`, `Path=/`, `Secure` in production.
- `GET` and `HEAD` are safe.
- Every `POST`, `PATCH`, `PUT` and `DELETE` must carry `X-Requested-With: azahar-web`, an
  `Origin` header equal to `APP_ORIGIN`, and `X-CSRF-Token` with the same value as the
  `azahar_csrf` cookie. The proxy forwards all three.
- Unauthenticated protected request: `401`.
- Authenticated but not an admin: `403`.
- Invalid input: `422`.
- Rate limited: `429` with `Retry-After`.
- Unknown route: `404`.
- Wrong method: `405`.

Error envelope:

```json
{
  "error": {
    "code": "validation_error",
    "message": "Check the marked fields.",
    "fields": { "email": "Enter a valid email." }
  }
}
```

## Public catalog

### `GET /api/categories`

Returns categories with at least one active product. `?all=1` also returns empty categories,
for the admin product form.

```json
{ "categories": [ { "slug": "face", "name": "Face", "description": "...", "plugin_id": "oils" } ] }
```

### `GET /api/products?category=face&concern=dry&q=oil&sort=featured`

`sort` is one of `featured`, `price_asc`, `price_desc`, `new`. Unknown values fall back to
`featured`. Only active products are returned to the public.

```json
{
  "products": [
    {
      "id": "prd_01",
      "slug": "orange-blossom-cleansing-oil",
      "name": "Orange blossom cleansing oil",
      "subtitle": "The day, lifted",
      "description": "...",
      "price_cents": 480000,
      "compare_at_cents": null,
      "sale_price_cents": null,
      "sale_starts_at": "",
      "sale_ends_at": "",
      "effective_price_cents": 480000,
      "on_sale": false,
      "currency": "DZD",
      "size": "100 ml",
      "category": { "slug": "face", "name": "Face", "plugin_id": "oils" },
      "concerns": ["dry", "sensitive"],
      "ingredients": ["Olive squalane", "Orange blossom", "Sweet almond"],
      "how_to": "...",
      "image": "/images/products/cleansing-oil-1200.webp",
      "gallery": ["/images/products/cleansing-oil-1200.webp"],
      "stock": 24,
      "featured": true,
      "active": true
    }
  ]
}
```

### `GET /api/products/{slug}`

Returns `200 { "product": Product, "related": [Product] }` or `404`.

Products also carry the framework fields:

```json
{
  "plugin_id": "phone",
  "plugin_name": "Phone catalog",
  "plugin_summary": "Every handset is tracked by IMEI.",
  "option_schema": [
    { "key": "config", "label": "Memory and storage", "type": "select", "required": true },
    { "key": "color", "label": "Color", "type": "select", "required": true }
  ],
  "attributes": {
    "config": ["12 GB RAM + 256 GB", "16 GB RAM + 512 GB"],
    "color": ["Midnight", "Silver"]
  },
  "plugin_data": {
    "devices_in_stock": 6,
    "devices_reserved": 0,
    "variants": [
      { "config": "16 GB RAM + 512 GB", "color": "Silver", "in_stock": 2 }
    ]
  }
}
```

Core products return `plugin_id: "core"`, an empty `option_schema` and empty `attributes`.
Product images can be bundled `/images/...` paths or uploaded `/api/images/{id}` URLs.
Oils return `text`, `list` and `textarea` fields in `option_schema`; only `select` fields are
customer choices.

## Framework surfaces

### `GET /api/plugins`

Returns the installed plugin manifests with an effective `enabled` flag. The storefront uses it to hide pages and products contributed by disabled or blocked plugins; disabled plugin routes return `404`.

```json
{
  "plugins": [
    {
      "id": "phone",
      "name": "Phone catalog",
      "version": "1.0.0",
      "description": "Sell handsets while tracking every unit by IMEI.",
      "enabled": true,
      "dependencies": [],
      "product_type": { "label": "Phone", "summary": "...", "fields": [] },
      "categories": [{ "slug": "phones", "name": "Phones", "sort_order": 1 }],
      "storefront_pages": [{ "path": "/build-your-pc", "label": "Build your PC" }]
    }
  ]
}
```

`enabled` is the effective state: a plugin is false when it is disabled or a dependency is
disabled. The storefront filters its navigation on it, and the public catalog and checkout
use the same effective check. Disabled plugin routes return `404`.

Settings definitions and secret state are only returned to an admin.

### `GET /api/storefront/theme`

Returns `{ "theme": Theme }`. The theme is the admin-owned brand layer: copy, fonts,
palette, hero and story images. The server validates it before saving; see
`docs/PLUGINS.md`. When no theme has been saved, the shipped defaults are returned.

## Auth

### `POST /api/auth/register`

```json
{ "name": "Amina", "email": "amina@example.com", "password": "at least 10 characters" }
```

`201 { "user": { "id": "usr_...", "name": "...", "email": "...", "role": "customer" } }`.
Sets the session cookie. Duplicate email returns `409`.

### `POST /api/auth/login`

```json
{ "email": "amina@example.com", "password": "..." }
```

`200 { "user": User }`. Sets a new session cookie. Invalid credentials return `401` with a
generic message.

### `POST /api/auth/logout`

`204`. Deletes the current session and clears the cookie.

### `GET /api/auth/me`

`200 { "user": User }` or `401`.

### `GET /api/account/orders`

Requires a session. Returns `{ "orders": [OrderSummary] }` for the signed-in customer,
newest first. `OrderSummary` is the order shape without the token and without the address.

## Checkout

### `GET /api/checkout/challenge`

Returns a one-time captcha challenge for the order form when Cloudflare Turnstile is not
configured. Requires `X-Requested-With: azahar-web`; a missing or wrong header returns `403`.
The SPA sends this header through `apiFetch`.

```json
{
  "token": "b7J...",
  "image": "data:image/png;base64,...",
  "expires_in": 600
}
```

The image is a rendered PNG, not text in the response. A challenge expires after 10 minutes, is
valid until the first wrong answer or one successful verification, and is bound to the
requesting client address when one is available. A full store evicts the oldest challenge
instead of failing new requests. The endpoint is limited to 30 requests per 10 minutes per
address.

When `TURNSTILE_SECRET_KEY` is set on the API and `PUBLIC_TURNSTILE_SITE_KEY` on the web app,
checkout uses Cloudflare Turnstile instead. The challenge endpoint is then unused.

### `POST /api/checkout`

The client sends product IDs and quantities only. The server re-reads prices, checks stock,
computes the subtotal and total, and writes the order and items in one transaction.

```json
{
  "name": "Amina B.",
  "email": "amina@example.com",
  "phone": "0550 12 34 56",
  "wilaya_code": 16,
  "commune_name": "Bab Ezzouar",
  "delivery_type": "home",
  "stop_desk_id": "",
  "address": "12 rue des Oliviers",
  "notes": "Leave with the concierge",
  "items": [ { "product_id": "prd_01", "quantity": 2, "expected_price_cents": 480000, "options": { "config": "16 GB RAM + 512 GB", "color": "Silver" } } ],
  "website": "",
  "captcha_token": "b7J...",
  "captcha_answer": "K7M2P",
  "turnstile_token": "",
  "terms_accepted": true,
  "terms_version": "2026-11-14"
}
```

`201 { "order": Order }` where `Order` includes `token` this one time. The token is 32
random bytes, base64url, and only its SHA-256 hash is stored. The confirmation page is
`/order/{token}`. `GET /api/orders/{token}` does not echo the token.

The checkout also requires the CSRF cookie and header described in the request rules. `website` is a
honeypot and must stay empty. `terms_accepted` must be true and `terms_version` must match the
current terms page. A missing or wrong captcha returns `422` with a `captcha` field. A filled
honeypot returns `403 bot_detected`. The local captcha is consumed on a successful
verification. When Turnstile is configured, `turnstile_token` is required instead of
`captcha_token` and `captcha_answer`; a verification-service failure returns `503
captcha_unavailable`.

`delivery_type` is `home` or `stopdesk`. A stop desk sends the desk `id` as `stop_desk_id`.
The server checks the wilaya, commune and stop desk against the courier reference data and
stores the canonical French spelling. `address` is required for home delivery and optional
for a stop desk. `phone` must be an Algerian mobile: 05, 06 or 07, with spaces, `+`, `-` or
parentheses allowed.

Rules:

- 1 to 10 units per line, at most 20 lines.
- A product that is inactive or has insufficient stock returns `409` with a field message.
- The price is recomputed on the server from the active sale window. If
  `expected_price_cents` is present and differs, the line returns `409 price_changed` so a
  sale that ended while the bag was open cannot charge a different price silently.
- `shipping_cents` is 0 for orders over 800000 centimes, otherwise 60000.
- `status` starts at `pending`.
- `number` is `AZ-{year}-{sequence}`.
- The courier reference service being unavailable returns `503 courier_unavailable`.
- `options` is validated against the product's `option_schema` and allowed `attributes`.
  A core product rejects any options. A plugin may also reject the line when its own
  inventory cannot cover the choice.
- Order items carry `options` and `options_label`, for example
  `"Storage: 256 GB · RAM: 8 GB · Color: Silver"`.

### `GET /api/orders/{token}`

Public, token is the capability. `200 { "order": Order }` or `404`. The address is included
because the token is unguessable and the page is the customer's own confirmation.

The order shape also carries `wilaya_code`, `wilaya_name`, `commune_name`, `delivery_type`,
`stop_desk_id`, `stop_desk_name`, `courier_key`, `tracking_number`, `courier_status`,
`courier_events` and `courier_synced_at` when they exist.

## Shipping reference

These public endpoints wrap the dzship reference data and cache it for a day.

### `GET /api/shipping/wilayas`

`200 { "wilayas": [ { "code": 16, "nameFr": "Alger", "nameAr": "...", "isDeepSouth": false, "communeCount": 57, "courierSupported": true } ] }`

### `GET /api/shipping/communes?wilaya=16`

`200 { "communes": [ { "wilayaCode": 16, "nameFr": "Bab Ezzouar", "nameAr": "..." } ] }`

### `GET /api/shipping/desks?wilaya=16`

`200 { "desks": [ { "id": "SBX-16", "name": "Sandbox desk Alger", "wilayaCode": 16, "communeName": "Ain Benian", "address": "..." } ] }`

A courier that publishes no desk list returns an empty list. An unknown wilaya returns
`422`. An upstream failure returns `503 courier_unavailable`.

## Admin

The shared operational routes require an authenticated `admin` or `employee`. Routes marked
**Admin only** require `admin` and return `403` to an employee. See `docs/PERMISSIONS.md`.

### `GET /api/admin/metrics?days=30`

`days` is 7, 30 or 90. The response is computed from Turso orders.

```json
{
  "demo": true,
  "range": { "days": 30, "from": "2026-10-12", "to": "2026-11-11" },
  "revenue_cents": 1843200,
  "previous_revenue_cents": 1520000,
  "delta_pct": 21.3,
  "orders_count": 48,
  "previous_orders_count": 41,
  "aov_cents": 38400,
  "units_sold": 92,
  "new_customers": 12,
  "low_stock": [ { "id": "prd_01", "name": "...", "stock": 3, "image": "..." } ],
  "revenue_series": [ { "date": "2026-10-12", "revenue_cents": 0, "orders": 0 } ],
  "status_split": [ { "status": "pending", "count": 3 } ],
  "top_products": [ { "product_id": "prd_01", "name": "...", "units": 12, "revenue_cents": 576000 } ],
  "recent_orders": [ { "id": "ord_...", "number": "AZ-2026-0041", "name": "Sample customer 01", "total_cents": 38400, "status": "pending", "created_at": "..." } ]
}
```

Revenue counts orders whose status is not `cancelled`. The delta is only shown when the
previous period has revenue.

### `GET /api/admin/products`

Returns all products, active and archived, with the same fields as `Product` plus
`created_at` and `updated_at`. Available to admins and employees.

### `POST /api/admin/products`

Admin only. `employee` returns `403`.

```json
{
  "name": "New oil",
  "slug": "new-oil",
  "subtitle": "...",
  "description": "...",
  "price_cents": 480000,
  "compare_at_cents": null,
  "sale_price_cents": 384000,
  "sale_starts_at": "2026-11-20T09:00:00Z",
  "sale_ends_at": "2026-11-27T21:00:00Z",
  "size": "100 ml",
  "category_slug": "face",
  "concerns": ["dry"],
  "ingredients": ["..."],
  "how_to": "...",
  "image": "/images/products/cleansing-oil-1200.webp",
  "gallery": [],
  "stock": 20,
  "featured": false,
  "active": true,
  "plugin_id": "core",
  "attributes": {}
}
```

`201 { "product": Product }`. Slug must be unique. `category_slug` must be a category owned
by the same plugin as `plugin_id`; a category from another plugin returns `422`. For a
plugin product, send the plugin id and one list of values per option field. Oils use `text`,
`list` and `textarea` fields:

```json
{
  "plugin_id": "oils",
  "attributes": {
    "size": ["100 ml"],
    "concerns": ["dry", "sensitive"],
    "ingredients": ["Olive squalane", "Orange blossom"],
    "how_to": ["Warm two pumps and massage over dry skin."]
  }
}
```

Text and textarea fields take one value; lists take up to 12. Phones use `select` fields for
paired memory/storage configurations and colour. A core product must send empty attributes. Unknown fields and values
outside the plugin schema return `422` with keys such as `attributes.size`.

### `PATCH /api/admin/products/{id}`

Admin only. Partial update. Only the fields present are changed. `200 { "product": Product }`.

Send `sale_price_cents` above zero with both `sale_starts_at` and `sale_ends_at` to start a
limited-time sale. The window must end after it starts and the sale price must be below the
regular price. Send `sale_price_cents: 0` to clear the sale. Timestamps are RFC 3339 UTC.

The public `Product` exposes `effective_price_cents` and `on_sale`; checkout uses the same
server-side calculation for the order item price.

### `DELETE /api/admin/products/{id}`

Admin only. Archives the product: sets `active = 0`. It is not deleted, so old orders keep
their items. `204`.

### `POST /api/admin/product-images`

Admin only. Multipart upload. Field name `image`. Maximum 5 MB. JPEG, PNG and WebP are
detected from the bytes. Returns:

```json
{
  "image": {
    "id": "img_...",
    "url": "/api/images/img_...",
    "mime": "image/png",
    "bytes": 148222
  }
}
```

The returned `url` can be used as a product `image` or `gallery` path. `422` for an
unsupported image, `413` for a file over 5 MB.

### `GET /api/images/{id}`

Public image bytes for an uploaded product image. Returns the stored MIME type with a long
immutable cache header. Unknown or malformed ids return `404`.

### `GET /api/admin/orders?status=pending&q=AZ-2026`

Returns `{ "orders": [AdminOrder] }`. `AdminOrder` includes the customer, address, items,
delivery details, courier tracking and status. Search matches order number, name, email,
phone or tracking number. Each item carries `plugin_id`, `options` and `options_label`.
The admin order dialog renders a plugin order panel for each plugin id, so the phone plugin
can show the reserved IMEIs beside the items.

### `PATCH /api/admin/orders/{id}/status`

```json
{ "status": "packed" }
```

Allowed transitions:

- `pending` to `confirmed` or `cancelled`
- `confirmed` to `packed` or `cancelled`
- `packed` to `shipped` or `cancelled`
- `shipped` to `delivered`
- `delivered` and `cancelled` are terminal

Invalid transition: `409`. `200 { "order": AdminOrder }`.

### `POST /api/admin/orders/{id}/shipment`

Creates the parcel on the configured courier account. Requires a stored wilaya, commune and
delivery type. Home delivery requires an address; a stop desk requires its id. An order that
already has a shipment returns `409`; a cancelled or delivered order also returns `409`.

`200 { "order": AdminOrder }` with `courier_key`, `tracking_number`, `courier_status` and
`courier_synced_at`. The order status is not advanced; mark the order shipped when the
courier collects it.

Errors: `422` for missing delivery details or a courier field error, `429` with `Retry-After`
when the courier rate limit is hit, `502` for a courier or network failure.

### `POST /api/admin/orders/{id}/tracking`

Refreshes the courier status and event history. Requires an existing tracking number.
`200 { "order": AdminOrder }`. The same courier error statuses apply.

### `GET /api/admin/customers`

```json
{
  "customers": [
    {
      "id": "usr_...",
      "name": "...",
      "email": "...",
      "orders_count": 3,
      "total_spent_cents": 120000,
      "last_order_at": "...",
      "created_at": "..."
    }
  ]
}
```

### `GET /api/admin/team`

Admin only. Returns every account with staff first:

```json
{ "users": [ { "id": "usr_...", "name": "...", "email": "...", "role": "employee", "created_at": "..." } ] }
```

### `PATCH /api/admin/team/{id}`

Admin only. `{ "role": "employee" }` where the role is `admin`, `employee` or `customer`.
Returns `{ "user": TeamMember }`. An administrator cannot remove their own admin role, and
the last administrator cannot be demoted (`409 last_admin`). Role changes take effect on the
next request because the session user is reloaded each time.

### `GET /api/admin/preferences`

`200 { "theme_preset": "cellar" }`. The value is the colour style. Unknown or missing values
return the default `cellar`. The light or dark mode is a local device preference and is not
stored in this API.

### `PUT /api/admin/preferences`

```json
{ "theme_preset": "lagoon" }
```

Allowed values: `plaster`, `cellar`, `terracotta`, `lagoon`. `200` echoes the stored value.
Each style has its own light and dark mode; the mode is not part of this payload.

### `GET /api/admin/storefront/theme`

`200 { "theme": Theme, "defaults": Theme }`. The defaults are the shipped theme, so the
editor can reset without a second request.

### `PUT /api/admin/storefront/theme`

Accepts a full `Theme` object. The server normalises, validates and audits it. Unknown
fields, non-hex colours, unknown fonts, unsafe image paths and contrast below the documented
minimum return `422` with field keys such as `light.ink` or `brand_name`.

### `GET /api/admin/plugins`

Returns every installed plugin with its admin manifest, `configured` and `settings_state`
(`ready` or `unreadable`) plus `updated_at`. Plugin state fields:

- `enabled`: the admin's desired state.
- `effective_enabled`: true only when the plugin and every dependency are enabled.
- `dependencies`: ids declared by the manifest.
- `dependents`: plugins that depend on this one, directly or transitively.
- `blocked_by`: disabled dependencies that make this plugin unavailable.

### `PUT /api/admin/plugins/{id}/state`

```json
{ "enabled": false, "confirm_dependents": true }
```

Enables or disables a plugin and writes an audit entry. Returns `{ "plugin": AdminPlugin }`.

- Enabling a plugin with a disabled dependency returns `409 plugin_dependency` and a
  `dependencies` field.
- Disabling a plugin with enabled dependents returns `409 plugin_dependency_warning` and a
  `dependents` field unless `confirm_dependents` is true.
- Disabled and blocked plugin routes return `404`; their products, categories and storefront
  pages disappear from the public surfaces. Data, settings and order hooks are retained.

### `GET /api/admin/plugins/{id}/settings`

```json
{
  "plugin_id": "ai",
  "settings": { "api_key": "", "model": "gpt-4o-mini", "language": "en", "enabled": "true" },
  "secrets_set": ["api_key"],
  "configured": true,
  "updated_at": "2026-11-20T10:00:00Z"
}
```

Secret values are never returned. `secrets_set` says which secrets already hold a value.
If the values cannot be decrypted with the current `PLUGIN_SECRET_KEY`, the endpoint returns
`409 plugin_settings_unreadable`.

### `PUT /api/admin/plugins/{id}/settings`

```json
{ "settings": { "api_key": "sk-...", "model": "gpt-4o-mini", "language": "en", "enabled": "true" } }
```

A blank secret keeps the stored value. Unknown keys, missing required values and
plugin-specific rules return `422`. The values are encrypted before they are written.

### Phone plugin

`GET /api/plugins/phone/admin/devices?product_id=prd_...` returns devices and counts:

```json
{
  "devices": [
    {
      "id": "phn_...",
      "product_id": "prd_...",
      "product_name": "Aster 5",
      "variant_key": "config=16+GB+RAM+%2B+512+GB&color=Silver",
      "variant_label": "Memory and storage: 16 GB RAM + 512 GB · Color: Silver",
      "config": "16 GB RAM + 512 GB",
      "color": "Silver",
      "imei": "356938035640003",
      "status": "in_stock",
      "order_id": "",
      "order_number": "",
      "created_at": "..."
    }
  ],
  "counts": { "in_stock": 6, "reserved": 0, "sold": 0 }
}
```

- `POST /api/plugins/phone/admin/devices` with
  `{ "product_id": "...", "config": "16 GB RAM + 512 GB", "color": "Silver", "imei": "356938035640003" }`.
  The IMEI must be 15 digits and pass Luhn. Duplicate IMEI returns `409`.
- `DELETE /api/plugins/phone/admin/devices/{id}` removes an in-stock unit, `204`.
- `GET /api/plugins/phone/admin/orders/{orderID}/devices` lists the units for an order.

Checkout reserves one in-stock IMEI per unit in the order transaction. Cancelling the order
releases them; delivering it marks them sold.

### PC builder plugin

`GET /api/plugins/pcbuilder/parts` returns active PC parts grouped by their plugin categories:

```json
{
  "categories": [
    {
      "slug": "cpus",
      "name": "CPU",
      "products": [
        {
          "id": "prd_...",
          "slug": "ryzen-7",
          "name": "Ryzen 7",
          "price_cents": 549000,
          "currency": "DZD",
          "image": "/images/...",
          "stock": 5,
          "plugin_id": "pcparts",
          "option_schema": [],
          "attributes": { "socket": ["AM5"], "tdp_watts": ["120"] },
          "category": { "slug": "cpus", "name": "CPU", "plugin_id": "pcparts" }
        }
      ]
    }
  ]
}
```

The storefront page uses the attributes for its compatibility checks. The endpoint is public
and read-only.

### Code mode plugin

- `POST /api/plugins/codemode/admin/preview` with `{ "instruction": "Change the hero heading..." }`
  calls the configured provider and stores a validated plan. Returns
  `{ "plan": { "id", "summary", "actions", "status", "created_at" } }`.
- `POST /api/plugins/codemode/admin/apply` with `{ "plan_id": "cmp_..." }` applies a preview
  plan once. Returns `{ "plan": Plan, "results": [{ "type", "ok", "message" }] }`.
- `GET /api/plugins/codemode/admin/plans?limit=10` returns the current admin's recent plans.
  Plans and their apply/read calls are bound to the admin who previewed them.
- Plans may contain `theme.update`, `product.update`, `product.archive`, `product.create` and
  `order.status`. `order.status` accepts the same transitions as the manual admin action;
  cancelling restores stock and runs the plugin order hooks.

Errors: `409 plugin_not_configured`, `409 plugin_disabled`, `409 plan_already_applied`,
`409 plan_stale`, `422 invalid_plan`, `502 ai_upstream_error`. The API key is stored encrypted
and never returned; plans are admin-only and audited.

### AI plugin

- `POST /api/plugins/ai/admin/analyze` with `{ "kind": "store", "days": 30 }` or
  `{ "kind": "product", "product_id": "prd_..." }`. Returns `{ "run": Run }`.
- `GET /api/plugins/ai/admin/runs?limit=10` returns recent runs.

Errors: `409 plugin_not_configured`, `409 plugin_disabled`, `502 ai_upstream_error`.
The provider API key never leaves the server and customer data is never sent to the model.

## Seed data

`SEED_DEMO=true` seeds the catalog, two users and 48 sample orders across the last 45 days.
Admin: `ADMIN_EMAIL` and `ADMIN_PASSWORD`. Customer: `CUSTOMER_EMAIL` and
`CUSTOMER_PASSWORD`. The seeder is idempotent. It never runs when the tables already hold
data.

## Security headers

The Go service sets `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`,
`Permissions-Policy`, `Cache-Control: no-store` on API responses, and HSTS when
`APP_ENV=production`. The SvelteKit server sets the document CSP and the same baseline
headers. See `docs/SECURITY.md`.

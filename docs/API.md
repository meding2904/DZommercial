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

```json
{ "categories": [ { "slug": "face", "name": "Face", "description": "..." } ] }
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
      "currency": "DZD",
      "size": "100 ml",
      "category": { "slug": "face", "name": "Face" },
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
  "items": [ { "product_id": "prd_01", "quantity": 2 } ],
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
- `shipping_cents` is 0 for orders over 800000 centimes, otherwise 60000.
- `status` starts at `pending`.
- `number` is `AZ-{year}-{sequence}`.
- The courier reference service being unavailable returns `503 courier_unavailable`.

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

Every route below requires an authenticated user with `role = "admin"`.

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
`created_at` and `updated_at`.

### `POST /api/admin/products`

```json
{
  "name": "New oil",
  "slug": "new-oil",
  "subtitle": "...",
  "description": "...",
  "price_cents": 480000,
  "compare_at_cents": null,
  "size": "100 ml",
  "category_slug": "face",
  "concerns": ["dry"],
  "ingredients": ["..."],
  "how_to": "...",
  "image": "/images/products/cleansing-oil-1200.webp",
  "gallery": [],
  "stock": 20,
  "featured": false,
  "active": true
}
```

`201 { "product": Product }`. Slug must be unique.

### `PATCH /api/admin/products/{id}`

Partial update. Only the fields present are changed. `200 { "product": Product }`.

### `DELETE /api/admin/products/{id}`

Archives the product: sets `active = 0`. It is not deleted, so old orders keep their items.
`204`.

### `GET /api/admin/orders?status=pending&q=AZ-2026`

Returns `{ "orders": [AdminOrder] }`. `AdminOrder` includes the customer, address, items,
delivery details, courier tracking and status. Search matches order number, name, email,
phone or tracking number.

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

### `GET /api/admin/preferences`

`200 { "theme_preset": "cellar" }`. Unknown or missing values return the default `cellar`.

### `PUT /api/admin/preferences`

```json
{ "theme_preset": "lagoon" }
```

Allowed values: `plaster`, `cellar`, `terracotta`, `lagoon`. `200` echoes the stored value.

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

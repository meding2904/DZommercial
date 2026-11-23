# Glossary

The shared domain language of the Azahar framework. Use these names in code,
docs and conversation; when a module is named after a concept, the concept
lives here so nobody has to rediscover it.

## The shop

- **Order**: one cash-on-delivery sale. Born from checkout or a counter sale,
  moves through the order status machine, and may carry a courier parcel.
- **Bag**: what the customer fills while shopping. Called `bag` in all
  user-facing copy and `cart` in code (the cart store, `cart.svelte.ts`). Both
  names refer to the same thing; new code should say bag in copy and cart only
  when naming the store.
- **Counter sale**: an order taken in person at the till by the owner or an
  employee, recorded as a delivered `pickup` order in one transaction, stock
  and receipt included.
- **Courier parcel**: the shipment handed to the courier. Creating one does not
  move the order to `shipped`; the order status and the courier status are
  separate stories.
- **Delivery map**: the wilaya, commune and stop-desk lists checkout chooses
  from. Served by the courier module, with a bundled snapshot as the offline
  fallback. Its storefront module is `frontend/src/lib/checkout/delivery-map.ts`.
- **Ledger**: the admin's takings view for a period: revenue, order counts,
  average order, top products, low stock.
- **Storefront**: the customer-facing shop and its theme (brand name, copy,
  images, fonts, palettes), validated by contrast rules and editable by an
  admin only.
- **Gate**: the private `/ops-<token>` link that un-hides the admin. Fail-closed
  in production.
- **Audit outbox**: the table audit rows land in inside their business
  transaction, flushed in batches by the background flusher.

## Module names

Names given to deepened modules during the architecture review of
23 November 2026. The API contract in `docs/API.md` stays the enforcement
authority; these modules mirror it once.

- **Order status machine** (`backend/internal/orderstatus`): the statuses an
  order can hold and the moves allowed between them. The admin API and the
  codemode planner both ask this table instead of carrying copies.
- **Product write rules** (`backend/internal/productrules`): the name, price,
  stock, image-path and attribute rules the admin API and the codemode planner
  must agree on. Each adapter keeps its own message wording.
- **Product options** (`frontend/src/lib/product-options.ts`): resolves a
  product's chosen options to a variant, its availability and its effective
  stock. The only core module that reads a plugin's `plugin_data` keys.
- **Order intake** (`backend/internal/orderintake`): the one path that turns
  basket lines into a stored order and the one path that moves an order's
  status. The web checkout, the counter sale and the admin status change are
  all thin adapters over it; every write runs inside one transaction with
  plugin hooks and the audit row in the same commit.
- **Resource module** (`frontend/src/lib/admin/resource.ts`, with its rune
  binding in `resource.svelte.ts`): the one request-at-a-time loader every
  admin screen uses: aborting the previous request, last-write-wins,
  loading and error state, retry and cleanup on unmount. The only place an
  AbortController lives in the admin.
- **Human check** (`frontend/src/lib/checkout/human-check.ts`): the seam both
  proof-of-person implementations sit behind: Cloudflare Turnstile when it is
  configured, the built-in image challenge otherwise. Checkout asks it for its
  state, its body fields and its recovery rule; the fork lives in the module,
  never in the page.
- **Checkout validation** (`frontend/src/lib/checkout/order-validation.ts`):
  the checkout matrix and the submit body as pure functions. The field order
  mirrors the server so the first message a person sees is the first field
  they read; docs/API.md stays the authority.
- **Courier seam** (`backend/internal/courier`): the delivery map and the
  parcel verbs behind one interface. The dzship HTTP client and the bundled
  snapshots are the two adapters; one resolver owns every fallback decision.
  ADR 0001 records the warn-only sandbox stance.
- **Admin domain** (`frontend/src/lib/admin/domain/`, copied byte-identically
  into `desktop/frontend/domain/` and enforced by `make desktop-check`): the
  receipt document, the status transitions and the courier and delivery labels
  shared by the web admin and the desktop renderer. The API contract stays the
  enforcement authority.
- **WhatsApp outbox** (`backend/internal/plugins/whatsapp`): the message rows
  written inside the order transaction and sent by the plugin dispatcher.
  Delivery receipts, inbound replies and consent live beside the queue; the
  webhook is the only public plugin route that bypasses the browser CSRF header,
  because it verifies Meta's signature instead.

# Azahar desktop admin

An Electron wrapper for the shop owner's admin panel. It calls the Go API directly from the
main process, with no SvelteKit server in front.

- Ledger with takings, work queue, ranked products and order mix.
- Counter sale for walk-in cash sales, including phone options and a printable receipt.
- Orders with status transitions, courier parcel creation and tracking refresh.
- Products and stock, with the stock stepper writing through the API.
- Customers and staff roles.

This is the operational admin panel: the counter, orders, stock, customers and staff. The
storefront editor, plugin settings and the AI screens stay in the web admin, where the gate
and those wider surfaces already live.

## Run

From this folder:

```bash
pnpm install
pnpm start
```

The app defaults to `http://127.0.0.1:8080` and reads the expected request origin from the
API health check. For the standard demo, start the stack with `./deploy/kiri-up.sh` from the
repository root first; the development start publishes the API on loopback.

The Connection screen on the sign-in page accepts another API address. In production, point it
at the TLS address of the API. Do not publish the API port directly.

## Build

```bash
pnpm dist
```

The Linux AppImage is written to `dist/`. `pnpm prepare-assets` copies the bundled fonts and
the demo product images from `../frontend` first.

## What talks to what

- `main.js` makes the HTTP requests, keeps the session cookie, and exposes a narrow bridge.
- `preload.js` exposes `window.azahar` with `request`, `image`, `health` and settings calls.
- `renderer/` is sandboxed with `contextIsolation`, no Node integration, and
  `connect-src 'none'`. It cannot reach the network except through the bridge.
- The API session cookie is stored in the app profile with Electron `safeStorage` when the
  platform provides it.

The web admin gate is not used here. The API still requires a staff session and returns `403`
for an employee on catalogue writes.

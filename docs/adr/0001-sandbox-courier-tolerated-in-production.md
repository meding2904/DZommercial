# ADR 0001: Tolerate the sandbox courier in production

- Status: accepted
- Date: 2026-11-23
- Module: `backend/internal/courier`

## Context

Checkout cannot render without the delivery map: the wilayas, their communes and
the stop desks a customer chooses from. Every one of those reads comes from the
courier gateway (dzship), a third party with a shared rate limit, its own
timeouts and no uptime promise to us. Without a cached answer or a bundled list,
a courier outage fails those reads as a 503, which turns a vendor problem into a
shop that cannot take an order at all.

The same module creates the courier parcel and refreshes its tracking. Those are
writes against a real parcel and cannot be invented: a fake tracking number in
production is a parcel that does not exist.

The courier key is `sandbox` by default, and production accepts it too. Without
real credentials the shop still has to be demonstrable end to end, including
"create a shipment" and "refresh tracking", so the module can answer with local
stand-ins. The bundled snapshots in `backend/internal/courier/data/` already hold
the whole delivery map.

## Decision

1. **A read-only delivery map beats a 503.** When dzship cannot answer, the
   wilaya and commune lists degrade to the bundled snapshot for every courier
   key, real or sandbox. A stale cached answer is preferred over the snapshot,
   and the snapshot over the error. Stop desks follow the courier's own list:
   stale cache first, and a sandbox desk only for the sandbox key.

2. **The sandbox is tolerated in production, with a warning.** Startup logs that
   the sandbox courier is active in production and that no real parcels will be
   created. It is not a configuration error, because a production demo is a
   supported deployment.

3. **The bundled snapshots are the degradation path.** They ship with the binary
   (`go:embed`), cost no network call, and cover the wilaya and commune lists.
   Stop desks are not snapshot: they belong to a courier account and change with
   it.

4. **One resolver owns the fallback.** Cache freshness, the offline window, the
   stale answer, the snapshot and the fallback warning live in one place. The
   dzship HTTP client and the snapshot adapter only fetch; they do not decide.

5. **A real courier key never falls back to the local sandbox.** Synthetic stop
   desks, parcels and tracking exist only when the key is `sandbox` and the app
   allows it. A non-sandbox key gets the real courier's answer or an error; a 4xx
   rejection on a parcel or tracking call is never masked by a fake success.
   The delivery-map reads keep their older, deliberate exception: wilayas and
   communes fall back to the bundled snapshot on any courier error, for every
   key, because a read-only map beats a 503.

## Consequences

- Checkout keeps working through a dzship outage for wilayas and communes.
  Stop-desk reads, parcel creation and tracking refresh still surface the
  courier failure, apart from a stale desk cache.
- A stale delivery map can lag a courier's real list until the next successful
  refresh. The map is small and changes rarely, so this is acceptable.
- A fallback emits a warning with the resource name, so an operator can tell a
  degraded delivery map from a healthy one.
- The sandbox path must stay behind an explicit key check. Adding a new fallback
  resource means deciding, in the resolver, whether a real key may use it.
- Delivery-map fallbacks are cached for five minutes, so a courier recovery is
  not noticed immediately.

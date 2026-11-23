# Wails bridge contract

The renderer talks to Go only through these bound methods on `main.App`. Wails exposes them
as `window.go.main.App.<Method>`. The renderer never receives a session cookie, a CSRF token,
or a raw network handle.

All methods return JSON-serialisable structs. `Request` and `Image` wrap failures in the same
shape as the old Electron bridge.

The admin domain the renderer shares with the web admin (order statuses and transitions, human
labels, receipt HTML) lives in `frontend/src/lib/admin/domain/`, which is the single source of
truth; `desktop/frontend/domain/` is a byte-identical copy kept by `make desktop-sync`.

## Methods

### `Info()`

```json
{
  "version": "2.0.0",
  "platform": "linux",
  "apiUrl": "http://127.0.0.1:8080",
  "origin": "",
  "resolvedOrigin": "http://localhost:3000",
  "networkAllowed": true
}
```

`networkAllowed` is consent for a non-loopback server. Loopback needs no explicit consent, so
the field is already `true` for a loopback `apiUrl`. For a non-loopback server it is true only
for the host saved in the settings; changing the host requires a fresh consent.

### `Health()`

```json
{
  "ok": true,
  "status": 200,
  "env": "development",
  "origin": "http://localhost:3000",
  "serverTime": "2026-11-21T10:46:28Z",
  "message": ""
}
```

A failed health check returns `ok: false` with `status: 0` and a human message. It never
throws for a network failure.

### `Request(method, path, body)`

Allowed methods are `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`. `path` must start with
`/api/`, must stay on the configured API origin, and may carry a query string. The renderer
always sends an object as `body` (an empty object for requests without a body); `GET` and
`HEAD` ignore it. Bodies are JSON and limited to 2 MiB. Redirects are never followed.

Success:

```json
{ "ok": true, "data": { "...": "..." } }
```

Failure:

```json
{
  "ok": false,
  "error": {
    "status": 403,
    "code": "csrf_failed",
    "message": "The request could not be verified.",
    "fields": {}
  }
}
```

### `Image(path)`

Only `/api/images/` paths. Returns `{ "ok": true, "data": "data:image/webp;base64,..." }`.
The allowlist is PNG, JPEG, WebP, GIF and AVIF, capped at 8 MiB.

### `UpdateSettings(apiUrl, origin, allowNetwork)`

Validates the server address, saves it, clears the old session and resolves the API origin.
`apiUrl` must be an absolute `http` or `https` root with no credentials, path, query or
fragment. Remote plain HTTP is refused. A non-loopback server requires `allowNetwork: true`,
and that consent is tied to the host: changing the host needs a new consent.

Returns the same fields as `Info()`, plus an optional `warning`. The call rejects with an
error string the form can show.

```json
{
  "apiUrl": "https://admin.example.com",
  "origin": "",
  "resolvedOrigin": "http://localhost:3000",
  "networkAllowed": true,
  "warning": "The old server's saved session could not be removed. It is scoped to the old server."
}
```

`warning` is omitted when empty. It is set only when the address changed and the session saved
for the previous server could not be deleted; the new settings are already live, so the form
should show the message and continue rather than treat it as a failed save.

### `ClearSession()`

Deletes the current server's saved session from the keyring and the fallback file. Rejects
with an error string when the local store cannot be removed.

### `FirewallGuide()`

```json
{
  "platform": "linux",
  "title": "Allow Azahar through your firewall",
  "intro": "This app talks to your API from Go, not from the page.",
  "steps": ["..."],
  "note": "Azahar never changes firewall rules for you."
}
```

### `OpenExternal(url)`

Only `http` and `https` URLs without credentials. Opens the system browser through Wails.

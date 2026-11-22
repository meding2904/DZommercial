# Azahar desktop admin

The owner's counter app, migrated from Electron to [Wails v2](https://wails.io) with a Go
backend. The renderer is the same dependency-free admin UI. Go owns the HTTP client, the
session, the server address and every validation boundary.

It is the operational panel: the ledger, a counter sale, orders, products and stock,
customers and staff. The storefront editor, plugin settings and the AI screens stay in the
web admin.

## Requirements

- Go 1.26 or newer
- Wails v2.17.1 (`make desktop-deps`)
- GTK 3 and WebKit2GTK 4.1 development packages on Linux
- `pkg-config` and a C compiler

On Ubuntu 24.04:

```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev pkg-config gcc
```

The Makefile detects WebKitGTK 4.1 with `pkg-config` and adds the `webkit2_41` build tag.
Override it with `WAILS_TAGS=` if the host has only WebKitGTK 4.0. A binary built for 4.1
will not run on a 4.0-only distribution.

Run `wails doctor` if the Linux webview dependencies are missing.

## Run

From the repository root, start the API and web app first:

```bash
./deploy/kiri-up.sh
```

Then start the desktop app:

```bash
make desktop-deps   # first time only
make desktop
```

The API is published on `http://127.0.0.1:8080` in development. The app opens the connection
screen on the sign-in page when it cannot reach that address.

## Build and package

```bash
make desktop-build
make desktop-package
```

`desktop-build` writes the native binary to `desktop/build/bin/azahar-admin`. Wails does not
produce an AppImage; the binary and the system GTK/WebKit runtime are the package.
`desktop-package` writes `desktop/build/azahar-admin-linux-amd64.tar.gz` with the binary, the
desktop entry, the icon and the licence files. Install it into a prefix such as `/usr` or
`~/.local`.

## Connect to your own server

Open **Server connection** on the sign-in screen, or the Connection page inside the app:

1. Enter the server root, for example `https://admin.example.com` or
   `http://192.168.1.20:8080`.
2. Leave **Request origin** empty to read it from `GET /api/health`, or enter the exact
   `APP_ORIGIN` the API was started with.
3. For any address that is not loopback, tick **Allow this app to contact the server on the
   network**.
4. Save and retry. Use **Firewall help** if the operating system is blocking the connection.

Plain HTTP is accepted only for loopback and private or local hosts. A public host must use
HTTPS, because the session cookie and the admin password cross this connection. The app never
turns off TLS verification.

The app makes outbound requests from the Go process, not from the webview. Windows and macOS
may ask whether the app may use the network. On Linux, allow outbound traffic for the
`azahar-admin` binary in `ufw`, `firewalld` or the active firewall. If the API runs on another
machine, its incoming port is usually the one that needs a firewall rule. The app does not
change firewall rules and never asks for administrator rights.

## Security notes

- The webview has no network access. Its CSP sets `connect-src 'none'`; every API call is a
  typed Wails call into Go.
- The Wails asset server adds a strict CSP header, `nosniff`, a no-referrer policy and
  `frame-ancestors 'none'`.
- Only `/api/` paths on the configured server are reachable. Methods are allowlisted, request
  bodies are capped at 2 MiB, response bodies at 16 MiB, and redirects are not followed.
- The Go client sends the API session cookie and CSRF token. Neither is returned to the
  renderer.
- The session is stored in the OS keyring with a per-profile, per-server account. If the
  keyring is unavailable or does not answer within a short timeout, the app falls back to a
  `0600` file. Changing the server clears the old session.
- Non-loopback servers require explicit network consent. The checkbox is cleared when the
  address changes, and a new host needs fresh consent. Public plain-HTTP servers are refused.
- External links are opened in the system browser only after a scheme and credential check.
  The webview blocks non-http(s) navigation and disables drag-and-drop.
- The API still enforces its own session, CSRF, role and rate-limit rules on every request.
- Wails uses DBus for the single-instance lock on Linux. If the desktop session has no bus,
  the app still runs, but two instances can open.

## Environment

The app profile is `~/.config/azahar-admin` on Linux, `~/Library/Application Support/azahar-admin`
on macOS and `%AppData%\azahar-admin` on Windows. It holds `settings.json`; the session is in
the OS keyring or the `session-*.json` fallback. On first run, the app imports
`apiUrl`, `origin` and the window bounds from the old Electron `Azahar Admin/settings.json`
when the new profile has no settings. The encrypted Electron session is not imported, so sign
in once after migrating.

These variables are for tests and support:

| Variable | What it does |
|---|---|
| `AZAHAR_PROFILE_DIR` | Use a different profile directory |
| `AZAHAR_API_URL` | Override the saved server address |
| `AZAHAR_ORIGIN` | Override the saved request origin |
| `AZAHAR_SESSION_BACKEND=file` | Skip the keyring and use the `0600` fallback |
| `AZAHAR_DISABLE_KEYRING=1` | Same as the file backend |

## Layout

```text
frontend/          Static admin UI served by Wails
internal/api/      Go HTTP client, settings, session storage, validation and tests
main.go            Wails window, bindings and lifecycle
build/             App icon, desktop entry and Wails build output
```

The old Electron files are gone. The web SvelteKit app and the Go API remain separate.

The source is licensed under the Apache License 2.0. See `../LICENSE` and `../NOTICE`.

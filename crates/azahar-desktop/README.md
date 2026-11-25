# azahar-desktop

The Tauri 2 host that replaces `desktop/main.go`. The renderer in
`desktop/frontend` is unchanged; this crate only replaces the Wails shell and
calls the already-ported `azahar-desktop-api` client.

## Versions and pins

| Item | Version | Why |
| --- | --- | --- |
| Tauri | `=2.9.5` | Wails replacement, 2.9.x stable |
| `tauri-build` | `=2.7.6` | Tauri 2.9.5 requires `^2.5.3`; 2.x build tooling |
| `tauri-macros` | `=2.5.2` (lockfile) | newer 2.7 macros emit a variant 2.9.5 does not have |
| `tauri-runtime` | `=2.9.2` (lockfile) | tied to `tauri-runtime-wry` 2.9.3 |
| `wry` | `=0.53.4` (lockfile) | 0.53.5 changed a callback bound |
| `tauri-plugin-opener` | `=2.5.0` | `OpenExternal` |
| `tauri-plugin-single-instance` | `=2.3.7` | Wails single-instance lock; 2.5.x needs Tauri 2.12 |

The lockfile pins are part of the deliverable: `cargo update` across Tauri 2.x
will otherwise mix a 2.9 runtime with newer macros and fail to compile.

## Run and check

```bash
cargo check -p azahar-desktop
cargo test -p azahar-desktop
cargo clippy -p azahar-desktop --all-targets -- -D warnings
cargo fmt --check -p azahar-desktop
```

`cargo run -p azahar-desktop` needs a desktop session and WebKitGTK. No Tauri
CLI or bundler is required for `check`/`test`.

## Layout

```text
Cargo.toml              package, Tauri and plugin versions
build.rs                tauri_build::build()
tauri.conf.json         window defaults, CSP, declarative headers, bundle icon
src/app.rs              profile directory, App state, 19-function port
src/commands.rs         Tauri commands and the spawn_blocking boundary
src/security.rs         CSP constant, security headers, response hook
src/window.rs           physical/logical conversion, monitor helpers
src/bridge.js           window.go.main.App compatibility shim
src/lib.rs              builder, setup, window event wiring, run/main
src/main.rs             azahar-admin binary entry point
```

There is no capability file. Tauri 2 only checks the ACL for plugin commands
unless the app registers an ACL manifest; the eight app commands are allowed by
the registered invoke handler. Keeping the capability surface empty is the
smallest privilege set.

## Function mapping

| Go (`desktop/main.go`) | Rust |
| --- | --- |
| `profileDirectory` | `app::profile_directory` / `profile_directory_from` |
| `NewApp` | `App::new` (creates `Client::new(version, profile_dir)`) |
| `context` | `App::context` -> `OnceLock<AppHandle>` |
| `windowBounds` | `App::window_bounds` -> `client.window_bounds()` |
| `main` | `lib::run` + `lib::main` |
| `securityHeaders` | `security::security_headers` (`on_web_resource_request`) + `app.security.headers` |
| `startup` | `App::startup` called from the Tauri `setup` hook |
| `clampWindowSize` | `App::clamp_window_size` -> `window::clamp_window_size` |
| `screenPositionAllowed` | `App::screen_position_allowed` -> `window::screen_position_allowed` |
| `domReady` | `App::dom_ready` from `on_page_load(PageLoadEvent::Finished)` |
| `beforeClose` | `App::before_close` from `WindowEvent::CloseRequested` |
| `Info` | `App::info` -> command `info` |
| `Health` | `Client::health` -> command `health` (`spawn_blocking`) |
| `Request` | `Client::request` -> command `request` (`spawn_blocking`) |
| `Image` | `Client::image` -> command `image` (`spawn_blocking`) |
| `UpdateSettings` | `Client::update_settings` -> command `update_settings` (`spawn_blocking`) |
| `ClearSession` | `Client::clear_session` -> command `clear_session` (`spawn_blocking`) |
| `FirewallGuide` | `App::firewall_guide` -> command `firewall_guide` |
| `OpenExternal` | `App::open_external` -> command `open_external` |

The `App` methods of the same names delegate to the client for callers that
hold the host state; the command wrappers above clone the `Arc<Client>` and run
the network work on a blocking worker so no client mutex is held across an
await.

`context.Context` has no direct Tauri equivalent. A `context.Context` was the
handle every Wails runtime call needed; in Tauri the equivalent is `AppHandle`,
stored once in `startup`. Lifecycle hooks receive the live `WebviewWindow` or
`Window` directly, so they do not look the handle up. Commands receive the
managed `Arc<App>` through `tauri::State`.

## Wails bridge shim

`src/bridge.js` is installed with
`WebviewWindowBuilder::initialization_script`. Tauri config has
`"withGlobalTauri": true`, so it uses `window.__TAURI__.core.invoke`:

```js
window.go = {
  main: {
    App: {
      Info: () => call('info', {}),
      Health: () => call('health', {}),
      Request: (method, path, body) =>
        call('request', { method, path, body: body == null ? {} : body }),
      Image: (path) => call('image', { path }),
      UpdateSettings: (apiUrl, origin, allowNetwork) =>
        call('update_settings', { apiUrl, origin, allowNetwork: Boolean(allowNetwork) }),
      ClearSession: () => call('clear_session', {}),
      FirewallGuide: () => call('firewall_guide', {}),
      OpenExternal: (url) => call('open_external', { url })
    }
  }
};
```

The positional Wails arguments become one camelCase object, which is exactly
what Tauri's command macro expects for `api_url`, `allow_network`, etc. A Rust
`Err(String)` is rejected by Tauri as that string; the shim wraps it in an
`Error`, and the existing `desktop/frontend/bridge.js` wraps it again with
`bridgeMessage`, so callers still see the Go message.

The script also suppresses drag/drop navigation, complementing
`.disable_drag_drop_handler()`, and `on_navigation` rejects any origin except the
bundled `tauri://localhost` / `http(s)://tauri.localhost` document. A
`window.open` request is denied by `on_new_window`, matching the renderer's own
navigation guard.

## Window, monitor and scale semantics

Wails reports window sizes, positions and `Screen.Size` in logical pixels. Tauri
reports physical pixels plus scale factors. This crate stores the same logical
units in `settings.json`, so a profile written by the Go host reopens at the same
place:

- Initial `.inner_size` and `.min_inner_size` use saved logical width/height.
- `dom_ready` sets a saved position with `LogicalPosition` and clamps with
  `LogicalSize`.
- `screen_position_allowed` converts each Tauri monitor's physical size with
  that monitor's scale factor. It keeps the Go rule: negative positions are
  rejected; the current monitor is used, else the primary; a missing or
  non-positive monitor allows the position; the position must be `<=` the
  screen width/height.
- `clamp_window_size` reads the window's physical inner size and divides by the
  window scale factor, compares with the current monitor's logical size, and
  calls `set_size` only when the minimum changes something.
- `before_close` saves the window's physical inner size and outer position
  divided by the window scale factor. Maximised and minimised windows keep the
  previous bounds. The close is never prevented.
- Conversions round to the nearest logical pixel. Tauri's `inner_size` is the
  content area; Wails' GTK path reported the window size, so a client-side
  decoration can account for a few pixels. Positions are outer positions in both.

`available_monitors` is not needed: Tauri exposes `current_monitor` and
`primary_monitor` directly, which is the same selection Wails made from its
screen list.

## Security headers

The CSP is the Go `contentSecurityPolicy` constant verbatim, including
`frame-ancestors 'none'`, in `tauri.conf.json`.

| Header | Mechanism |
| --- | --- |
| `Content-Security-Policy` | Tauri config CSP; the hook only supplies it when absent so Tauri's nonces survive |
| `X-Content-Type-Options` | config + hook |
| `Cross-Origin-Opener-Policy` | config + hook |
| `Cross-Origin-Resource-Policy` | config + hook |
| `Permissions-Policy` | config + hook |
| `X-Frame-Options` | hook (no Tauri `HeaderConfig` key) |
| `Referrer-Policy` | hook (no Tauri `HeaderConfig` key) |

The hook is Tauri's documented `on_web_resource_request`, which applies to the
bundled `tauri` asset protocol. It does not cover a development server (none is
configured) or Tauri's own protocol error pages. Those are the two places where
the Go middleware had broader coverage.

`connect-src 'none'` is kept exactly as the renderer specifies. Tauri's preferred
IPC path is a `fetch` to `ipc://localhost`, which that CSP blocks; Tauri's
documented fallback then swaps to the native `window.ipc.postMessage` channel.
The bridge works, at the cost of one console warning on the first call. Adding
`ipc:` to `connect-src` would remove the warning but would widen the renderer's
network policy, so it was not done.

## Known differences

- Tauri's CSP header is the configured policy plus Tauri's nonces/hashes instead
  of the byte-identical Go header. The effective policy is equal or stricter.
- `X-Frame-Options` and `Referrer-Policy` are set by the response hook, not by
  Tauri config; protocol error pages do not get them. The CSP still carries
  `frame-ancestors 'none'`.
- Opening a valid external URL is best-effort. If the opener cannot start, the
  error is printed; `OpenExternal` resolves, as `BrowserOpenURL` returned
  nothing. Invalid URLs reject with `only http and https links can be opened`.
- The single-instance callback also focuses the restored window; Wails only
  unminimised and showed it. On Linux the plugin is installed only when a
  session bus is present: version 2.3 unwraps its D-Bus connection, while the
  Wails host kept running without the lock when no bus existed.
- The stored `AppHandle` is cleared on `RunEvent::Exit`; it is the Tauri
  equivalent of the Go context and would otherwise hold the manager in an
  `Arc` cycle.
- `AZAHAR_VERSION` is a compile-time override (`option_env!`), matching the
  `-ldflags -X main.version` role. The default is `2.0.0`.
- The `.desktop` entry, tarball packaging and icon set remain Wails packaging
  artifacts; only `desktop/build/appicon.png` is wired into Tauri's bundle
  config.

## Profile

`AZAHAR_PROFILE_DIR` (absolute, or made absolute) wins; otherwise
`dirs::config_dir()/azahar-admin`. The directory is created with mode 0700
(best-effort chmod on an existing directory, as the Go client did).
`Client::new` creates the same client the Go host used, including the keyring +
`0600` file session fallback and `settings.json` import.

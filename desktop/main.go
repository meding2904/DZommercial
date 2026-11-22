package main

import (
	"context"
	"embed"
	"errors"
	"log"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sync"

	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
	"github.com/wailsapp/wails/v2/pkg/options/linux"
	wailsRuntime "github.com/wailsapp/wails/v2/pkg/runtime"

	"azahar/desktop/internal/api"
)

//go:embed all:frontend
var assets embed.FS

//go:embed build/appicon.png
var appIcon []byte

var version = "2.0.0"

const contentSecurityPolicy = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"

func main() {
	app, err := NewApp(version)
	if err != nil {
		log.Fatal(err)
	}
	bounds := app.windowBounds()

	err = wails.Run(&options.App{
		Title:            "Azahar admin",
		Width:            bounds.Width,
		Height:           bounds.Height,
		MinWidth:         1080,
		MinHeight:        720,
		BackgroundColour: &options.RGBA{R: 246, G: 240, B: 235, A: 1},
		AssetServer: &assetserver.Options{
			Assets:     assets,
			Middleware: securityHeaders,
		},
		DragAndDrop: &options.DragAndDrop{
			DisableWebViewDrop: true,
		},
		OnStartup:     app.startup,
		OnDomReady:    app.domReady,
		OnBeforeClose: app.beforeClose,
		SingleInstanceLock: &options.SingleInstanceLock{
			UniqueId: "azahar-admin",
			OnSecondInstanceLaunch: func(options.SecondInstanceData) {
				ctx := app.context()
				if ctx == nil {
					return
				}
				wailsRuntime.WindowUnminimise(ctx)
				wailsRuntime.WindowShow(ctx)
			},
		},
		Linux: &linux.Options{
			ProgramName: "azahar-admin",
			Icon:        appIcon,
		},
		Bind: []interface{}{
			app,
		},
	})
	if err != nil {
		log.Fatal(err)
	}
}

func securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Security-Policy", contentSecurityPolicy)
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("X-Frame-Options", "DENY")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("Cross-Origin-Opener-Policy", "same-origin")
		w.Header().Set("Cross-Origin-Resource-Policy", "same-origin")
		w.Header().Set("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
		next.ServeHTTP(w, r)
	})
}

type App struct {
	ctxMu  sync.RWMutex
	ctx    context.Context
	client *api.Client
}

func NewApp(appVersion string) (*App, error) {
	profileDir, err := profileDirectory()
	if err != nil {
		return nil, err
	}
	client, err := api.NewClient(appVersion, profileDir)
	if err != nil {
		return nil, err
	}
	return &App{client: client}, nil
}

func profileDirectory() (string, error) {
	if override := os.Getenv("AZAHAR_PROFILE_DIR"); override != "" {
		return filepath.Abs(override)
	}
	root, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	dir := filepath.Join(root, "azahar-admin")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", err
	}
	return dir, nil
}

func (a *App) startup(ctx context.Context) {
	a.ctxMu.Lock()
	a.ctx = ctx
	a.ctxMu.Unlock()
}

func (a *App) context() context.Context {
	a.ctxMu.RLock()
	defer a.ctxMu.RUnlock()
	return a.ctx
}

func (a *App) domReady(ctx context.Context) {
	bounds := a.client.WindowBounds()
	if (bounds.X != 0 || bounds.Y != 0) && screenPositionAllowed(ctx, bounds) {
		wailsRuntime.WindowSetPosition(ctx, bounds.X, bounds.Y)
	}
	clampWindowSize(ctx)
}

// screenPositionAllowed keeps a saved window position from landing off-screen after
// the monitor layout changed. Wails reports screen sizes but not origins, so this is
// deliberately coarse: on a multi-monitor layout a saved position to the right of or
// below the primary screen is indistinguishable from an off-screen one and is dropped.
func screenPositionAllowed(ctx context.Context, bounds api.WindowBounds) bool {
	if bounds.X < 0 || bounds.Y < 0 {
		return false
	}
	screens, err := wailsRuntime.ScreenGetAll(ctx)
	if err != nil {
		return true
	}
	var target *wailsRuntime.Screen
	for i := range screens {
		if screens[i].IsCurrent {
			target = &screens[i]
			break
		}
	}
	if target == nil {
		for i := range screens {
			if screens[i].IsPrimary {
				target = &screens[i]
				break
			}
		}
	}
	if target == nil || target.Size.Width <= 0 || target.Size.Height <= 0 {
		return true
	}
	return bounds.X <= target.Size.Width && bounds.Y <= target.Size.Height
}

func (a *App) beforeClose(ctx context.Context) bool {
	if wailsRuntime.WindowIsMaximised(ctx) || wailsRuntime.WindowIsMinimised(ctx) {
		return false
	}
	width, height := wailsRuntime.WindowGetSize(ctx)
	if width <= 0 || height <= 0 {
		return false
	}
	x, y := wailsRuntime.WindowGetPosition(ctx)
	a.client.SaveWindowBounds(api.WindowBounds{
		Width:  width,
		Height: height,
		X:      x,
		Y:      y,
	})
	return false
}

func clampWindowSize(ctx context.Context) {
	screens, err := wailsRuntime.ScreenGetAll(ctx)
	if err != nil {
		return
	}
	var current *wailsRuntime.Screen
	for i := range screens {
		if screens[i].IsCurrent {
			current = &screens[i]
			break
		}
	}
	if current == nil || current.Size.Width <= 0 || current.Size.Height <= 0 {
		return
	}
	width, height := wailsRuntime.WindowGetSize(ctx)
	if width <= 0 || height <= 0 {
		return
	}
	nextWidth := min(width, current.Size.Width)
	nextHeight := min(height, current.Size.Height)
	if nextWidth != width || nextHeight != height {
		wailsRuntime.WindowSetSize(ctx, nextWidth, nextHeight)
	}
}

func (a *App) windowBounds() api.WindowBounds {
	return a.client.WindowBounds()
}

func (a *App) Info() api.Info {
	return a.client.Info()
}

func (a *App) Health() api.Health {
	return a.client.Health()
}

func (a *App) Request(method, path string, body any) api.Response {
	return a.client.Request(method, path, body)
}

func (a *App) Image(path string) api.Response {
	return a.client.Image(path)
}

func (a *App) UpdateSettings(apiURL, origin string, allowNetwork bool) (api.SettingsResult, error) {
	return a.client.UpdateSettings(apiURL, origin, allowNetwork)
}

func (a *App) ClearSession() error {
	return a.client.ClearSession()
}

func (a *App) FirewallGuide() api.FirewallGuide {
	return a.client.FirewallGuide()
}

func (a *App) OpenExternal(rawURL string) error {
	parsed, err := url.Parse(rawURL)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" || parsed.Hostname() == "" || parsed.User != nil {
		return errInvalidExternalURL
	}
	ctx := a.context()
	if ctx == nil {
		return errInvalidExternalURL
	}
	wailsRuntime.BrowserOpenURL(ctx, parsed.String())
	return nil
}

var errInvalidExternalURL = errors.New("only http and https links can be opened")

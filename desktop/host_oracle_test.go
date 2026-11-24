//go:build oracle

// Oracle adapter for the Tauri host differential test in
// crates/azahar-desktop/tests/differential_host.rs.
//
// Why `desktop/main.go` is not called for every case:
//
//   - `screenPositionAllowed` (main.go:151-178) and `clampWindowSize`
//     (main.go:198-222) call `wailsRuntime.ScreenGetAll`, `WindowGetSize` and
//     `WindowSetSize`. Those read a live Wails runtime out of the
//     `context.Context` created by `wails.Run`; with `context.Background()`
//     they panic or block, and the run itself needs a GTK/WebKit display. The
//     pure arithmetic is copied below verbatim (only the runtime reads are
//     injected) and the Rust side is compared against it.
//   - `OpenExternal` (main.go:256-267) also checks `a.context()` before it
//     calls `BrowserOpenURL`, so only its URL condition (main.go:257-259) is
//     reachable here. That condition is copied exactly; the context check and
//     process launch are left to the Rust and live smoke tests.
//   - `profileDirectory` (main.go:112-125) is plain environment and filesystem
//     code, so it is called directly. `api.NewClient` is called afterwards
//     because `NewApp` (main.go:100-110) always does, and the client performs
//     the 0700 creation/chmod for the override path that `profileDirectory`
//     itself leaves to it.
//   - There is no Go side for the bridge contract: Wails generates
//     `window.go.main.App` bindings at build time and ships no JavaScript shim.
//     The Rust differential test parses `bridge.js` against the documented
//     eight-method surface instead.
//
// Input is a JSON document read from AZAHAR_HOST_ORACLE_INPUT. The result is
// written to AZAHAR_HOST_ORACLE_OUTPUT when set, otherwise printed to stdout
// after the marker below.
package main

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"testing"

	"azahar/desktop/internal/api"
)

const oracleOutputMarker = "AZAHAR_HOST_ORACLE_RESULT:"

type oracleInput struct {
	Profile  []oracleProfileCase  `json:"profile"`
	Clamp    []oracleClampCase    `json:"clamp"`
	Position []oraclePositionCase `json:"position"`
	URLs     []oracleURLCase      `json:"urls"`
}

type oracleProfileCase struct {
	ID       string  `json:"id"`
	Override *string `json:"override"`
	Home     *string `json:"home"`
	XDG      *string `json:"xdg"`
	Cwd      string  `json:"cwd"`
}

type oracleClampCase struct {
	ID           string `json:"id"`
	Width        int    `json:"width"`
	Height       int    `json:"height"`
	ScreenWidth  int    `json:"screenWidth"`
	ScreenHeight int    `json:"screenHeight"`
}

type oracleScreen struct {
	Width   int  `json:"width"`
	Height  int  `json:"height"`
	Current bool `json:"current"`
	Primary bool `json:"primary"`
}

type oraclePositionCase struct {
	ID          string         `json:"id"`
	X           int            `json:"x"`
	Y           int            `json:"y"`
	Screens     []oracleScreen `json:"screens"`
	ScreenError bool           `json:"screenError"`
}

type oracleURLCase struct {
	ID  string `json:"id"`
	URL string `json:"url"`
}

type oracleResults struct {
	Profile  []oracleProfileResult  `json:"profile"`
	Clamp    []oracleClampResult    `json:"clamp"`
	Position []oraclePositionResult `json:"position"`
	URLs     []oracleURLResult      `json:"urls"`
}

type oracleProfileResult struct {
	ID          string `json:"id"`
	OK          bool   `json:"ok"`
	Path        string `json:"path"`
	Mode        uint32 `json:"mode"`
	Exists      bool   `json:"exists"`
	Error       string `json:"error,omitempty"`
	ClientError string `json:"clientError,omitempty"`
}

type oracleClampResult struct {
	ID     string `json:"id"`
	OK     bool   `json:"ok"`
	Width  int    `json:"width"`
	Height int    `json:"height"`
}

type oraclePositionResult struct {
	ID      string `json:"id"`
	OK      bool   `json:"ok"`
	Allowed bool   `json:"allowed"`
}

type oracleURLResult struct {
	ID         string `json:"id"`
	OK         bool   `json:"ok"`
	Accepted   bool   `json:"accepted"`
	Normalized string `json:"normalized,omitempty"`
}

// oracleClampWindowSize is the effective result of
// `desktop.clampWindowSize` (desktop/main.go:198-222). The early returns map to
// "leave the size unchanged", exactly what the Wails host does by not calling
// WindowSetSize. The final min expressions are copied verbatim; only
// `current.Size.Width`/`Height` are injected as screenWidth/screenHeight.
func oracleClampWindowSize(width, height, screenWidth, screenHeight int) (int, int) {
	// main.go:210-212: no current screen or a non-positive screen returns.
	if screenWidth <= 0 || screenHeight <= 0 {
		return width, height
	}
	// main.go:213-216: a non-positive window size returns.
	if width <= 0 || height <= 0 {
		return width, height
	}
	// main.go:217-221:
	nextWidth := min(width, screenWidth)
	nextHeight := min(height, screenHeight)
	if nextWidth != width || nextHeight != height {
		return nextWidth, nextHeight
	}
	return width, height
}

// oracleScreenPositionAllowed is `desktop.screenPositionAllowed`
// (desktop/main.go:151-178) with the Wails screen list injected. The early
// returns and comparisons are copied verbatim; only ScreenGetAll is replaced by
// the screens/screenError arguments.
func oracleScreenPositionAllowed(bounds api.WindowBounds, screens []oracleScreen, screenError bool) bool {
	// main.go:152-154.
	if bounds.X < 0 || bounds.Y < 0 {
		return false
	}
	// main.go:155-158: a ScreenGetAll error allows the position.
	if screenError {
		return true
	}
	// main.go:159-165.
	var target *oracleScreen
	for i := range screens {
		if screens[i].Current {
			target = &screens[i]
			break
		}
	}
	// main.go:166-173.
	if target == nil {
		for i := range screens {
			if screens[i].Primary {
				target = &screens[i]
				break
			}
		}
	}
	// main.go:174-176.
	if target == nil || target.Width <= 0 || target.Height <= 0 {
		return true
	}
	// main.go:177.
	return bounds.X <= target.Width && bounds.Y <= target.Height
}

// oracleOpenExternalAllowed copies the OpenExternal validation condition from
// desktop/main.go:257-259 exactly. The context check and BrowserOpenURL on the
// following lines need the live Wails runtime and are not called here.
func oracleOpenExternalAllowed(raw string) bool {
	parsed, err := url.Parse(raw)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" || parsed.Hostname() == "" || parsed.User != nil {
		return false
	}
	return true
}

func oracleProfile(t *testing.T, testCase oracleProfileCase) oracleProfileResult {
	t.Helper()
	result := oracleProfileResult{ID: testCase.ID}
	if testCase.Cwd != "" {
		// This test is serial, so the process-wide chdir is safe and matches
		// the Rust side, which resolves relative overrides from this cwd.
		// T.Chdir restores the original working directory at cleanup.
		t.Chdir(testCase.Cwd)
	}
	setOrUnset("AZAHAR_PROFILE_DIR", testCase.Override)
	setOrUnset("HOME", testCase.Home)
	setOrUnset("XDG_CONFIG_HOME", testCase.XDG)

	dir, err := profileDirectory()
	if err != nil {
		result.Error = err.Error()
		return result
	}
	result.OK = true
	result.Path = dir

	// NewApp always constructs a client with the profile directory. It is the
	// step that creates and chmods an override path, so include it to compare
	// the final host state rather than only profileDirectory's first half.
	if _, clientErr := api.NewClient("2.0.0-oracle", dir); clientErr != nil {
		result.ClientError = clientErr.Error()
		return result
	}
	info, statErr := os.Stat(dir)
	if statErr != nil {
		result.Error = statErr.Error()
		return result
	}
	result.Exists = info.IsDir()
	result.Mode = uint32(info.Mode().Perm())
	return result
}

func setOrUnset(name string, value *string) {
	if value == nil {
		_ = os.Unsetenv(name)
		return
	}
	_ = os.Setenv(name, *value)
}

// builtinInput keeps `go test -tags oracle -run TestOracleHost .` useful
// without the Rust harness: it drives each oracle path once with temporary
// directories. The Rust side always supplies its own input file.
func builtinInput(t *testing.T) oracleInput {
	t.Helper()
	root := t.TempDir()
	home := filepath.Join(root, "home")
	xdg := filepath.Join(root, "xdg")
	for _, dir := range []string{home, xdg} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	absolute := filepath.Join(root, "absolute profile")
	relative := "relative-profile"
	empty := ""
	return oracleInput{
		Profile: []oracleProfileCase{
			{ID: "builtin-absolute", Override: &absolute, Cwd: root},
			{ID: "builtin-relative", Override: &relative, Cwd: root},
			{ID: "builtin-default", Override: &empty, Home: &home, XDG: &xdg, Cwd: root},
		},
		Clamp: []oracleClampCase{
			{ID: "builtin-clamp-fit", Width: 1380, Height: 900, ScreenWidth: 1920, ScreenHeight: 1080},
			{ID: "builtin-clamp-wide", Width: 2400, Height: 900, ScreenWidth: 1920, ScreenHeight: 1080},
			{ID: "builtin-clamp-zero", Width: 0, Height: 900, ScreenWidth: 1920, ScreenHeight: 1080},
		},
		Position: []oraclePositionCase{
			{ID: "builtin-position-current", X: 10, Y: 10, Screens: []oracleScreen{{Width: 1920, Height: 1080, Current: true}}},
			{ID: "builtin-position-primary", X: 2000, Y: 10, Screens: []oracleScreen{{Width: 1920, Height: 1080, Primary: true}}},
			{ID: "builtin-position-negative", X: -1, Y: 10, Screens: []oracleScreen{{Width: 1920, Height: 1080, Current: true}}},
		},
		URLs: []oracleURLCase{
			{ID: "builtin-url-https", URL: "https://example.com/path?q=1#frag"},
			{ID: "builtin-url-userinfo", URL: "https://user:pass@example.com/"},
			{ID: "builtin-url-escape", URL: "https://example.com/%zz"},
		},
	}
}

func TestOracleHost(t *testing.T) {
	// Keep the keyring out of the profile cases: the default secret store
	// selector would otherwise probe a live Secret Service for every profile.
	// The Rust side compares only the profile path and mode, not session I/O.
	for name, value := range map[string]string{
		"AZAHAR_SESSION_BACKEND": "file",
		"AZAHAR_DISABLE_KEYRING": "1",
		"AZAHAR_API_URL":         "",
		"AZAHAR_ORIGIN":          "",
	} {
		t.Setenv(name, value)
	}

	input := builtinInput(t)
	if path := os.Getenv("AZAHAR_HOST_ORACLE_INPUT"); path != "" {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read oracle input %q: %v", path, err)
		}
		input = oracleInput{}
		if err := json.Unmarshal(data, &input); err != nil {
			t.Fatalf("decode oracle input %q: %v", path, err)
		}
	}

	results := oracleResults{
		Profile:  make([]oracleProfileResult, 0, len(input.Profile)),
		Clamp:    make([]oracleClampResult, 0, len(input.Clamp)),
		Position: make([]oraclePositionResult, 0, len(input.Position)),
		URLs:     make([]oracleURLResult, 0, len(input.URLs)),
	}

	for _, testCase := range input.Profile {
		results.Profile = append(results.Profile, oracleProfile(t, testCase))
	}
	for _, testCase := range input.Clamp {
		width, height := oracleClampWindowSize(testCase.Width, testCase.Height, testCase.ScreenWidth, testCase.ScreenHeight)
		results.Clamp = append(results.Clamp, oracleClampResult{
			ID:     testCase.ID,
			OK:     true,
			Width:  width,
			Height: height,
		})
	}
	for _, testCase := range input.Position {
		screens := make([]oracleScreen, 0, len(testCase.Screens))
		screens = append(screens, testCase.Screens...)
		allowed := oracleScreenPositionAllowed(
			api.WindowBounds{X: testCase.X, Y: testCase.Y},
			screens,
			testCase.ScreenError,
		)
		results.Position = append(results.Position, oraclePositionResult{
			ID:      testCase.ID,
			OK:      true,
			Allowed: allowed,
		})
	}
	for _, testCase := range input.URLs {
		accepted := oracleOpenExternalAllowed(testCase.URL)
		result := oracleURLResult{ID: testCase.ID, OK: true, Accepted: accepted}
		if accepted {
			parsed, err := url.Parse(testCase.URL)
			if err != nil {
				t.Fatalf("%s: accepted URL does not parse: %v", testCase.ID, err)
			}
			// The Rust side compares the accept/reject decision only: WHATWG
			// normalisation (lowercase host, IDN punycode, added slash) differs
			// from net/url's string form even when both accept the URL.
			result.Normalized = parsed.String()
		}
		results.URLs = append(results.URLs, result)
	}

	t.Logf("oracle host cases: %d profile, %d clamp, %d position, %d urls",
		len(results.Profile), len(results.Clamp), len(results.Position), len(results.URLs))
	t.Log("note: screenPositionAllowed/clampWindowSize values come from the pure expressions copied from main.go:151-178 and main.go:198-222; the Wails runtime calls cannot run headlessly")
	t.Log("note: OpenExternal is the condition copied from main.go:257-259; its context check and BrowserOpenURL are not exercised")
	t.Log("note: there is no Go bridge to compare; Wails generates window.go.main.App at build time")

	data, err := json.MarshalIndent(results, "", "  ")
	if err != nil {
		t.Fatalf("encode oracle results: %v", err)
	}
	if path := os.Getenv("AZAHAR_HOST_ORACLE_OUTPUT"); path != "" {
		if err := os.WriteFile(path, data, 0o600); err != nil {
			t.Fatalf("write oracle output %q: %v", path, err)
		}
		return
	}
	fmt.Printf("%s%s\n", oracleOutputMarker, data)
}

package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestSettingsDefaults(t *testing.T) {
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	profile := filepath.Join(t.TempDir(), "profile")
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	info := c.Info()
	if info.Version != "2.0.0-test" || info.Platform != runtime.GOOS {
		t.Fatalf("info = %#v", info)
	}
	if info.APIURL != defaultAPIURL || info.Origin != "" || info.ResolvedOrigin != "" || !info.NetworkAllowed {
		t.Fatalf("info = %#v", info)
	}
	if c.settings.NetworkAllowedHost != "" {
		t.Fatalf("consent host = %q, want empty for loopback", c.settings.NetworkAllowedHost)
	}
	if c.timeout != 20*time.Second || c.imageTimeout != 15*time.Second {
		t.Fatalf("timeouts = %v, %v", c.timeout, c.imageTimeout)
	}
	bounds := c.WindowBounds()
	if bounds.Width != defaultWindowWidth || bounds.Height != defaultWindowHeight {
		t.Fatalf("bounds = %#v", bounds)
	}
}

func TestNewClientWithFileSecretBackend(t *testing.T) {
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	t.Setenv("AZAHAR_DISABLE_KEYRING", "1")
	c, err := NewClient("2.0.0-test", filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if _, ok := c.secrets.(*fileSecretStore); !ok {
		t.Fatalf("secret backend = %T, want file", c.secrets)
	}
	if info := c.Info(); info.APIURL != defaultAPIURL || info.Origin != "" || !info.NetworkAllowed {
		t.Fatalf("info = %#v", info)
	}
}

func TestSettingsEnvOverrides(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}
	saved := Settings{
		APIURL:             "https://saved.example:8443",
		Origin:             "http://saved-origin.example",
		NetworkAllowed:     true,
		NetworkAllowedHost: "saved.example",
	}
	writeSettingsFile(t, profile, saved)

	t.Run("valid overrides", func(t *testing.T) {
		t.Setenv("AZAHAR_API_URL", "http://127.0.0.1:9090/")
		t.Setenv("AZAHAR_ORIGIN", "https://override.example/")
		c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
		if err != nil {
			t.Fatalf("new client: %v", err)
		}
		info := c.Info()
		if info.APIURL != "http://127.0.0.1:9090" || info.Origin != "https://override.example" {
			t.Fatalf("info = %#v", info)
		}
		if !info.NetworkAllowed {
			t.Fatal("saved network consent was lost")
		}
	})

	t.Run("bad overrides are ignored", func(t *testing.T) {
		t.Setenv("AZAHAR_API_URL", "http://public.example")
		t.Setenv("AZAHAR_ORIGIN", "ftp://bad.example")
		c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
		if err != nil {
			t.Fatalf("new client: %v", err)
		}
		info := c.Info()
		if info.APIURL != saved.APIURL || info.Origin != saved.Origin || !info.NetworkAllowed {
			t.Fatalf("info = %#v, want saved settings", info)
		}
	})
}

func TestSettingsPersistenceAndModes(t *testing.T) {
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		writeJSON(t, w, http.StatusOK, map[string]any{
			"env":    "test",
			"origin": "http://localhost:3000",
		})
	}))
	defer server.Close()

	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o755); err != nil {
		t.Fatal(err)
	}
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	result, err := c.UpdateSettings(server.URL+"/", "", false)
	if err != nil {
		t.Fatalf("update settings: %v", err)
	}
	if result.APIURL != server.URL || result.Origin != "" ||
		result.ResolvedOrigin != "http://localhost:3000" || !result.NetworkAllowed {
		t.Fatalf("result = %#v", result)
	}

	dirInfo, err := os.Stat(profile)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && dirInfo.Mode().Perm() != 0o700 {
		t.Fatalf("profile mode = %o, want 700", dirInfo.Mode().Perm())
	}
	settingsPath := filepath.Join(profile, "settings.json")
	settingsInfo, err := os.Stat(settingsPath)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && settingsInfo.Mode().Perm() != 0o600 {
		t.Fatalf("settings mode = %o, want 600", settingsInfo.Mode().Perm())
	}

	var stored Settings
	readSettingsFile(t, settingsPath, &stored)
	if stored.APIURL != server.URL || stored.Origin != "" || stored.NetworkAllowed || stored.NetworkAllowedHost != "" {
		t.Fatalf("stored settings = %#v", stored)
	}

	reloaded, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("reload: %v", err)
	}
	if got := reloaded.Info(); got.APIURL != server.URL || got.Origin != "" {
		t.Fatalf("reloaded info = %#v", got)
	}

	bounds := WindowBounds{Width: 1200, Height: 800, X: 40, Y: 30}
	reloaded.SaveWindowBounds(bounds)
	if got := reloaded.WindowBounds(); got != bounds {
		t.Fatalf("bounds = %#v, want %#v", got, bounds)
	}
	readSettingsFile(t, settingsPath, &stored)
	if stored.Window != bounds {
		t.Fatalf("stored window = %#v, want %#v", stored.Window, bounds)
	}

	again, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("reload window: %v", err)
	}
	if got := again.WindowBounds(); got != bounds {
		t.Fatalf("reloaded bounds = %#v, want %#v", got, bounds)
	}

	reloaded.SaveWindowBounds(WindowBounds{Width: -1, Height: 0})
	if got := reloaded.WindowBounds(); got.Width != defaultWindowWidth || got.Height != defaultWindowHeight {
		t.Fatalf("invalid bounds were not replaced: %#v", got)
	}
}

func TestUpdateSettingsFallsBackWhenHealthFails(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		writeProblem(t, w, http.StatusServiceUnavailable, "unavailable", "Try later.")
	}))
	defer server.Close()

	c, _ := testClient(t, "")
	result, err := c.UpdateSettings(server.URL, "", false)
	if err != nil {
		t.Fatalf("update settings: %v", err)
	}
	if result.APIURL != server.URL || result.ResolvedOrigin != fallbackOrigin {
		t.Fatalf("result = %#v, want fallback origin", result)
	}
}

func TestSettingsOriginOverride(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://server.example"})
	}))
	defer server.Close()

	c, _ := testClient(t, "")
	result, err := c.UpdateSettings(server.URL, "http://admin.example", true)
	if err != nil {
		t.Fatalf("update settings: %v", err)
	}
	if result.Origin != "http://admin.example" || result.ResolvedOrigin != "http://admin.example" {
		t.Fatalf("result = %#v", result)
	}
	if !result.NetworkAllowed {
		t.Fatal("network consent was not saved")
	}
}

func writeSettingsFile(t *testing.T, profile string, settings Settings) {
	t.Helper()
	data, err := json.Marshal(settings)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, "settings.json"), data, 0o600); err != nil {
		t.Fatal(err)
	}
}

func readSettingsFile(t *testing.T, path string, settings *Settings) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, settings); err != nil {
		t.Fatal(err)
	}
}

func TestSettingsDecodingIsLenient(t *testing.T) {
	unsetEnv(t, "AZAHAR_API_URL")
	unsetEnv(t, "AZAHAR_ORIGIN")
	profile := filepath.Join(t.TempDir(), "profile")
	settingsPath := filepath.Join(profile, "settings.json")

	write := func(data string) {
		t.Helper()
		if err := os.MkdirAll(profile, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(settingsPath, []byte(data), 0o600); err != nil {
			t.Fatal(err)
		}
	}

	write(`{"window":"broken","apiUrl":"https://saved.example","origin":"http://saved-origin.example","networkAllowedHost":"saved.example"}`)
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if info := c.Info(); info.APIURL != "https://saved.example" ||
		info.Origin != "http://saved-origin.example" || !info.NetworkAllowed {
		t.Fatalf("info = %#v", info)
	}

	write(`{"apiUrl":123,"origin":"http://ok.example","window":{"width":"bad"}}`)
	c, err = newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if info := c.Info(); info.APIURL != defaultAPIURL || info.Origin != "http://ok.example" || !info.NetworkAllowed {
		t.Fatalf("info = %#v", info)
	}

	write(`{"apiUrl":`)
	c, err = newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if info := c.Info(); info.APIURL != defaultAPIURL || info.Origin != "" || !info.NetworkAllowed {
		t.Fatalf("info = %#v", info)
	}
}

func TestNewClientToleratesChmodFailure(t *testing.T) {
	if runtime.GOOS != "linux" {
		t.Skip("needs a Linux virtual directory")
	}
	profile := "/proc/self/fd"
	if info, statErr := os.Stat(profile); statErr != nil || !info.IsDir() {
		t.Skip("virtual directory is unavailable")
	}
	if err := os.Chmod(profile, 0o700); err == nil {
		t.Skip("chmod unexpectedly succeeded")
	}
	t.Setenv("AZAHAR_DISABLE_KEYRING", "1")
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	c, err := NewClient("2.0.0-test", profile)
	if err != nil {
		t.Fatalf("NewClient failed on an unwritable profile: %v", err)
	}
	if c == nil {
		t.Fatal("NewClient returned no client")
	}
}

func TestUpdateSettingsSaveFailureIsFriendly(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(filepath.Join(profile, "settings.json"), 0o700); err != nil {
		t.Fatal(err)
	}
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	_, err = c.UpdateSettings("http://127.0.0.1:9090", "", false)
	if err == nil || !strings.Contains(err.Error(), "profile folder is writable") {
		t.Fatalf("error = %v, want the friendly save error", err)
	}
}

func TestUpdateSettingsSaveFailureKeepsSession(t *testing.T) {
	unsetEnv(t, "AZAHAR_API_URL")
	unsetEnv(t, "AZAHAR_ORIGIN")
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())

	profile := filepath.Join(t.TempDir(), "profile")
	secrets := &memorySecrets{}
	c, err := newClientWithSecrets("2.0.0-test", profile, secrets)
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	expires := time.Date(2030, 1, 1, 0, 0, 0, 0, time.UTC)
	wantSession := storedCookie{value: "session-token", expires: expires}
	wantCSRF := storedCookie{value: "csrf-token", expires: expires}
	wantOrigin := "http://localhost:3000"
	wantGeneration := uint64(7)
	wantImageBytes := int64(31)

	c.mu.Lock()
	c.resolvedOrigin = wantOrigin
	c.session = wantSession
	c.csrf = wantCSRF
	c.generation = wantGeneration
	c.imageCache["/api/images/one"] = "data:image/png;base64,AAAA"
	c.imageOrder = []string{"/api/images/one"}
	c.imageCacheBytes = wantImageBytes
	original := c.settings
	c.mu.Unlock()

	if err := os.MkdirAll(c.settingsPath, 0o700); err != nil {
		t.Fatalf("prepare failing settings path: %v", err)
	}

	_, err = c.UpdateSettings("http://127.0.0.1:9090", "http://admin.example", true)
	if err == nil || err.Error() != "The settings could not be saved. Check that the profile folder is writable." {
		t.Fatalf("error = %v, want the friendly save error", err)
	}

	c.mu.RLock()
	gotSettings := c.settings
	gotOrigin := c.resolvedOrigin
	gotSession := c.session
	gotCSRF := c.csrf
	gotGeneration := c.generation
	gotOrder := append([]string(nil), c.imageOrder...)
	gotImageBytes := c.imageCacheBytes
	gotImage, gotImageOK := c.imageCache["/api/images/one"]
	gotImageCount := len(c.imageCache)
	gotSecrets := c.secrets
	c.mu.RUnlock()

	if gotSettings != original {
		t.Fatalf("settings = %#v, want %#v", gotSettings, original)
	}
	if gotOrigin != wantOrigin {
		t.Fatalf("resolved origin = %q, want %q", gotOrigin, wantOrigin)
	}
	if gotSession != wantSession {
		t.Fatalf("session = %#v, want %#v", gotSession, wantSession)
	}
	if gotCSRF != wantCSRF {
		t.Fatalf("csrf = %#v, want %#v", gotCSRF, wantCSRF)
	}
	if gotGeneration != wantGeneration {
		t.Fatalf("generation = %d, want %d", gotGeneration, wantGeneration)
	}
	if gotImageCount != 1 || !gotImageOK || gotImage != "data:image/png;base64,AAAA" {
		t.Fatalf("image cache = %v (present %v, %d entries)", gotImage, gotImageOK, gotImageCount)
	}
	if len(gotOrder) != 1 || gotOrder[0] != "/api/images/one" {
		t.Fatalf("image order = %v", gotOrder)
	}
	if gotImageBytes != wantImageBytes {
		t.Fatalf("image cache bytes = %d, want %d", gotImageBytes, wantImageBytes)
	}
	if gotSecrets != secretStore(secrets) {
		t.Fatalf("secret store = %T, want the original store", gotSecrets)
	}
}

func TestUpdateSettingsSkipsIdenticalValues(t *testing.T) {
	var healthHits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		healthHits.Add(1)
		writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://localhost:3000"})
	}))
	defer server.Close()

	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}
	writeSettingsFile(t, profile, Settings{
		APIURL: server.URL,
	})
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	settingsPath := filepath.Join(profile, "settings.json")
	if err := os.Remove(settingsPath); err != nil {
		t.Fatal(err)
	}

	result, err := c.UpdateSettings(server.URL, "", true)
	if err != nil {
		t.Fatalf("unchanged update: %v", err)
	}
	if result.APIURL != server.URL || !result.NetworkAllowed {
		t.Fatalf("result = %#v", result)
	}
	if healthHits.Load() != 0 {
		t.Fatalf("unchanged update made %d health requests", healthHits.Load())
	}
	if _, err := os.Stat(settingsPath); !os.IsNotExist(err) {
		t.Fatalf("unchanged update wrote settings: %v", err)
	}
}

func TestConsentIsPerHost(t *testing.T) {
	c, _ := testClient(t, "")
	origin := "http://admin.example"

	result, err := c.UpdateSettings("https://a.example.com", origin, true)
	if err != nil {
		t.Fatalf("consent for A: %v", err)
	}
	if result.APIURL != "https://a.example.com" || !result.NetworkAllowed {
		t.Fatalf("result = %#v", result)
	}
	c.mu.RLock()
	host := c.settings.NetworkAllowedHost
	c.mu.RUnlock()
	if host != "a.example.com" {
		t.Fatalf("consent host = %q", host)
	}

	_, err = c.UpdateSettings("https://b.example.com", origin, false)
	var apiErr *Error
	if err == nil || !errors.As(err, &apiErr) || apiErr.Code != "network_consent_required" {
		t.Fatalf("B without consent error = %v", err)
	}
	if info := c.Info(); info.APIURL != "https://a.example.com" || !info.NetworkAllowed {
		t.Fatalf("settings changed after rejected switch: %#v", info)
	}

	result, err = c.UpdateSettings("https://b.example.com", origin, true)
	if err != nil {
		t.Fatalf("consent for B: %v", err)
	}
	if result.APIURL != "https://b.example.com" || !result.NetworkAllowed {
		t.Fatalf("result = %#v", result)
	}

	_, err = c.UpdateSettings("https://a.example.com", origin, false)
	if err == nil || !errors.As(err, &apiErr) || apiErr.Code != "network_consent_required" {
		t.Fatalf("A after B without consent error = %v", err)
	}

	result, err = c.UpdateSettings("http://127.0.0.1:9090", origin, false)
	if err != nil {
		t.Fatalf("loopback update: %v", err)
	}
	if !result.NetworkAllowed {
		t.Fatalf("loopback result = %#v", result)
	}
	c.mu.RLock()
	host = c.settings.NetworkAllowedHost
	c.mu.RUnlock()
	if host != "" {
		t.Fatalf("consent host = %q after loopback, want empty", host)
	}
}

func TestSettingsConsentRequiresMatchingHost(t *testing.T) {
	unsetEnv(t, "AZAHAR_API_URL")
	unsetEnv(t, "AZAHAR_ORIGIN")
	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}

	writeSettingsFile(t, profile, Settings{
		APIURL:         "https://saved.example",
		NetworkAllowed: true,
	})
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if c.Info().NetworkAllowed {
		t.Fatal("the legacy global consent flag granted network access")
	}

	writeSettingsFile(t, profile, Settings{
		APIURL:             "https://saved.example",
		NetworkAllowedHost: "other.example",
	})
	c, err = newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if c.Info().NetworkAllowed {
		t.Fatal("consent for another host granted network access")
	}
	c.mu.RLock()
	host := c.settings.NetworkAllowedHost
	c.mu.RUnlock()
	if host != "" {
		t.Fatalf("stale consent host = %q, want empty", host)
	}

	writeSettingsFile(t, profile, Settings{
		APIURL:             "https://saved.example",
		NetworkAllowedHost: "saved.example",
	})
	c, err = newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if !c.Info().NetworkAllowed {
		t.Fatal("matching host consent was not loaded")
	}
}

func TestLegacyElectronSettingsImport(t *testing.T) {
	if runtime.GOOS != "linux" {
		t.Skip("XDG_CONFIG_HOME is used by this test")
	}
	unsetEnv(t, "AZAHAR_API_URL")
	unsetEnv(t, "AZAHAR_ORIGIN")
	config := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", config)
	legacyDir := filepath.Join(config, "Azahar Admin")
	if err := os.MkdirAll(legacyDir, 0o700); err != nil {
		t.Fatal(err)
	}
	legacy := legacySettings{
		APIURL: "https://legacy.example.com",
		Origin: "https://legacy.example.com",
		Window: WindowBounds{Width: 1200, Height: 800, X: 40, Y: 50},
	}
	data, err := json.Marshal(legacy)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(legacyDir, "settings.json"), data, 0o600); err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(t.TempDir(), "profile")
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	info := c.Info()
	if info.APIURL != legacy.APIURL || info.Origin != legacy.Origin {
		t.Fatalf("imported info = %#v", info)
	}
	bounds := c.WindowBounds()
	if bounds.Width != 1200 || bounds.Height != 800 || bounds.X != 40 || bounds.Y != 50 {
		t.Fatalf("imported bounds = %#v", bounds)
	}
	if _, err := os.Stat(filepath.Join(profile, "settings.json")); err != nil {
		t.Fatalf("imported settings were not persisted: %v", err)
	}
}

func TestConsentCanBeRevokedForCurrentHost(t *testing.T) {
	unsetEnv(t, "AZAHAR_API_URL")
	unsetEnv(t, "AZAHAR_ORIGIN")
	c, err := newClientWithSecrets("2.0.0-test", filepath.Join(t.TempDir(), "profile"), &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if _, err := c.UpdateSettings("https://a.example.com", "https://a.example.com", true); err != nil {
		t.Fatalf("grant consent: %v", err)
	}
	if !c.Info().NetworkAllowed {
		t.Fatal("consent was not granted")
	}
	if _, err := c.UpdateSettings("https://a.example.com", "https://a.example.com", false); err != nil {
		t.Fatalf("revoke consent: %v", err)
	}
	if c.Info().NetworkAllowed {
		t.Fatal("consent was not revoked for the current host")
	}
	c.mu.RLock()
	host := c.settings.NetworkAllowedHost
	c.mu.RUnlock()
	if host != "" {
		t.Fatalf("consent host = %q, want empty", host)
	}
}

//go:build oracle

package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"azahar/desktop/internal/api"
)

// This file is the scripted differential driver for the desktop API. Each
// command builds a real Client, installs a fake http.RoundTripper and applies
// a list of steps, so the same script can be replayed by the Rust port.

// scriptEnv carries per-client environment values.
type scriptEnv struct {
	APIURL         *string `json:"api_url"`
	Origin         *string `json:"origin"`
	SessionBackend *string `json:"session_backend"`
	DisableKeyring *string `json:"disable_keyring"`
}

type transportResponse struct {
	Status      int      `json:"status"`
	Body        string   `json:"body"`
	BodyB64     string   `json:"body_b64"`
	ContentType string   `json:"content_type"`
	SetCookies  []string `json:"set_cookies"`
	Oversize    bool     `json:"oversize"`
	Error       string   `json:"error"`
}

type transportSpec struct {
	Mode      string              `json:"mode"`
	Responses []transportResponse `json:"responses"`
}

type scriptStep struct {
	Kind                 string            `json:"kind"`
	APIURL               string            `json:"api_url"`
	Origin               string            `json:"origin"`
	AllowNetwork         bool              `json:"allow_network"`
	Force                bool              `json:"force"`
	Bounds               *api.WindowBounds `json:"bounds"`
	Value                string            `json:"value"`
	Token                string            `json:"token"`
	Key                  string            `json:"key"`
	Path                 string            `json:"path"`
	Method               string            `json:"method"`
	Body                 json.RawMessage   `json:"body"`
	BodyPresent          bool              `json:"body_present"`
	Generation           uint64            `json:"generation"`
	UseCurrentGeneration bool              `json:"use_current_generation"`
	SetCookies           []string          `json:"set_cookies"`
	ExpiresUnix          int64             `json:"expires_unix"`
}

type clientScript struct {
	Version         string        `json:"version"`
	SettingsRaw     *string       `json:"settings_raw"`
	SettingsDir     bool          `json:"settings_dir"`
	SecretRaw       *string       `json:"secret_raw"`
	LegacyRaw       *string       `json:"legacy_raw"`
	LegacyKind      string        `json:"legacy_kind"`
	Env             scriptEnv     `json:"env"`
	Secrets         string        `json:"secrets"`
	Transport       transportSpec `json:"transport"`
	ImageCacheLimit *int64        `json:"image_cache_limit"`
	Steps           []scriptStep  `json:"steps"`
}

type observedRequest struct {
	Method  string              `json:"method"`
	URL     string              `json:"url"`
	Headers map[string][]string `json:"headers"`
	Body    string              `json:"body"`
}

type scriptedTimeout struct{}

func (scriptedTimeout) Error() string   { return "scripted timeout" }
func (scriptedTimeout) Timeout() bool   { return true }
func (scriptedTimeout) Temporary() bool { return true }

type scriptedTransport struct {
	mu        sync.Mutex
	mode      string
	responses []transportResponse
	index     int
	requests  []observedRequest
}

func (t *scriptedTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	body, _ := io.ReadAll(request.Body)
	headers := make(map[string][]string, len(request.Header))
	for name, values := range request.Header {
		lower := strings.ToLower(name)
		copied := append([]string(nil), values...)
		if lower == "user-agent" {
			for index := range copied {
				copied[index] = normalizeUserAgent(copied[index])
			}
		}
		headers[lower] = copied
	}
	t.mu.Lock()
	t.requests = append(t.requests, observedRequest{
		Method:  request.Method,
		URL:     request.URL.String(),
		Headers: headers,
		Body:    string(body),
	})
	t.mu.Unlock()

	if t.mode == "fail" {
		return nil, errors.New("scripted network failure")
	}
	if t.mode == "timeout" {
		return nil, scriptedTimeout{}
	}

	t.mu.Lock()
	spec := transportResponse{Status: http.StatusOK, Body: "{}"}
	if t.index < len(t.responses) {
		spec = t.responses[t.index]
		t.index++
	}
	t.mu.Unlock()

	switch spec.Error {
	case "fail":
		return nil, errors.New("scripted network failure")
	case "timeout":
		return nil, scriptedTimeout{}
	}

	header := http.Header{}
	if spec.ContentType != "" {
		header.Set("Content-Type", spec.ContentType)
	}
	for _, cookie := range spec.SetCookies {
		header.Add("Set-Cookie", cookie)
	}
	if spec.Status == 0 {
		spec.Status = http.StatusOK
	}
	responseBody := []byte(spec.Body)
	if spec.BodyB64 != "" {
		decoded, decodeErr := base64.StdEncoding.DecodeString(spec.BodyB64)
		if decodeErr != nil {
			return nil, decodeErr
		}
		responseBody = decoded
	}
	contentLength := int64(len(responseBody))
	if spec.Oversize {
		contentLength = 1 << 62
	}
	return &http.Response{
		StatusCode:    spec.Status,
		Status:        fmt.Sprintf("%d %s", spec.Status, http.StatusText(spec.Status)),
		Header:        header,
		Body:          io.NopCloser(bytes.NewReader(responseBody)),
		ContentLength: contentLength,
		Request:       request,
	}, nil
}

func normalizeUserAgent(value string) string {
	if !strings.HasPrefix(value, "AzaharDesktop/") {
		return value
	}
	if start := strings.Index(value, " ("); start >= 0 && strings.HasSuffix(value, ")") {
		return value[:start] + " (device)"
	}
	return value
}

type secretSnapshot struct {
	Value   string `json:"value"`
	Present bool   `json:"present"`
	Error   string `json:"error,omitempty"`
}

type fileSnapshot struct {
	Mode  int    `json:"mode"`
	Bytes string `json:"bytes"`
	Dir   bool   `json:"dir,omitempty"`
}

type profileSnapshot struct {
	ProfileMode int            `json:"profile_mode"`
	Settings    *fileSnapshot  `json:"settings"`
	Sessions    []fileSnapshot `json:"sessions"`
	Leftovers   []string       `json:"leftovers"`
}

type pathSnapshot struct {
	Exists bool   `json:"exists"`
	Dir    bool   `json:"dir"`
	Mode   int    `json:"mode"`
	Bytes  string `json:"bytes"`
}

func snapshotPath(path string) pathSnapshot {
	info, err := os.Stat(path)
	if err != nil {
		return pathSnapshot{}
	}
	if info.IsDir() {
		return pathSnapshot{Exists: true, Dir: true, Mode: int(info.Mode().Perm())}
	}
	data, _ := os.ReadFile(path)
	return pathSnapshot{Exists: true, Mode: int(info.Mode().Perm()), Bytes: string(data)}
}

func snapshotProfile(profile string) profileSnapshot {
	snapshot := profileSnapshot{Settings: nil, Sessions: []fileSnapshot{}, Leftovers: []string{}}
	if info, err := os.Stat(profile); err == nil {
		snapshot.ProfileMode = int(info.Mode().Perm())
	}
	entries, err := os.ReadDir(profile)
	if err != nil {
		return snapshot
	}
	for _, entry := range entries {
		name := entry.Name()
		if name == "settings.json" {
			info, _ := entry.Info()
			snapshot.Settings = &fileSnapshot{}
			if info != nil {
				snapshot.Settings.Mode = int(info.Mode().Perm())
			}
			if entry.IsDir() {
				snapshot.Settings.Dir = true
				continue
			}
			data, _ := os.ReadFile(filepath.Join(profile, name))
			snapshot.Settings.Bytes = string(data)
			continue
		}
		if !entry.IsDir() && strings.HasPrefix(name, "session-") && strings.HasSuffix(name, ".json") {
			data, _ := os.ReadFile(filepath.Join(profile, name))
			info, _ := entry.Info()
			mode := 0
			if info != nil {
				mode = int(info.Mode().Perm())
			}
			snapshot.Sessions = append(snapshot.Sessions, fileSnapshot{Mode: mode, Bytes: string(data)})
			continue
		}
		if strings.HasPrefix(name, ".settings.json.") {
			snapshot.Leftovers = append(snapshot.Leftovers, "temp")
		} else {
			snapshot.Leftovers = append(snapshot.Leftovers, "other")
		}
	}
	sort.SliceStable(snapshot.Sessions, func(i, j int) bool {
		if snapshot.Sessions[i].Bytes != snapshot.Sessions[j].Bytes {
			return snapshot.Sessions[i].Bytes < snapshot.Sessions[j].Bytes
		}
		return snapshot.Sessions[i].Mode < snapshot.Sessions[j].Mode
	})
	sort.Strings(snapshot.Leftovers)
	return snapshot
}

// isolatedEnv keeps ambient user configuration and Azahar variables out of a
// script. HOME and XDG_CONFIG_HOME point inside the script's temporary root.
func isolatedEnv(root string) map[string]*string {
	home := filepath.Join(root, "home")
	config := filepath.Join(root, "config")
	_ = os.MkdirAll(home, 0o700)
	_ = os.MkdirAll(config, 0o700)
	return map[string]*string{
		"HOME":                   &home,
		"XDG_CONFIG_HOME":        &config,
		"AZAHAR_API_URL":         nil,
		"AZAHAR_ORIGIN":          nil,
		"AZAHAR_SESSION_BACKEND": nil,
		"AZAHAR_DISABLE_KEYRING": nil,
	}
}

func applyEnv(values map[string]*string) func() {
	type savedValue struct {
		value   string
		present bool
	}
	saved := make(map[string]savedValue, len(values))
	for name, value := range values {
		old, ok := os.LookupEnv(name)
		saved[name] = savedValue{value: old, present: ok}
		if value == nil {
			_ = os.Unsetenv(name)
		} else {
			_ = os.Setenv(name, *value)
		}
	}
	return func() {
		for name, old := range saved {
			if old.present {
				_ = os.Setenv(name, old.value)
			} else {
				_ = os.Unsetenv(name)
			}
		}
	}
}

// prepareLegacy writes an old Electron settings file and returns the
// environment that makes os.UserConfigDir find it.
func prepareLegacy(root, kind string, raw string) map[string]*string {
	env := map[string]*string{}
	write := func(dir string) {
		target := filepath.Join(dir, "Azahar Admin")
		_ = os.MkdirAll(target, 0o700)
		_ = os.WriteFile(filepath.Join(target, "settings.json"), []byte(raw), 0o600)
	}
	home := filepath.Join(root, "home")
	switch kind {
	case "", "xdg":
		config := filepath.Join(root, "config")
		write(config)
		env["XDG_CONFIG_HOME"] = &config
		env["HOME"] = &home
	case "home":
		config := filepath.Join(home, ".config")
		write(config)
		env["XDG_CONFIG_HOME"] = nil
		env["HOME"] = &home
	case "xdg-relative":
		relative := "relative/config"
		env["XDG_CONFIG_HOME"] = &relative
		env["HOME"] = &home
	case "none":
		env["XDG_CONFIG_HOME"] = nil
		env["HOME"] = nil
	default:
		config := filepath.Join(root, "config")
		write(config)
		env["XDG_CONFIG_HOME"] = &config
		env["HOME"] = &home
	}
	return env
}

type scriptMemory struct {
	mu         sync.Mutex
	value      string
	present    bool
	fail       bool
	deleteFail bool
	sleep      time.Duration
}

func (s *scriptMemory) Load() (string, bool, error) {
	if s.sleep > 0 {
		time.Sleep(s.sleep)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return "", false, errors.New("keyring unavailable")
	}
	return s.value, s.present, nil
}

func (s *scriptMemory) Save(value string) error {
	if s.sleep > 0 {
		time.Sleep(s.sleep)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return errors.New("keyring unavailable")
	}
	s.value = value
	s.present = true
	return nil
}

func (s *scriptMemory) Delete() error {
	if s.sleep > 0 {
		time.Sleep(s.sleep)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return errors.New("keyring unavailable")
	}
	if s.deleteFail {
		return errors.New("delete failed")
	}
	s.value = ""
	s.present = false
	return nil
}

func (s *scriptMemory) snapshot() secretSnapshot {
	s.mu.Lock()
	defer s.mu.Unlock()
	return secretSnapshot{Value: s.value, Present: s.present}
}

type stepResult struct {
	OK    bool       `json:"ok"`
	Value any        `json:"value"`
	Error *api.Error `json:"error,omitempty"`
	Plain string     `json:"plain,omitempty"`
}

func stepOK(value any) stepResult {
	return stepResult{OK: true, Value: value}
}

func stepAPIError(err *api.Error) stepResult {
	return stepResult{OK: false, Value: nil, Error: err}
}

func stepPlain(err error) stepResult {
	return stepResult{OK: false, Value: nil, Plain: err.Error()}
}

func stepPlainError(err *api.Error) stepResult {
	return stepResult{OK: false, Value: nil, Plain: err.Message}
}

func storeSnapshot(store api.OracleSecretStore) secretSnapshot {
	if store == nil {
		return secretSnapshot{}
	}
	value, present, err := store.Load()
	if err != nil {
		return secretSnapshot{Error: err.Error()}
	}
	return secretSnapshot{Value: value, Present: present}
}

func runClientScript(script *clientScript) result {
	root, err := os.MkdirTemp("", "oracle-client-")
	if err != nil {
		return stepPlain(err).asResult()
	}
	defer os.RemoveAll(root)
	profile := filepath.Join(root, "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		return stepPlain(err).asResult()
	}
	if script.SettingsRaw != nil {
		if err := os.WriteFile(filepath.Join(profile, "settings.json"), []byte(*script.SettingsRaw), 0o644); err != nil {
			return stepPlain(err).asResult()
		}
	}
	if script.SettingsDir {
		if err := os.MkdirAll(filepath.Join(profile, "settings.json"), 0o700); err != nil {
			return stepPlain(err).asResult()
		}
	}
	if script.SecretRaw != nil {
		if err := os.WriteFile(filepath.Join(profile, "session.json"), []byte(*script.SecretRaw), 0o644); err != nil {
			return stepPlain(err).asResult()
		}
	}

	env := isolatedEnv(root)
	env["AZAHAR_API_URL"] = script.Env.APIURL
	env["AZAHAR_ORIGIN"] = script.Env.Origin
	env["AZAHAR_SESSION_BACKEND"] = script.Env.SessionBackend
	env["AZAHAR_DISABLE_KEYRING"] = script.Env.DisableKeyring
	if script.LegacyRaw != nil {
		for name, value := range prepareLegacy(root, script.LegacyKind, *script.LegacyRaw) {
			env[name] = value
		}
	}
	restore := applyEnv(env)

	version := script.Version
	if version == "" {
		version = "2.0.0-test"
	}

	var client *api.Client
	var store api.OracleSecretStore
	factories := map[string]*scriptMemory{}
	var factoryMu sync.Mutex
	factory := func(apiURL string) api.OracleSecretStore {
		factoryMu.Lock()
		defer factoryMu.Unlock()
		if factories[apiURL] == nil {
			factories[apiURL] = &scriptMemory{}
		}
		return factories[apiURL]
	}

	switch script.Secrets {
	case "file":
		store = api.OracleFileStore(filepath.Join(profile, "session.json"))
		client, err = api.OracleNewClientLoaded(version, profile, store)
	case "failing":
		store = &scriptMemory{fail: true}
		client, err = api.OracleNewClientLoaded(version, profile, store)
	case "delete-failing":
		store = &scriptMemory{deleteFail: true}
		client, err = api.OracleNewClientLoaded(version, profile, store)
	case "fallback":
		store = api.OracleFallbackStore(
			&scriptMemory{fail: true},
			api.OracleFileStore(filepath.Join(profile, "session.json")),
		)
		client, err = api.OracleNewClientLoaded(version, profile, store)
	case "factory":
		client, err = api.OracleNewClientWithStoreFactory(version, profile, factory)
	case "factory-none":
		client, err = api.OracleNewClientWithStoreFactory(version, profile, func(string) api.OracleSecretStore {
			return nil
		})
	default:
		memory := &scriptMemory{}
		store = memory
		client, err = api.OracleNewClientLoaded(version, profile, store)
	}
	restore()
	if err != nil {
		return stepPlain(err).asResult()
	}

	if script.ImageCacheLimit != nil {
		api.OracleSetImageCacheLimit(client, *script.ImageCacheLimit)
	}
	transport := &scriptedTransport{
		mode:      script.Transport.Mode,
		responses: script.Transport.Responses,
	}
	api.OracleUseTransport(client, transport)

	results := make([]stepResult, 0, len(script.Steps))
	for _, step := range script.Steps {
		results = append(results, runScriptStep(client, store, step, factories))
	}

	current := api.OracleClientStateOf(client)
	entries, order, bytes := api.OracleImageCache(client)
	if order == nil {
		order = []string{}
	}
	factoryState := map[string]secretSnapshot{}
	for apiURL, memory := range factories {
		factoryState[apiURL] = memory.snapshot()
	}
	transport.mu.Lock()
	requests := append([]observedRequest{}, transport.requests...)
	transport.mu.Unlock()

	state := map[string]any{
		"client": current,
		"image": map[string]any{
			"entries": entries,
			"order":   order,
			"bytes":   bytes,
		},
		"secret":  storeSnapshot(store),
		"factory": factoryState,
	}
	return wrap(map[string]any{
		"steps":    results,
		"requests": requests,
		"state":    state,
		"profile":  snapshotProfile(profile),
	}, nil)
}

func (r stepResult) asResult() result {
	if r.Error != nil {
		return result{OK: false, Error: r.Error}
	}
	return result{OK: false, Error: &api.Error{
		Code:    "oracle_plain",
		Message: r.Plain,
		Fields:  map[string]string{},
	}}
}

func runScriptStep(client *api.Client, store api.OracleSecretStore, step scriptStep, factories map[string]*scriptMemory) stepResult {
	generation := step.Generation
	if step.UseCurrentGeneration {
		generation = api.OracleClientStateOf(client).Generation
	}
	switch step.Kind {
	case "info":
		return stepOK(client.Info())
	case "health":
		return stepOK(client.Health())
	case "ensure_origin":
		if err := api.OracleEnsureOrigin(client); err != nil {
			return stepAPIError(err)
		}
		return stepOK(nil)
	case "ensure_csrf":
		if err := api.OracleEnsureCSRF(client); err != nil {
			return stepAPIError(err)
		}
		return stepOK(nil)
	case "resolve_origin":
		return stepOK(api.OracleResolveOrigin(client, step.Force))
	case "window_bounds":
		return stepOK(client.WindowBounds())
	case "save_window_bounds":
		if step.Bounds == nil {
			return stepPlain(errors.New("bounds are required"))
		}
		client.SaveWindowBounds(*step.Bounds)
		return stepOK(nil)
	case "update_settings":
		settings, updateErr := client.UpdateSettings(step.APIURL, step.Origin, step.AllowNetwork)
		if updateErr != nil {
			var apiErr *api.Error
			if errors.As(updateErr, &apiErr) {
				return stepAPIError(apiErr)
			}
			return stepPlain(updateErr)
		}
		return stepOK(settings)
	case "settings_snapshot":
		return stepOK(api.OracleSettingsSnapshot(client))
	case "settings_consent_allowed":
		return stepOK(api.OracleSettingsConsentAllowed(api.OracleSettingsSnapshot(client)))
	case "seed_session":
		api.OracleSeedSession(client, step.Value, step.ExpiresUnix)
		return stepOK(nil)
	case "seed_csrf":
		api.OracleSeedCSRF(client, step.Value, step.ExpiresUnix)
		return stepOK(nil)
	case "active_csrf":
		return stepOK(api.OracleActiveCSRF(client))
	case "store_csrf":
		api.OracleStoreCSRF(client, step.Token, generation)
		return stepOK(nil)
	case "clear_csrf":
		api.OracleClearCSRF(client)
		return stepOK(nil)
	case "clear_session":
		if err := client.ClearSession(); err != nil {
			return stepPlain(err)
		}
		return stepOK(nil)
	case "persist_session":
		api.OraclePersistSession(client, step.Value)
		return stepOK(nil)
	case "delete_stored_session":
		if err := api.OracleDeleteStoredSession(client); err != nil {
			return stepPlain(err)
		}
		return stepOK(nil)
	case "load_session":
		api.OracleLoadSession(client)
		return stepOK(nil)
	case "absorb_cookies":
		api.OracleAbsorbCookies(client, step.SetCookies, generation)
		return stepOK(nil)
	case "session_state":
		state := api.OracleClientStateOf(client)
		return stepOK(map[string]any{"session": state.Session, "csrf": state.CSRF})
	case "request":
		var body any
		if step.BodyPresent {
			if err := json.Unmarshal(step.Body, &body); err != nil {
				return stepPlain(err)
			}
		}
		return stepOK(client.Request(step.Method, step.Path, body))
	case "image":
		return stepOK(client.Image(step.Path))
	case "store_image":
		api.OracleStoreImage(client, step.Key, step.Value, generation)
		return stepOK(nil)
	case "image_cache":
		entries, order, bytes := api.OracleImageCache(client)
		if order == nil {
			order = []string{}
		}
		return stepOK(map[string]any{"entries": entries, "order": order, "bytes": bytes})
	case "clear_image_cache":
		api.OracleClearImageCache(client)
		return stepOK(nil)
	case "secret_state":
		return stepOK(storeSnapshot(store))
	case "factory_state":
		state := map[string]secretSnapshot{}
		for apiURL, memory := range factories {
			state[apiURL] = memory.snapshot()
		}
		return stepOK(state)
	default:
		return stepPlain(fmt.Errorf("unknown step %q", step.Kind))
	}
}

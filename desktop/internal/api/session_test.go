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
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestCookieScopingAndDeletion(t *testing.T) {
	var mu sync.Mutex
	calls := 0
	cookies := map[int]string{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		calls++
		call := calls
		cookies[call] = r.Header.Get("Cookie")
		mu.Unlock()

		switch call {
		case 1:
			http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "session-one", Path: "/", HttpOnly: true})
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "csrf-one", Path: "/"})
			http.SetCookie(w, &http.Cookie{Name: "other", Value: "not-ours", Path: "/"})
			http.SetCookie(w, &http.Cookie{Name: "azahar_extra", Value: "not-ours", Path: "/"})
		case 3:
			http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "", MaxAge: -1, Path: "/"})
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "", MaxAge: -1, Path: "/"})
		}
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, secrets := testClient(t, server.URL)
	for i := 0; i < 4; i++ {
		if response := c.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
			t.Fatalf("request %d failed: %#v", i+1, response.Error)
		}
	}

	mu.Lock()
	defer mu.Unlock()
	if cookies[1] != "" {
		t.Fatalf("first request cookie = %q, want none", cookies[1])
	}
	if want := "azahar_session=session-one; azahar_csrf=csrf-one"; cookies[2] != want {
		t.Fatalf("second request cookie = %q, want %q", cookies[2], want)
	}
	if cookies[4] != "" {
		t.Fatalf("cookie was sent after deletion: %q", cookies[4])
	}
	if value, ok := secrets.stored(); ok {
		t.Fatalf("session was persisted after deletion: %q", value)
	}

	secrets.mu.Lock()
	defer secrets.mu.Unlock()
	if secrets.value != "" || secrets.present {
		t.Fatalf("secret store still holds %q", secrets.value)
	}
}

func TestOnlySessionIsPersisted(t *testing.T) {
	var mu sync.Mutex
	cookies := []string{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		cookies = append(cookies, r.Header.Get("Cookie"))
		mu.Unlock()
		http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "session-one", Path: "/"})
		http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "csrf-one", Path: "/"})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	secrets := &memorySecrets{}
	first, _ := newTestClientWithSecrets(t, server.URL, secrets)
	if response := first.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("first request failed: %#v", response.Error)
	}
	if value, ok := secrets.stored(); !ok || value != "session-one" {
		t.Fatalf("stored secret = %q, present %v; want session-one", value, ok)
	}

	second, _ := newTestClientWithSecrets(t, server.URL, secrets)
	if response := second.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("second request failed: %#v", response.Error)
	}

	mu.Lock()
	defer mu.Unlock()
	if len(cookies) != 2 {
		t.Fatalf("cookie observations = %d", len(cookies))
	}
	if cookies[1] != "azahar_session=session-one" {
		t.Fatalf("restored cookie = %q, want only the session", cookies[1])
	}
}

func TestExpiredCookieIsNotStored(t *testing.T) {
	var mu sync.Mutex
	var cookies []string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		cookies = append(cookies, r.Header.Get("Cookie"))
		mu.Unlock()
		http.SetCookie(w, &http.Cookie{
			Name:    sessionCookieName,
			Value:   "stale-session",
			Path:    "/",
			Expires: time.Now().Add(-time.Hour),
		})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, secrets := testClient(t, server.URL)
	if response := c.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("first request failed: %#v", response.Error)
	}
	if response := c.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("second request failed: %#v", response.Error)
	}
	mu.Lock()
	defer mu.Unlock()
	if cookies[1] != "" {
		t.Fatalf("expired cookie was sent: %q", cookies[1])
	}
	if value, ok := secrets.stored(); ok {
		t.Fatalf("expired cookie was persisted: %q", value)
	}
}

func TestSessionFileModeAndContents(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "session-file", Path: "/"})
		http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "csrf-memory-only", Path: "/"})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	profile := filepath.Join(t.TempDir(), "profile")
	sessionPath := filepath.Join(profile, "session.json")
	store := &fileSecretStore{path: sessionPath}
	t.Setenv("AZAHAR_API_URL", server.URL)
	t.Setenv("AZAHAR_ORIGIN", "")
	c, err := newClientWithSecrets("2.0.0-test", profile, store)
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if response := c.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}

	info, err := os.Stat(sessionPath)
	if err != nil {
		t.Fatalf("session file: %v", err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("session mode = %o, want 600", info.Mode().Perm())
	}
	data, err := os.ReadFile(sessionPath)
	if err != nil {
		t.Fatalf("read session file: %v", err)
	}
	var record map[string]string
	if err := json.Unmarshal(data, &record); err != nil {
		t.Fatalf("decode session file: %v", err)
	}
	if len(record) != 1 || record[sessionCookieName] != "session-file" {
		t.Fatalf("session file = %#v, want only azahar_session", record)
	}
}

func TestKeyringFailureFallsBackToFile(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "fallback-session", Path: "/"})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	profile := filepath.Join(t.TempDir(), "profile")
	sessionPath := filepath.Join(profile, "session.json")
	store := &fallbackSecretStore{
		primary:  &memorySecrets{fail: true},
		fallback: &fileSecretStore{path: sessionPath},
	}
	t.Setenv("AZAHAR_API_URL", server.URL)
	t.Setenv("AZAHAR_ORIGIN", "")
	c, err := newClientWithSecrets("2.0.0-test", profile, store)
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if response := c.Request(http.MethodGet, "/api/cookies", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}

	info, err := os.Stat(sessionPath)
	if err != nil {
		t.Fatalf("fallback session file: %v", err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("fallback session mode = %o, want 600", info.Mode().Perm())
	}

	second, err := newClientWithSecrets("2.0.0-test", profile, store)
	if err != nil {
		t.Fatalf("second client: %v", err)
	}
	if value, ok, err := store.Load(); err != nil || !ok || value != "fallback-session" {
		t.Fatalf("fallback load = %q, %v, %v", value, ok, err)
	}
	if err := second.ClearSession(); err == nil {
		t.Fatal("clear session hid the keyring delete failure")
	}
	if _, err := os.Stat(sessionPath); !os.IsNotExist(err) {
		t.Fatalf("fallback file still exists after clear: %v", err)
	}
}

func TestClearSession(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "session-one", Path: "/"})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, secrets := testClient(t, server.URL)
	if response := c.Request(http.MethodGet, "/api/session", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if _, ok := secrets.stored(); !ok {
		t.Fatal("session was not saved")
	}
	if err := c.ClearSession(); err != nil {
		t.Fatalf("clear session: %v", err)
	}
	if value, ok := secrets.stored(); ok {
		t.Fatalf("session still saved after clear: %q", value)
	}
}

func TestUpdateSettingsClearsSession(t *testing.T) {
	handler := func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/health" {
			writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://localhost:3000"})
			return
		}
		http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: "old-session", Path: "/"})
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}
	first := httptest.NewServer(http.HandlerFunc(handler))
	defer first.Close()
	second := httptest.NewServer(http.HandlerFunc(handler))
	defer second.Close()

	c, secrets := testClient(t, first.URL)
	if response := c.Request(http.MethodGet, "/api/session", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if _, ok := secrets.stored(); !ok {
		t.Fatal("session was not saved")
	}

	result, err := c.UpdateSettings(second.URL, "", false)
	if err != nil {
		t.Fatalf("update settings: %v", err)
	}
	if result.APIURL != second.URL {
		t.Fatalf("APIURL = %q, want %q", result.APIURL, second.URL)
	}
	if value, ok := secrets.stored(); ok {
		t.Fatalf("session survived server change: %q", value)
	}
}

type blockingSecretStore struct {
	release <-chan struct{}
}

func (s blockingSecretStore) Load() (string, bool, error) {
	<-s.release
	return "", false, fakeError("released")
}

func (s blockingSecretStore) Save(string) error {
	<-s.release
	return fakeError("released")
}

func (s blockingSecretStore) Delete() error {
	<-s.release
	return fakeError("released")
}

func TestTimedSecretStoreTimesOut(t *testing.T) {
	release := make(chan struct{})
	defer close(release)
	store := &timeoutSecretStore{
		store:   blockingSecretStore{release: release},
		timeout: 20 * time.Millisecond,
	}

	start := time.Now()
	if _, ok, err := store.Load(); !errors.Is(err, errSecretTimeout) || ok {
		t.Fatalf("Load = %v, %v", ok, err)
	}
	if err := store.Save("value"); !errors.Is(err, errSecretTimeout) {
		t.Fatalf("Save = %v", err)
	}
	if err := store.Delete(); !errors.Is(err, errSecretTimeout) {
		t.Fatalf("Delete = %v", err)
	}
	if elapsed := time.Since(start); elapsed > time.Second {
		t.Fatalf("timeouts took %v", elapsed)
	}
}

func TestKeyringTimeoutFallsBackToFile(t *testing.T) {
	release := make(chan struct{})
	defer close(release)
	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}
	sessionPath := filepath.Join(profile, "session-test.json")
	store := &fallbackSecretStore{
		primary: &timeoutSecretStore{
			store:   blockingSecretStore{release: release},
			timeout: 20 * time.Millisecond,
		},
		fallback: &fileSecretStore{path: sessionPath},
	}

	start := time.Now()
	if _, err := newClientWithSecrets("2.0.0-test", profile, store); err != nil {
		t.Fatalf("new client: %v", err)
	}
	if elapsed := time.Since(start); elapsed > time.Second {
		t.Fatalf("startup took %v", elapsed)
	}

	if err := store.Save("timeout-session"); err != nil {
		t.Fatalf("fallback save: %v", err)
	}
	info, err := os.Stat(sessionPath)
	if err != nil {
		t.Fatalf("session file: %v", err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("session mode = %o, want 600", info.Mode().Perm())
	}

	reloaded, err := newClientWithSecrets("2.0.0-test", profile, store)
	if err != nil {
		t.Fatalf("reload: %v", err)
	}
	reloaded.mu.RLock()
	loaded := reloaded.session.value
	reloaded.mu.RUnlock()
	if loaded != "timeout-session" {
		t.Fatalf("loaded session = %q", loaded)
	}

	if err := reloaded.ClearSession(); !errors.Is(err, errSecretTimeout) {
		t.Fatalf("clear session error = %v, want the keyring timeout", err)
	}
	if _, err := os.Stat(sessionPath); !os.IsNotExist(err) {
		t.Fatalf("session file still exists after clear: %v", err)
	}
}

type deleteFailSecretStore struct {
	memorySecrets
}

func (s *deleteFailSecretStore) Delete() error {
	return fakeError("delete failed")
}

func TestClearSessionReturnsDeleteError(t *testing.T) {
	c, err := newClientWithSecrets("2.0.0-test", filepath.Join(t.TempDir(), "profile"), &deleteFailSecretStore{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	c.mu.Lock()
	c.session = storedCookie{value: "session-one"}
	c.csrf = storedCookie{value: "csrf-one"}
	c.mu.Unlock()

	err = c.ClearSession()
	if err == nil || !strings.Contains(err.Error(), "delete failed") {
		t.Fatalf("clear session error = %v", err)
	}
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.session.value != "" || c.csrf.value != "" {
		t.Fatalf("session state survived clear: %#v %#v", c.session, c.csrf)
	}
}

func TestSessionScopeIsProfileAndServerSpecific(t *testing.T) {
	profileA := filepath.Join(t.TempDir(), "profile-a")
	profileB := filepath.Join(t.TempDir(), "profile-b")
	urlA := "https://a.example.com"
	urlB := "https://b.example.com"

	scopeA := sessionScope(profileA, urlA)
	scopeB := sessionScope(profileA, urlB)
	scopeProfileB := sessionScope(profileB, urlA)
	if scopeA == scopeB || scopeA == scopeProfileB || scopeB == scopeProfileB {
		t.Fatalf("scopes collide: %q %q %q", scopeA, scopeB, scopeProfileB)
	}
	if !strings.HasPrefix(scopeA, "session-") || len(scopeA) != len("session-")+16 {
		t.Fatalf("scope = %q", scopeA)
	}
	if sessionScope(profileA, urlA+"/") != scopeA {
		t.Fatal("scope is not stable across trailing slashes")
	}

	t.Setenv("AZAHAR_DISABLE_KEYRING", "1")
	t.Setenv("AZAHAR_SESSION_BACKEND", "")
	fileA, ok := defaultSecretStore(profileA, urlA).(*fileSecretStore)
	if !ok {
		t.Fatalf("default store = %T, want file", defaultSecretStore(profileA, urlA))
	}
	if filepath.Base(fileA.path) != scopeA+".json" {
		t.Fatalf("file = %q, want %s.json", fileA.path, scopeA)
	}
	fileB, ok := defaultSecretStore(profileA, urlB).(*fileSecretStore)
	if !ok || fileB.path == fileA.path {
		t.Fatalf("server scopes share a file: %q", fileB.path)
	}
	fileProfileB, ok := defaultSecretStore(profileB, urlA).(*fileSecretStore)
	if !ok || fileProfileB.path == fileA.path {
		t.Fatalf("profile scopes share a file: %q", fileProfileB.path)
	}

	t.Setenv("AZAHAR_DISABLE_KEYRING", "0")
	store := defaultSecretStore(profileA, urlA)
	fallback, ok := store.(*fallbackSecretStore)
	if !ok {
		t.Fatalf("default store = %T, want fallback", store)
	}
	timed, ok := fallback.primary.(*timeoutSecretStore)
	if !ok {
		t.Fatalf("primary = %T, want timeout store", fallback.primary)
	}
	keyring, ok := timed.store.(keyringSecretStore)
	if !ok || keyring.account != scopeA {
		t.Fatalf("keyring account = %q, want %q", keyring.account, scopeA)
	}
}

func TestSessionStoresAreScopedByServer(t *testing.T) {
	var mu sync.Mutex
	stores := map[string]*memorySecrets{}
	factory := func(apiURL string) secretStore {
		mu.Lock()
		defer mu.Unlock()
		if stores[apiURL] == nil {
			stores[apiURL] = &memorySecrets{}
		}
		return stores[apiURL]
	}
	captured := map[string][]string{}
	newServer := func(name, session string) *httptest.Server {
		return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path == "/api/health" {
				writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://localhost:3000"})
				return
			}
			mu.Lock()
			captured[name] = append(captured[name], r.Header.Get("Cookie"))
			mu.Unlock()
			http.SetCookie(w, &http.Cookie{Name: sessionCookieName, Value: session, Path: "/"})
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}))
	}
	serverA := newServer("a", "session-a")
	defer serverA.Close()
	serverB := newServer("b", "session-b")
	defer serverB.Close()

	profile := filepath.Join(t.TempDir(), "profile")
	t.Setenv("AZAHAR_ORIGIN", "")
	t.Setenv("AZAHAR_API_URL", serverA.URL)
	clientA, err := newClientWithStoreFactory("2.0.0-test", profile, factory)
	if err != nil {
		t.Fatalf("client A: %v", err)
	}
	if response := clientA.Request(http.MethodGet, "/api/session", nil); !response.OK {
		t.Fatalf("client A request: %#v", response.Error)
	}

	t.Setenv("AZAHAR_API_URL", serverB.URL)
	clientB, err := newClientWithStoreFactory("2.0.0-test", profile, factory)
	if err != nil {
		t.Fatalf("client B: %v", err)
	}
	if response := clientB.Request(http.MethodGet, "/api/session", nil); !response.OK {
		t.Fatalf("client B request: %#v", response.Error)
	}

	mu.Lock()
	storeA := stores[serverA.URL]
	storeB := stores[serverB.URL]
	lastBCookie := ""
	if list := captured["b"]; len(list) > 0 {
		lastBCookie = list[len(list)-1]
	}
	mu.Unlock()
	if value, ok := storeA.stored(); !ok || value != "session-a" {
		t.Fatalf("store A = %q, %v", value, ok)
	}
	if lastBCookie != "" {
		t.Fatalf("client B sent another server's session: %q", lastBCookie)
	}
	if value, ok := storeB.stored(); !ok || value != "session-b" {
		t.Fatalf("store B = %q, %v", value, ok)
	}

	if _, err := clientA.UpdateSettings(serverB.URL, "", false); err != nil {
		t.Fatalf("switch to B: %v", err)
	}
	if value, ok := storeA.stored(); ok {
		t.Fatalf("old server session survived switch: %q", value)
	}
	if value, ok := storeB.stored(); !ok || value != "session-b" {
		t.Fatalf("new server store changed on switch: %q, %v", value, ok)
	}

	mu.Lock()
	captured["b"] = nil
	mu.Unlock()
	if response := clientA.Request(http.MethodGet, "/api/session", nil); !response.OK {
		t.Fatalf("request after switch: %#v", response.Error)
	}
	mu.Lock()
	afterSwitchCookie := ""
	if list := captured["b"]; len(list) > 0 {
		afterSwitchCookie = list[len(list)-1]
	}
	mu.Unlock()
	if afterSwitchCookie != "" {
		t.Fatalf("switch loaded another server's session: %q", afterSwitchCookie)
	}

	if err := storeA.Save("other-session"); err != nil {
		t.Fatal(err)
	}
	if err := clientA.ClearSession(); err != nil {
		t.Fatalf("clear session: %v", err)
	}
	if _, ok := storeB.stored(); ok {
		t.Fatal("clear session did not delete the current server store")
	}
	if value, ok := storeA.stored(); !ok || value != "other-session" {
		t.Fatalf("clear session touched another server: %q, %v", value, ok)
	}
}

func TestFallbackSecretStoreLoadsFallbackOnPrimaryMiss(t *testing.T) {
	file := &fileSecretStore{path: filepath.Join(t.TempDir(), "session.json")}
	if err := file.Save("fallback-session"); err != nil {
		t.Fatalf("save fallback: %v", err)
	}
	store := &fallbackSecretStore{primary: &memorySecrets{}, fallback: file}
	value, ok, err := store.Load()
	if err != nil || !ok || value != "fallback-session" {
		t.Fatalf("load = %q, %v, %v; want fallback-session, true, nil", value, ok, err)
	}
}

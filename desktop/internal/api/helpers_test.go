package api

import (
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

type memorySecrets struct {
	mu      sync.Mutex
	value   string
	present bool
	fail    bool
}

func (s *memorySecrets) Load() (string, bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return "", false, errFakeKeyring
	}
	return s.value, s.present, nil
}

func (s *memorySecrets) Save(value string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return errFakeKeyring
	}
	s.value = value
	s.present = true
	return nil
}

func (s *memorySecrets) Delete() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.fail {
		return errFakeKeyring
	}
	s.value = ""
	s.present = false
	return nil
}

func (s *memorySecrets) stored() (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.value, s.present
}

type fakeError string

func (e fakeError) Error() string { return string(e) }

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

const errFakeKeyring = fakeError("keyring unavailable")

func testClient(t *testing.T, apiURL string) (*Client, *memorySecrets) {
	t.Helper()
	return newTestClientWithSecrets(t, apiURL, &memorySecrets{})
}

func newTestClientWithSecrets(t *testing.T, apiURL string, secrets secretStore) (*Client, *memorySecrets) {
	t.Helper()
	t.Setenv("AZAHAR_API_URL", apiURL)
	t.Setenv("AZAHAR_ORIGIN", "")
	client, err := newClientWithSecrets("2.0.0-test", filepath.Join(t.TempDir(), "profile"), secrets)
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	recorder, _ := secrets.(*memorySecrets)
	return client, recorder
}

func unsetEnv(t *testing.T, key string) {
	t.Helper()
	value, ok := os.LookupEnv(key)
	if err := os.Unsetenv(key); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if ok {
			_ = os.Setenv(key, value)
		} else {
			_ = os.Unsetenv(key)
		}
	})
}

func seedCSRF(c *Client, token string) {
	c.mu.Lock()
	c.csrf = storedCookie{value: token}
	c.mu.Unlock()
}

func writeJSON(t *testing.T, w http.ResponseWriter, status int, value any) {
	t.Helper()
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func writeProblem(t *testing.T, w http.ResponseWriter, status int, code, message string) {
	t.Helper()
	writeJSON(t, w, status, map[string]any{
		"error": map[string]any{"code": code, "message": message, "fields": map[string]string{}},
	})
}

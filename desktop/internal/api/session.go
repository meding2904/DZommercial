package api

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/zalando/go-keyring"
)

const (
	sessionCookieName = "azahar_session"
	csrfCookieName    = "azahar_csrf"
	keyringService    = "azahar-admin"
	keyringTimeout    = 1500 * time.Millisecond
)

var errSecretTimeout = errors.New("secret store timed out")

type secretStore interface {
	Load() (string, bool, error)
	Save(string) error
	Delete() error
}

type timeoutSecretStore struct {
	store   secretStore
	timeout time.Duration
}

type secretLoadResult struct {
	value string
	ok    bool
	err   error
}

func (s *timeoutSecretStore) Load() (string, bool, error) {
	if s.store == nil {
		return "", false, errors.New("secret store is not configured")
	}
	result := make(chan secretLoadResult, 1)
	go func() {
		value, ok, err := s.store.Load()
		result <- secretLoadResult{value: value, ok: ok, err: err}
	}()
	timer := time.NewTimer(s.timeout)
	defer timer.Stop()
	select {
	case loaded := <-result:
		return loaded.value, loaded.ok, loaded.err
	case <-timer.C:
		return "", false, errSecretTimeout
	}
}

func (s *timeoutSecretStore) Save(value string) error {
	if s.store == nil {
		return errors.New("secret store is not configured")
	}
	result := make(chan error, 1)
	go func() {
		result <- s.store.Save(value)
	}()
	timer := time.NewTimer(s.timeout)
	defer timer.Stop()
	select {
	case err := <-result:
		return err
	case <-timer.C:
		return errSecretTimeout
	}
}

func (s *timeoutSecretStore) Delete() error {
	if s.store == nil {
		return errors.New("secret store is not configured")
	}
	result := make(chan error, 1)
	go func() {
		result <- s.store.Delete()
	}()
	timer := time.NewTimer(s.timeout)
	defer timer.Stop()
	select {
	case err := <-result:
		return err
	case <-timer.C:
		return errSecretTimeout
	}
}

func sessionScope(profileDir, apiURL string) string {
	if normalized, err := normalizeAPIURL(apiURL); err == nil {
		apiURL = normalized
	}
	sum := sha256.Sum256([]byte(filepath.Clean(profileDir) + "\n" + apiURL))
	return "session-" + hex.EncodeToString(sum[:])[:16]
}

type sessionFile struct {
	Session string `json:"azahar_session"`
}

type fileSecretStore struct {
	path string
}

func (s *fileSecretStore) Load() (string, bool, error) {
	data, err := os.ReadFile(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return "", false, nil
	}
	if err != nil {
		return "", false, err
	}
	_ = os.Chmod(s.path, 0o600)
	var record sessionFile
	if err := json.Unmarshal(data, &record); err != nil {
		return "", false, nil
	}
	if !validCookieValue(record.Session) {
		return "", false, nil
	}
	return record.Session, true, nil
}

func (s *fileSecretStore) Save(value string) error {
	data, err := json.Marshal(sessionFile{Session: value})
	if err != nil {
		return err
	}
	return writePrivateFile(s.path, data)
}

func (s *fileSecretStore) Delete() error {
	err := os.Remove(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

type keyringSecretStore struct {
	account string
}

func (s keyringSecretStore) Load() (string, bool, error) {
	value, err := keyring.Get(keyringService, s.account)
	if errors.Is(err, keyring.ErrNotFound) {
		return "", false, nil
	}
	if err != nil {
		return "", false, err
	}
	if value == "" {
		return "", false, nil
	}
	return value, true, nil
}

func (s keyringSecretStore) Save(value string) error {
	return keyring.Set(keyringService, s.account, value)
}

func (s keyringSecretStore) Delete() error {
	err := keyring.Delete(keyringService, s.account)
	if errors.Is(err, keyring.ErrNotFound) {
		return nil
	}
	return err
}

type fallbackSecretStore struct {
	primary  secretStore
	fallback secretStore
}

func (s *fallbackSecretStore) Load() (string, bool, error) {
	if s.primary != nil {
		if value, ok, err := s.primary.Load(); err == nil && ok {
			return value, true, nil
		}
	}
	return s.fallback.Load()
}

func (s *fallbackSecretStore) Save(value string) error {
	if s.primary != nil {
		if err := s.primary.Save(value); err == nil {
			_ = s.fallback.Delete()
			return nil
		}
	}
	return s.fallback.Save(value)
}

func (s *fallbackSecretStore) Delete() error {
	var primaryErr error
	if s.primary != nil {
		primaryErr = s.primary.Delete()
	}
	if err := s.fallback.Delete(); err != nil {
		return err
	}
	return primaryErr
}

func defaultSecretStore(profileDir string, apiURLs ...string) secretStore {
	apiURL := defaultAPIURL
	if len(apiURLs) > 0 {
		apiURL = apiURLs[0]
	}
	scope := sessionScope(profileDir, apiURL)
	file := &fileSecretStore{path: filepath.Join(profileDir, scope+".json")}
	if strings.EqualFold(strings.TrimSpace(os.Getenv("AZAHAR_SESSION_BACKEND")), "file") || envEnabled("AZAHAR_DISABLE_KEYRING") {
		return file
	}
	primary := &timeoutSecretStore{
		store:   keyringSecretStore{account: scope},
		timeout: keyringTimeout,
	}
	return &fallbackSecretStore{primary: primary, fallback: file}
}

func envEnabled(name string) bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv(name))) {
	case "", "0", "false", "no", "off":
		return false
	default:
		return true
	}
}

type storedCookie struct {
	value   string
	expires time.Time
}

func (c storedCookie) active(now time.Time) bool {
	return c.value != "" && (c.expires.IsZero() || now.Before(c.expires))
}

func cookieFromSetCookie(cookie *http.Cookie, now time.Time) (storedCookie, bool) {
	if cookie.Value == "" || cookie.MaxAge < 0 || !validCookieValue(cookie.Value) {
		return storedCookie{}, false
	}
	expires := cookie.Expires
	if cookie.MaxAge > 0 {
		expires = now.Add(time.Duration(cookie.MaxAge) * time.Second)
	}
	if !expires.IsZero() && !expires.After(now) {
		return storedCookie{}, false
	}
	return storedCookie{value: cookie.Value, expires: expires}, true
}

func validCookieValue(value string) bool {
	if value == "" || len(value) > 4096 {
		return false
	}
	for i := 0; i < len(value); i++ {
		b := value[i]
		if b < 0x21 || b > 0x7e || b == ';' || b == '"' || b == '\\' || b == ',' {
			return false
		}
	}
	return true
}

func (c *Client) loadSession() {
	if c.secrets == nil {
		return
	}
	value, ok, err := c.secrets.Load()
	if err != nil || !ok || !validCookieValue(value) {
		return
	}
	c.mu.Lock()
	c.session = storedCookie{value: value}
	c.mu.Unlock()
}

func (c *Client) absorbCookies(resp *http.Response, generation uint64) {
	now := time.Now()
	var sessionChanged bool
	var sessionValue string

	c.mu.Lock()
	if generation != c.generation {
		c.mu.Unlock()
		return
	}
	for _, cookie := range resp.Cookies() {
		switch cookie.Name {
		case sessionCookieName:
			next, ok := cookieFromSetCookie(cookie, now)
			if !ok {
				next = storedCookie{}
			}
			if c.session != next {
				c.session = next
				sessionChanged = true
			}
		case csrfCookieName:
			next, ok := cookieFromSetCookie(cookie, now)
			if !ok {
				next = storedCookie{}
			}
			c.csrf = next
		}
	}
	sessionValue = c.session.value
	c.mu.Unlock()

	if sessionChanged {
		c.persistSession(sessionValue)
	}
}

func (c *Client) persistSession(value string) {
	c.secretMu.Lock()
	defer c.secretMu.Unlock()

	c.mu.RLock()
	current := c.session.value
	store := c.secrets
	c.mu.RUnlock()
	if current != value || store == nil {
		return
	}
	if value == "" {
		_ = store.Delete()
		return
	}
	_ = store.Save(value)
}

func (c *Client) deleteStoredSession(store secretStore) error {
	if store == nil {
		return nil
	}
	c.secretMu.Lock()
	defer c.secretMu.Unlock()
	return store.Delete()
}

func (c *Client) activeCSRF() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.csrf.active(time.Now()) {
		return c.csrf.value
	}
	return ""
}

func (c *Client) storeCSRF(token string, generation uint64) {
	if !validCookieValue(token) {
		return
	}
	c.mu.Lock()
	if c.generation == generation && !c.csrf.active(time.Now()) {
		c.csrf = storedCookie{value: token}
	}
	c.mu.Unlock()
}

func (c *Client) clearCSRF() {
	c.mu.Lock()
	c.csrf = storedCookie{}
	c.mu.Unlock()
}

func (c *Client) ClearSession() error {
	c.mu.Lock()
	c.generation++
	c.session = storedCookie{}
	c.csrf = storedCookie{}
	c.clearImageCacheLocked()
	store := c.secrets
	c.mu.Unlock()
	return c.deleteStoredSession(store)
}

func writePrivateFile(path string, data []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	_ = os.Chmod(dir, 0o700)

	tmp, err := os.CreateTemp(dir, "."+filepath.Base(path)+".*")
	if err != nil {
		return err
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)

	if err := tmp.Chmod(0o600); err != nil {
		tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpName, path)
}

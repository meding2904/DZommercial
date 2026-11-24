//go:build oracle

package api

import (
	"encoding/json"
	"net/http"
	"net/url"
	"time"
)

// This file exposes the package's decisions to the differential oracle. It is
// built only with -tags oracle, so the desktop binary is unchanged.

func OracleNormalizeAPIURL(raw string) (string, *Error) {
	return normalizeAPIURL(raw)
}

func OracleNormalizeOrigin(raw string) (string, *Error) {
	return normalizeOrigin(raw)
}

func OracleValidateServerAddress(apiURL string, allowNetwork bool) *Error {
	return validateServerAddress(apiURL, allowNetwork)
}

func OracleConsentHostForUpdate(previous Settings, apiURL string, allowNetwork bool) (string, *Error) {
	return consentHostForUpdate(previous, apiURL, allowNetwork)
}

func OracleAPIHost(apiURL string) (string, *Error) {
	return apiHost(apiURL)
}

func OracleNormalizeHost(host string) string {
	return normalizeHost(host)
}

func OracleHostRules(host string) (bool, bool) {
	return isLoopbackHost(host), isLocalHost(host)
}

func OracleCanonicalOrigin(raw string) string {
	parsed, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	return canonicalOrigin(parsed)
}

func OracleValidateAPIPath(raw string) (string, string, bool, *Error) {
	parsed, err := validateAPIPath(raw)
	if err != nil {
		return "", "", false, err
	}
	return parsed.Path, parsed.RawQuery, isCSRFEndpoint(raw), nil
}

func OracleTargetURL(apiURL, allowedHost string, generation uint64, raw string) (string, uint64, *Error) {
	c := &Client{
		settings:   Settings{APIURL: apiURL, NetworkAllowedHost: allowedHost},
		generation: generation,
	}
	target, gotGeneration, err := c.targetURL(raw)
	if err != nil {
		return "", 0, err
	}
	return target.String(), gotGeneration, nil
}

func OracleRequestHeaders(userAgent, apiURL, origin, resolvedOrigin, allowedHost, session, csrf string, sessionExpires, csrfExpires, now time.Time, includeOrigin, includeCSRF, hasBody bool) http.Header {
	c := &Client{
		userAgent: userAgent,
		settings: Settings{
			APIURL:             apiURL,
			Origin:             origin,
			NetworkAllowedHost: allowedHost,
		},
		resolvedOrigin: resolvedOrigin,
		session:        storedCookie{value: session, expires: sessionExpires},
		csrf:           storedCookie{value: csrf, expires: csrfExpires},
	}
	return c.requestHeadersLocked(now, includeOrigin, includeCSRF, hasBody)
}

func OracleErrorFromResult(status int, body string) *Error {
	return errorFromResult(httpResult{status: status, body: []byte(body)})
}

func OracleErrorCode(body string) string {
	return errorCode([]byte(body))
}

func OracleDecodeJSON(body string) any {
	return decodeJSON([]byte(body))
}

func OracleEncodeRequestBody(method string, body any) (string, *Error) {
	payload, err := encodeRequestBody(method, body)
	if err != nil {
		return "", err
	}
	return string(payload), nil
}

func OracleCookieFromSetCookie(header string, now time.Time) (string, string, bool) {
	cookie, err := http.ParseSetCookie(header)
	if err != nil {
		return "", "", false
	}
	stored, ok := cookieFromSetCookie(cookie, now)
	if !ok {
		return "", "", false
	}
	expires := ""
	if !stored.expires.IsZero() {
		expires = stored.expires.UTC().Format(time.RFC3339Nano)
	}
	return stored.value, expires, true
}

func OracleValidCookieValue(value string) bool {
	return validCookieValue(value)
}

func OracleSessionScope(profileDir, apiURL string) string {
	return sessionScope(profileDir, apiURL)
}

func OracleDeviceHash(profileDir string) string {
	return deviceHash(profileDir)
}

func OracleFirewallGuide(platform string) FirewallGuide {
	return firewallGuide(platform)
}

// OracleSecretStore is the test-facing copy of secretStore.
type OracleSecretStore interface {
	Load() (string, bool, error)
	Save(string) error
	Delete() error
}

func OracleNewClient(version, profileDir, apiURL string, secrets OracleSecretStore) (*Client, error) {
	c, err := newClientWithSecrets(version, profileDir, secrets)
	if err != nil {
		return nil, err
	}
	c.mu.Lock()
	c.settings = Settings{APIURL: apiURL}
	c.resolvedOrigin = ""
	c.mu.Unlock()
	return c, nil
}

func OracleSessionState(c *Client) (string, string) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.session.value, c.csrf.value
}

func OracleUseTransport(c *Client, transport http.RoundTripper) {
	c.httpClient = &http.Client{
		Transport: transport,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
}

// --- Extended differential surface: settings, session, images, secrets. ---

// OracleClientState is the observable in-memory state of a Client.
type OracleClientState struct {
	Settings           Settings `json:"settings"`
	ResolvedOrigin     string   `json:"resolved_origin"`
	Generation         uint64   `json:"generation"`
	Session            string   `json:"session"`
	SessionExpiresUnix int64    `json:"session_expires_unix"`
	CSRF               string   `json:"csrf"`
	CSRFExpiresUnix    int64    `json:"csrf_expires_unix"`
	ActiveCSRF         string   `json:"active_csrf"`
}

func oracleExpiresUnix(value time.Time) int64 {
	if value.IsZero() {
		return 0
	}
	return value.Unix()
}

func OracleClientStateOf(c *Client) OracleClientState {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return OracleClientState{
		Settings:           c.settings,
		ResolvedOrigin:     c.resolvedOrigin,
		Generation:         c.generation,
		Session:            c.session.value,
		SessionExpiresUnix: oracleExpiresUnix(c.session.expires),
		CSRF:               c.csrf.value,
		CSRFExpiresUnix:    oracleExpiresUnix(c.csrf.expires),
		ActiveCSRF:         oracleActiveCSRF(c),
	}
}

func oracleActiveCSRF(c *Client) string {
	if c.csrf.active(time.Now()) {
		return c.csrf.value
	}
	return ""
}

func OracleSettingsConsentAllowed(settings Settings) bool {
	return settingsConsentAllowed(settings)
}

func OracleSeedSession(c *Client, value string, expiresUnix int64) {
	var expires time.Time
	if expiresUnix != 0 {
		expires = time.Unix(expiresUnix, 0).UTC()
	}
	c.mu.Lock()
	c.session = storedCookie{value: value, expires: expires}
	c.mu.Unlock()
}

func OracleSeedCSRF(c *Client, value string, expiresUnix int64) {
	var expires time.Time
	if expiresUnix != 0 {
		expires = time.Unix(expiresUnix, 0).UTC()
	}
	c.mu.Lock()
	c.csrf = storedCookie{value: value, expires: expires}
	c.mu.Unlock()
}

func OracleActiveCSRF(c *Client) string {
	return c.activeCSRF()
}

func OracleStoreCSRF(c *Client, token string, generation uint64) {
	c.storeCSRF(token, generation)
}

func OracleClearCSRF(c *Client) {
	c.clearCSRF()
}

func OraclePersistSession(c *Client, value string) {
	c.persistSession(value)
}

func OracleDeleteStoredSession(c *Client) error {
	c.mu.RLock()
	store := c.secrets
	c.mu.RUnlock()
	return c.deleteStoredSession(store)
}

func OracleLoadSession(c *Client) {
	c.loadSession()
}

func OracleAbsorbCookies(c *Client, setCookies []string, generation uint64) {
	response := &http.Response{Header: http.Header{}}
	for _, value := range setCookies {
		response.Header.Add("Set-Cookie", value)
	}
	c.absorbCookies(response, generation)
}

func OracleImageCache(c *Client) (map[string]string, []string, int64) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	entries := make(map[string]string, len(c.imageCache))
	for key, value := range c.imageCache {
		entries[key] = value
	}
	order := append([]string(nil), c.imageOrder...)
	return entries, order, c.imageCacheBytes
}

func OracleStoreImage(c *Client, key, value string, generation uint64) {
	c.storeImage(key, value, generation)
}

func OracleClearImageCache(c *Client) {
	c.mu.Lock()
	c.clearImageCacheLocked()
	c.mu.Unlock()
}

func OracleSetImageCacheLimit(c *Client, limit int64) {
	c.mu.Lock()
	c.imageCacheByteLimit = limit
	c.mu.Unlock()
}

func OracleEnsureOrigin(c *Client) *Error {
	return c.ensureOrigin()
}

func OracleEnsureCSRF(c *Client) *Error {
	return c.ensureCSRF()
}

func OracleResolveOrigin(c *Client, force bool) string {
	return c.resolveOrigin(force)
}

func OracleSaveSettingsFile(path string, settings Settings) error {
	c := &Client{settingsPath: path}
	return c.saveSettings(settings)
}

func OracleSettingsSnapshot(c *Client) Settings {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.settings
}

func OracleNewClientLoaded(version, profileDir string, secrets OracleSecretStore) (*Client, error) {
	return newClientWithSecrets(version, profileDir, secrets)
}

func OracleNewClientWithStoreFactory(version, profileDir string, factory func(string) OracleSecretStore) (*Client, error) {
	return newClientWithStoreFactory(version, profileDir, func(apiURL string) secretStore {
		return factory(apiURL)
	})
}

func OracleDecodeSetting(document, key, kind string) (any, bool) {
	var fields map[string]json.RawMessage
	if json.Unmarshal([]byte(document), &fields) != nil {
		switch kind {
		case "string":
			return "", false
		case "bool":
			return false, false
		default:
			return WindowBounds{}, false
		}
	}
	raw, present := fields[key]
	switch kind {
	case "string":
		var value string
		err := json.Unmarshal(raw, &value)
		return value, present && err == nil
	case "bool":
		var value bool
		err := json.Unmarshal(raw, &value)
		return value, present && err == nil
	default:
		var value WindowBounds
		err := json.Unmarshal(raw, &value)
		return value, present && err == nil
	}
}

func OracleWritePrivateFile(path string, data []byte) error {
	return writePrivateFile(path, data)
}

func OracleLoadLegacySettings() (Settings, bool) {
	return loadLegacySettings()
}

func OracleFileStore(path string) OracleSecretStore {
	return &fileSecretStore{path: path}
}

func OracleFallbackStore(primary, fallback OracleSecretStore) OracleSecretStore {
	return &fallbackSecretStore{primary: primary, fallback: fallback}
}

func OracleTimeoutStore(store OracleSecretStore, timeout time.Duration) OracleSecretStore {
	return &timeoutSecretStore{store: store, timeout: timeout}
}

func OracleStoreKind(store OracleSecretStore) string {
	switch store.(type) {
	case *fileSecretStore:
		return "file"
	case *fallbackSecretStore:
		return "fallback"
	case *timeoutSecretStore:
		return "timeout"
	case keyringSecretStore, *keyringSecretStore:
		return "keyring"
	default:
		return "other"
	}
}

func OracleDefaultSecretStoreKind(profileDir, apiURL string) string {
	return OracleStoreKind(defaultSecretStore(profileDir, apiURL))
}

func OracleEnvEnabled(name string) bool {
	return envEnabled(name)
}

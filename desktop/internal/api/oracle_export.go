//go:build oracle

package api

import (
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

package api

import (
	"net"
	"net/url"
	"strings"
)

const (
	defaultAPIURL    = "http://127.0.0.1:8080"
	fallbackOrigin   = "http://localhost:3000"
	maxAPIPathLength = 4096
)

func normalizeAPIURL(raw string) (string, *Error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", validationError("invalid_api_url", "Enter the API address.")
	}
	if containsControl(raw) || strings.ContainsAny(raw, "\\#") {
		return "", invalidAPIURL()
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" || parsed.Opaque != "" {
		return "", invalidAPIURL()
	}
	scheme := strings.ToLower(parsed.Scheme)
	if scheme != "http" && scheme != "https" {
		return "", validationError("invalid_api_url", "The API address must use http or https.")
	}
	if parsed.User != nil {
		return "", validationError("invalid_api_url", "The API address must not contain credentials.")
	}
	if parsed.Hostname() == "" || strings.HasSuffix(parsed.Host, ":") {
		return "", invalidAPIURL()
	}
	path := parsed.EscapedPath()
	if (path != "" && path != "/") || parsed.RawQuery != "" || parsed.ForceQuery ||
		parsed.Fragment != "" || parsed.RawFragment != "" {
		return "", validationError("invalid_api_url", "Use the API root, without a path, query or fragment.")
	}
	return canonicalOrigin(parsed), nil
}

func normalizeOrigin(raw string) (string, *Error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", nil
	}
	if containsControl(raw) || strings.Contains(raw, "\\") {
		return "", invalidOrigin()
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" || parsed.Opaque != "" {
		return "", invalidOrigin()
	}
	scheme := strings.ToLower(parsed.Scheme)
	if scheme != "http" && scheme != "https" {
		return "", validationError("invalid_origin", "The origin must use http or https.")
	}
	if parsed.User != nil {
		return "", validationError("invalid_origin", "The origin must not contain credentials.")
	}
	if parsed.Hostname() == "" || strings.HasSuffix(parsed.Host, ":") {
		return "", invalidOrigin()
	}
	return canonicalOrigin(parsed), nil
}

func invalidAPIURL() *Error {
	return validationError("invalid_api_url", "The API address must be an absolute http or https URL.")
}

func invalidOrigin() *Error {
	return validationError("invalid_origin", "The origin must be an absolute http or https URL.")
}

func validateServerAddress(apiURL string, allowNetwork bool) *Error {
	_, err := consentHostForUpdate(Settings{}, apiURL, allowNetwork)
	return err
}

func apiHost(apiURL string) (string, *Error) {
	parsed, err := url.Parse(apiURL)
	if err != nil || parsed.Hostname() == "" {
		return "", invalidAPIURL()
	}
	if transportErr := validateTransportAddress(parsed); transportErr != nil {
		return "", transportErr
	}
	return normalizeHost(parsed.Hostname()), nil
}

func consentHostForUpdate(previous Settings, apiURL string, allowNetwork bool) (string, *Error) {
	host, err := apiHost(apiURL)
	if err != nil {
		return "", err
	}
	if isLoopbackHost(host) {
		return "", nil
	}
	if allowNetwork {
		return host, nil
	}
	if strings.EqualFold(normalizeHost(previous.NetworkAllowedHost), host) {
		return "", nil
	}
	return "", networkConsentError()
}

func settingsConsentAllowed(settings Settings) bool {
	host, err := apiHost(settings.APIURL)
	if err != nil {
		return false
	}
	return isLoopbackHost(host) || strings.EqualFold(host, normalizeHost(settings.NetworkAllowedHost))
}

func normalizeHost(host string) string {
	return strings.ToLower(strings.TrimRight(strings.TrimSpace(host), "."))
}

func validateTransportAddress(parsed *url.URL) *Error {
	if parsed.Scheme == "http" && !isLocalHost(parsed.Hostname()) {
		return validationError("invalid_api_url", "Use an https address for a server that is not on this device.")
	}
	return nil
}

func isLoopbackHost(host string) bool {
	host = strings.ToLower(strings.TrimSuffix(host, "."))
	if host == "localhost" || strings.HasSuffix(host, ".localhost") {
		return true
	}
	ip := parseHostIP(host)
	return ip != nil && ip.IsLoopback()
}

func isLocalHost(host string) bool {
	host = strings.ToLower(strings.TrimSuffix(host, "."))
	if isLoopbackHost(host) || host == "localhost" || strings.HasSuffix(host, ".local") ||
		strings.HasSuffix(host, ".localhost") {
		return true
	}
	ip := parseHostIP(host)
	if ip == nil {
		return false
	}
	return ip.IsPrivate() || ip.IsLoopback() || ip.IsUnspecified() ||
		ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast()
}

func parseHostIP(host string) net.IP {
	if index := strings.IndexByte(host, '%'); index > 0 && strings.Contains(host[:index], ":") {
		host = host[:index]
	}
	return net.ParseIP(host)
}

func canonicalOrigin(parsed *url.URL) string {
	scheme := strings.ToLower(parsed.Scheme)
	host := strings.ToLower(parsed.Host)
	port := parsed.Port()
	if (scheme == "http" && port == "80") || (scheme == "https" && port == "443") {
		host = strings.TrimSuffix(host, ":"+port)
	}
	origin := url.URL{Scheme: scheme, Host: host}
	return origin.String()
}

func validateAPIPath(raw string) (*url.URL, *Error) {
	if raw == "" || len(raw) > maxAPIPathLength || containsControl(raw) || strings.Contains(raw, "\\") {
		return nil, invalidPath()
	}
	if !strings.HasPrefix(raw, "/api/") || strings.HasPrefix(raw, "//") {
		return nil, invalidPath()
	}
	rawPath, rawQuery, _ := strings.Cut(raw, "?")
	lowerPath := strings.ToLower(rawPath)
	if strings.Contains(raw, "#") ||
		strings.Contains(lowerPath, "%2e") ||
		strings.Contains(lowerPath, "%2f") ||
		strings.Contains(lowerPath, "%25") ||
		strings.Contains(lowerPath, "%5c") {
		return nil, invalidPath()
	}
	parsed, err := url.ParseRequestURI(raw)
	if err != nil || parsed.Scheme != "" || parsed.Host != "" || parsed.Opaque != "" || parsed.User != nil {
		return nil, invalidPath()
	}
	if containsControl(parsed.Path) || strings.Contains(parsed.Path, "\\") ||
		!strings.HasPrefix(parsed.Path, "/api/") {
		return nil, invalidPath()
	}
	for _, segment := range strings.Split(parsed.Path, "/") {
		if segment == "." || segment == ".." {
			return nil, invalidPath()
		}
	}
	decodedQuery, err := url.PathUnescape(rawQuery)
	if err != nil || containsControl(decodedQuery) {
		return nil, invalidPath()
	}
	return parsed, nil
}

func invalidPath() *Error {
	return validationError("invalid_path", "The desktop client only calls the API namespace.")
}

func containsControl(value string) bool {
	for i := 0; i < len(value); i++ {
		if value[i] < 0x20 || value[i] == 0x7f {
			return true
		}
	}
	return false
}

func (c *Client) targetURL(raw string) (*url.URL, uint64, *Error) {
	relative, pathErr := validateAPIPath(raw)
	if pathErr != nil {
		return nil, 0, pathErr
	}

	c.mu.RLock()
	apiURL := c.settings.APIURL
	allowedHost := c.settings.NetworkAllowedHost
	generation := c.generation
	c.mu.RUnlock()

	base, err := url.Parse(apiURL)
	if err != nil || base.Hostname() == "" {
		return nil, 0, invalidAPIURL()
	}
	if transportErr := validateTransportAddress(base); transportErr != nil {
		return nil, 0, transportErr
	}
	host := normalizeHost(base.Hostname())
	if !isLoopbackHost(host) && !strings.EqualFold(host, normalizeHost(allowedHost)) {
		return nil, 0, networkConsentError()
	}

	target := base.ResolveReference(relative)
	if canonicalOrigin(target) != canonicalOrigin(base) ||
		!strings.HasPrefix(target.Path, "/api/") || target.User != nil {
		return nil, 0, invalidPath()
	}
	return target, generation, nil
}

func networkConsentError() *Error {
	return validationError("network_consent_required",
		"Allow network access before connecting to a server outside this device.")
}

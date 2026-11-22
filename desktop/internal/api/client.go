package api

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

const (
	defaultWindowWidth  = 1380
	defaultWindowHeight = 900
)

type Client struct {
	version      string
	profileDir   string
	settingsPath string
	userAgent    string
	httpClient   *http.Client
	secrets      secretStore
	secretsFor   func(apiURL string) secretStore
	timeout      time.Duration
	imageTimeout time.Duration

	secretMu            sync.Mutex
	mu                  sync.RWMutex
	settings            Settings
	resolvedOrigin      string
	session             storedCookie
	csrf                storedCookie
	generation          uint64
	imageCache          map[string]string
	imageOrder          []string
	imageCacheBytes     int64
	imageCacheByteLimit int64
}

func NewClient(version, profileDir string) (*Client, error) {
	if strings.TrimSpace(profileDir) == "" {
		return nil, errors.New("profile directory is required")
	}
	return newClientWithStoreFactory(version, profileDir, nil)
}

func newClientWithSecrets(version, profileDir string, secrets secretStore) (*Client, error) {
	return newClientWithStoreFactory(version, profileDir, func(string) secretStore {
		return secrets
	})
}

func newClientWithStoreFactory(version, profileDir string, secretsFor func(string) secretStore) (*Client, error) {
	profileDir, err := filepath.Abs(profileDir)
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(profileDir, 0o700); err != nil {
		return nil, err
	}
	_ = os.Chmod(profileDir, 0o700)
	if secretsFor == nil {
		secretsFor = func(apiURL string) secretStore {
			return defaultSecretStore(profileDir, apiURL)
		}
	}

	c := &Client{
		version:      version,
		profileDir:   profileDir,
		settingsPath: filepath.Join(profileDir, "settings.json"),
		httpClient: &http.Client{
			CheckRedirect: func(*http.Request, []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
		secretsFor:          secretsFor,
		timeout:             defaultRequestTimeout,
		imageTimeout:        defaultImageTimeout,
		imageCacheByteLimit: defaultImageCacheBytes,
		settings:            Settings{APIURL: defaultAPIURL},
		imageCache:          make(map[string]string),
	}
	c.userAgent = fmt.Sprintf("AzaharDesktop/%s (%s)", version, deviceHash(profileDir))
	c.loadSettings()
	c.mu.RLock()
	apiURL := c.settings.APIURL
	c.mu.RUnlock()
	c.secrets = secretsFor(apiURL)
	if c.secrets == nil {
		c.secrets = &fileSecretStore{path: filepath.Join(profileDir, sessionScope(profileDir, apiURL)+".json")}
	}
	c.loadSession()
	return c, nil
}

func deviceHash(profileDir string) string {
	sum := sha256.Sum256([]byte(profileDir))
	return hex.EncodeToString(sum[:])[:12]
}

func (c *Client) loadSettings() {
	next := Settings{APIURL: defaultAPIURL}
	imported := false
	if data, err := os.ReadFile(c.settingsPath); err == nil {
		_ = os.Chmod(c.settingsPath, 0o600)
		var fields map[string]json.RawMessage
		if json.Unmarshal(data, &fields) == nil {
			decodeSetting(fields, "apiUrl", &next.APIURL)
			decodeSetting(fields, "origin", &next.Origin)
			decodeSetting(fields, "networkAllowed", &next.NetworkAllowed)
			decodeSetting(fields, "networkAllowedHost", &next.NetworkAllowedHost)
			decodeSetting(fields, "window", &next.Window)
		}
	} else if errors.Is(err, os.ErrNotExist) {
		if legacy, ok := loadLegacySettings(); ok {
			next = legacy
			imported = true
		}
	}
	if next.APIURL == "" {
		next.APIURL = defaultAPIURL
	}

	if value := strings.TrimSpace(os.Getenv("AZAHAR_API_URL")); value != "" {
		if normalized, err := normalizeAPIURL(value); err == nil {
			if _, hostErr := apiHost(normalized); hostErr == nil {
				next.APIURL = normalized
			}
		}
	}
	if value, ok := os.LookupEnv("AZAHAR_ORIGIN"); ok {
		if normalized, err := normalizeOrigin(value); err == nil {
			next.Origin = normalized
		}
	}

	host := ""
	if normalized, err := normalizeAPIURL(next.APIURL); err == nil {
		if parsedHost, hostErr := apiHost(normalized); hostErr == nil {
			next.APIURL = normalized
			host = parsedHost
		} else {
			next.APIURL = defaultAPIURL
		}
	} else {
		next.APIURL = defaultAPIURL
	}
	if host == "" {
		host, _ = apiHost(next.APIURL)
	}
	if normalized, err := normalizeOrigin(next.Origin); err == nil {
		next.Origin = normalized
	} else {
		next.Origin = ""
	}
	next.NetworkAllowedHost = normalizeHost(next.NetworkAllowedHost)
	if isLoopbackHost(host) || !strings.EqualFold(host, next.NetworkAllowedHost) {
		next.NetworkAllowedHost = ""
	}

	if imported {
		_ = c.saveSettings(next)
	}

	c.mu.Lock()
	c.settings = next
	c.mu.Unlock()
}

type legacySettings struct {
	APIURL string       `json:"apiUrl"`
	Origin string       `json:"origin"`
	Window WindowBounds `json:"window"`
}

func loadLegacySettings() (Settings, bool) {
	root, err := os.UserConfigDir()
	if err != nil {
		return Settings{}, false
	}
	data, err := os.ReadFile(filepath.Join(root, "Azahar Admin", "settings.json"))
	if err != nil {
		return Settings{}, false
	}
	var legacy legacySettings
	if json.Unmarshal(data, &legacy) != nil || strings.TrimSpace(legacy.APIURL) == "" {
		return Settings{}, false
	}
	return Settings{
		APIURL: legacy.APIURL,
		Origin: legacy.Origin,
		Window: legacy.Window,
	}, true
}

func decodeSetting(fields map[string]json.RawMessage, key string, target any) {
	raw, ok := fields[key]
	if !ok {
		return
	}
	_ = json.Unmarshal(raw, target)
}

func (c *Client) saveSettings(settings Settings) error {
	data, err := json.MarshalIndent(settings, "", "  ")
	if err != nil {
		return err
	}
	return writePrivateFile(c.settingsPath, data)
}

func (c *Client) Info() Info {
	c.mu.RLock()
	settings := c.settings
	resolved := settings.Origin
	if resolved == "" {
		resolved = c.resolvedOrigin
	}
	c.mu.RUnlock()

	return Info{
		Version:        c.version,
		Platform:       runtime.GOOS,
		APIURL:         settings.APIURL,
		Origin:         settings.Origin,
		ResolvedOrigin: resolved,
		NetworkAllowed: settingsConsentAllowed(settings),
	}
}

func (c *Client) UpdateSettings(apiURL, origin string, allowNetwork bool) (SettingsResult, error) {
	normalizedURL, urlErr := normalizeAPIURL(apiURL)
	if urlErr != nil {
		return SettingsResult{}, urlErr
	}
	normalizedOrigin, originErr := normalizeOrigin(origin)
	if originErr != nil {
		return SettingsResult{}, originErr
	}

	c.mu.Lock()
	previous := c.settings
	desiredHost, policyErr := consentHostForUpdate(previous, normalizedURL, allowNetwork)
	if policyErr != nil {
		c.mu.Unlock()
		return SettingsResult{}, policyErr
	}
	if previous.APIURL == normalizedURL && previous.Origin == normalizedOrigin &&
		previous.NetworkAllowedHost == desiredHost {
		resolved := previous.Origin
		if resolved == "" {
			resolved = c.resolvedOrigin
		}
		c.mu.Unlock()
		return c.settingsResult(resolved), nil
	}

	urlChanged := previous.APIURL != normalizedURL
	originChanged := previous.Origin != normalizedOrigin
	oldSecrets := c.secrets
	var newSecrets secretStore
	if urlChanged {
		newSecrets = c.secretsFor(normalizedURL)
		if newSecrets == nil {
			newSecrets = &fileSecretStore{
				path: filepath.Join(c.profileDir, sessionScope(c.profileDir, normalizedURL)+".json"),
			}
		}
	}

	c.settings.APIURL = normalizedURL
	c.settings.Origin = normalizedOrigin
	c.settings.NetworkAllowed = allowNetwork
	c.settings.NetworkAllowedHost = desiredHost
	if urlChanged {
		c.resolvedOrigin = ""
		c.session = storedCookie{}
		c.csrf = storedCookie{}
		c.clearImageCacheLocked()
		c.secrets = newSecrets
	}
	if urlChanged || originChanged {
		c.generation++
	}
	next := c.settings
	err := c.saveSettings(next)
	if err != nil {
		c.settings = previous
		c.generation++
		c.secrets = oldSecrets
	}
	c.mu.Unlock()

	if err != nil {
		return SettingsResult{}, errors.New("The settings could not be saved. Check that the profile folder is writable.")
	}
	if urlChanged {
		_ = c.deleteStoredSession(oldSecrets)
	}

	return c.settingsResult(c.resolveOrigin(true)), nil
}

func (c *Client) settingsResult(resolved string) SettingsResult {
	c.mu.RLock()
	settings := c.settings
	if resolved == "" {
		resolved = settings.Origin
	}
	if resolved == "" {
		resolved = c.resolvedOrigin
	}
	c.mu.RUnlock()

	return SettingsResult{
		APIURL:         settings.APIURL,
		Origin:         settings.Origin,
		ResolvedOrigin: resolved,
		NetworkAllowed: settingsConsentAllowed(settings),
	}
}

func (c *Client) WindowBounds() WindowBounds {
	c.mu.RLock()
	bounds := c.settings.Window
	c.mu.RUnlock()

	if bounds.Width <= 0 {
		bounds.Width = defaultWindowWidth
	}
	if bounds.Height <= 0 {
		bounds.Height = defaultWindowHeight
	}
	return bounds
}

func (c *Client) SaveWindowBounds(bounds WindowBounds) {
	if bounds.Width <= 0 {
		bounds.Width = defaultWindowWidth
	}
	if bounds.Height <= 0 {
		bounds.Height = defaultWindowHeight
	}

	c.mu.Lock()
	c.settings.Window = bounds
	_ = c.saveSettings(c.settings)
	c.mu.Unlock()
}

func (c *Client) resolveOrigin(force bool) string {
	c.mu.RLock()
	origin := c.settings.Origin
	resolved := c.resolvedOrigin
	apiURL := c.settings.APIURL
	c.mu.RUnlock()

	if origin != "" {
		return origin
	}
	if resolved != "" && !force {
		return resolved
	}

	c.Health()

	c.mu.Lock()
	defer c.mu.Unlock()
	if c.settings.Origin != "" {
		return c.settings.Origin
	}
	if c.settings.APIURL != apiURL {
		return c.resolvedOrigin
	}
	if c.resolvedOrigin != "" {
		return c.resolvedOrigin
	}
	if host, err := apiHost(c.settings.APIURL); err == nil && isLoopbackHost(host) {
		c.resolvedOrigin = fallbackOrigin
	}
	return c.resolvedOrigin
}

func (c *Client) clearImageCacheLocked() {
	c.imageCache = make(map[string]string)
	c.imageOrder = nil
	c.imageCacheBytes = 0
}

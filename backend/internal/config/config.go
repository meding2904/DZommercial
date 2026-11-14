package config

import (
	"errors"
	"fmt"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	AppEnv             string
	APIAddr            string
	AppOrigin          string
	TursoURL           string
	TursoToken         string
	CookieSecure       bool
	TrustProxy         bool
	SeedDemo           bool
	AdminEmail         string
	AdminPassword      string
	CustomerEmail      string
	CustomerPassword   string
	SessionTTL         time.Duration
	CourierAPIURL      string
	CourierKey         string
	CourierCredentials string
	CourierFromWilaya  int
	CourierTimeout     time.Duration
	TurnstileSecret    string
}

func Load() (Config, error) {
	appEnv := getenv("APP_ENV", "development")
	cfg := Config{
		AppEnv:           appEnv,
		APIAddr:          getenv("API_ADDR", ":8080"),
		AppOrigin:        strings.TrimRight(getenv("APP_ORIGIN", "http://localhost:5173"), "/"),
		TursoURL:         getenv("TURSO_DATABASE_URL", "http://127.0.0.1:8880"),
		TursoToken:       os.Getenv("TURSO_AUTH_TOKEN"),
		AdminEmail:       strings.ToLower(strings.TrimSpace(getenv("ADMIN_EMAIL", "admin@azahar.example"))),
		AdminPassword:    getenv("ADMIN_PASSWORD", "azahar-dev-admin-2026"),
		CustomerEmail:    strings.ToLower(strings.TrimSpace(getenv("CUSTOMER_EMAIL", "client@azahar.example"))),
		CustomerPassword: getenv("CUSTOMER_PASSWORD", "azahar-dev-client-2026"),
		SessionTTL:       time.Duration(getint("SESSION_TTL_HOURS", 168)) * time.Hour,

		CourierAPIURL:      strings.TrimRight(strings.TrimSpace(getenv("COURIER_API_URL", "https://freeship.dzbuild.com")), "/"),
		CourierKey:         strings.ToLower(strings.TrimSpace(getenv("COURIER_KEY", "sandbox"))),
		CourierCredentials: strings.TrimSpace(os.Getenv("COURIER_CREDENTIALS")),
		CourierFromWilaya:  getint("COURIER_FROM_WILAYA", 16),
		CourierTimeout:     time.Duration(getint("COURIER_TIMEOUT_SECONDS", 12)) * time.Second,
		TurnstileSecret:    strings.TrimSpace(os.Getenv("TURNSTILE_SECRET_KEY")),
	}
	cfg.SeedDemo = getbool("SEED_DEMO", strings.EqualFold(appEnv, "development"))

	cfg.CookieSecure = getbool("COOKIE_SECURE", cfg.IsProduction())
	cfg.TrustProxy = getbool("TRUST_PROXY", false)

	if cfg.IsProduction() {
		if _, ok := os.LookupEnv("TURSO_DATABASE_URL"); !ok || strings.TrimSpace(cfg.TursoURL) == "" {
			return Config{}, errors.New("TURSO_DATABASE_URL must be set when APP_ENV=production")
		}
		if _, ok := os.LookupEnv("TURSO_AUTH_TOKEN"); !ok || strings.TrimSpace(cfg.TursoToken) == "" {
			return Config{}, errors.New("TURSO_AUTH_TOKEN must be set when APP_ENV=production")
		}
		if _, ok := os.LookupEnv("APP_ORIGIN"); !ok || strings.TrimSpace(cfg.AppOrigin) == "" {
			return Config{}, errors.New("APP_ORIGIN must be set when APP_ENV=production")
		}
		if _, ok := os.LookupEnv("ADMIN_EMAIL"); !ok || strings.TrimSpace(cfg.AdminEmail) == "" {
			return Config{}, errors.New("ADMIN_EMAIL must be set when APP_ENV=production")
		}
		if _, ok := os.LookupEnv("ADMIN_PASSWORD"); !ok || strings.TrimSpace(cfg.AdminPassword) == "" {
			return Config{}, errors.New("ADMIN_PASSWORD must be set when APP_ENV=production")
		}
		if cfg.AdminPassword == "azahar-dev-admin-2026" {
			return Config{}, errors.New("ADMIN_PASSWORD must not be the development default when APP_ENV=production")
		}
		if len(cfg.AdminPassword) < 14 {
			return Config{}, errors.New("ADMIN_PASSWORD must be at least 14 characters when APP_ENV=production")
		}
		if cfg.SeedDemo && cfg.CustomerPassword == "azahar-dev-client-2026" {
			return Config{}, errors.New("CUSTOMER_PASSWORD must not be the development default when SEED_DEMO=true")
		}
		if cfg.TurnstileSecret == "" {
			return Config{}, errors.New("TURNSTILE_SECRET_KEY must be set when APP_ENV=production")
		}
		if strings.HasPrefix(cfg.TurnstileSecret, "1x") ||
			strings.HasPrefix(cfg.TurnstileSecret, "2x") ||
			strings.HasPrefix(cfg.TurnstileSecret, "3x") {
			return Config{}, errors.New("TURNSTILE_SECRET_KEY must not be a Turnstile test key")
		}
		cfg.CookieSecure = true
	}

	if cfg.AppOrigin != "" {
		u, err := url.Parse(cfg.AppOrigin)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" {
			return Config{}, fmt.Errorf("APP_ORIGIN must be an absolute http(s) URL")
		}
	}
	if cfg.SessionTTL <= 0 {
		cfg.SessionTTL = 168 * time.Hour
	}
	if cfg.SessionTTL > 30*24*time.Hour {
		return Config{}, errors.New("SESSION_TTL_HOURS must be 720 or less")
	}
	if strings.TrimSpace(cfg.APIAddr) == "" {
		return Config{}, errors.New("API_ADDR must not be empty")
	}

	courierURL, err := url.Parse(cfg.CourierAPIURL)
	if err != nil || (courierURL.Scheme != "http" && courierURL.Scheme != "https") || courierURL.Host == "" {
		return Config{}, errors.New("COURIER_API_URL must be an absolute http(s) URL")
	}
	if cfg.IsProduction() && courierURL.Scheme != "https" {
		return Config{}, errors.New("COURIER_API_URL must use https when APP_ENV=production")
	}
	if cfg.CourierKey == "" || len(cfg.CourierKey) > 64 {
		return Config{}, errors.New("COURIER_KEY must be between 1 and 64 characters")
	}
	if cfg.CourierFromWilaya < 1 || cfg.CourierFromWilaya > 58 {
		return Config{}, errors.New("COURIER_FROM_WILAYA must be between 1 and 58")
	}
	if cfg.CourierTimeout < time.Second || cfg.CourierTimeout > 30*time.Second {
		return Config{}, errors.New("COURIER_TIMEOUT_SECONDS must be between 1 and 30")
	}
	return cfg, nil
}

// DSN adds the auth token as a query parameter expected by the libsql driver.
func (c Config) DSN() string {
	if c.TursoToken == "" {
		return c.TursoURL
	}
	u, err := url.Parse(c.TursoURL)
	if err != nil {
		return c.TursoURL
	}
	q := u.Query()
	if q.Get("authToken") == "" {
		q.Set("authToken", c.TursoToken)
	}
	u.RawQuery = q.Encode()
	return u.String()
}

func (c Config) IsProduction() bool {
	return strings.EqualFold(c.AppEnv, "production")
}

func (c Config) IsDevelopment() bool {
	return strings.EqualFold(c.AppEnv, "development")
}

func (c Config) AdminSessionTTL() time.Duration {
	if c.SessionTTL < 12*time.Hour {
		return c.SessionTTL
	}
	return 12 * time.Hour
}

func getenv(key, fallback string) string {
	if v, ok := os.LookupEnv(key); ok {
		return v
	}
	return fallback
}

func getbool(key string, fallback bool) bool {
	v, ok := os.LookupEnv(key)
	if !ok || strings.TrimSpace(v) == "" {
		return fallback
	}
	b, err := strconv.ParseBool(strings.TrimSpace(v))
	if err != nil {
		return fallback
	}
	return b
}

func getint(key string, fallback int) int {
	v, ok := os.LookupEnv(key)
	if !ok || strings.TrimSpace(v) == "" {
		return fallback
	}
	n, err := strconv.Atoi(strings.TrimSpace(v))
	if err != nil {
		return fallback
	}
	return n
}

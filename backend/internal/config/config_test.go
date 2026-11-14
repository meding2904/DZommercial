package config

import (
	"strings"
	"testing"
)

func setProductionEnv(t *testing.T) {
	t.Helper()
	t.Setenv("APP_ENV", "production")
	t.Setenv("TURSO_DATABASE_URL", "http://127.0.0.1:8880")
	t.Setenv("TURSO_AUTH_TOKEN", "test-token")
	t.Setenv("APP_ORIGIN", "https://shop.example")
	t.Setenv("ADMIN_EMAIL", "admin@shop.example")
	t.Setenv("ADMIN_PASSWORD", "a-unique-production-password")
	t.Setenv("CUSTOMER_PASSWORD", "a-unique-customer-password")
	t.Setenv("SEED_DEMO", "false")
	t.Setenv("COOKIE_SECURE", "false")
	t.Setenv("SESSION_TTL_HOURS", "168")
	t.Setenv("COURIER_API_URL", "https://freeship.dzbuild.com")
	t.Setenv("COURIER_KEY", "sandbox")
	t.Setenv("COURIER_FROM_WILAYA", "16")
	t.Setenv("COURIER_TIMEOUT_SECONDS", "12")
	t.Setenv("TURNSTILE_SECRET_KEY", "test-turnstile-secret")
}

func TestProductionRequiresAdminPassword(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("ADMIN_PASSWORD", "")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "ADMIN_PASSWORD must be set") {
		t.Fatalf("expected an explicit ADMIN_PASSWORD error, got %v", err)
	}
}

func TestProductionRequiresTurnstileSecret(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("TURNSTILE_SECRET_KEY", "")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "TURNSTILE_SECRET_KEY must be set") {
		t.Fatalf("expected a TURNSTILE_SECRET_KEY error, got %v", err)
	}
}

func TestProductionRejectsTurnstileTestSecret(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("TURNSTILE_SECRET_KEY", "1x0000000000000000000000000000000AA")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "must not be a Turnstile test key") {
		t.Fatalf("expected a Turnstile test-key error, got %v", err)
	}
}

func TestProductionRejectsDevelopmentAdminPassword(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("ADMIN_PASSWORD", "azahar-dev-admin-2026")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "development default") {
		t.Fatalf("expected the development password to be rejected, got %v", err)
	}
}

func TestProductionRequiresAppOrigin(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("APP_ORIGIN", "")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "APP_ORIGIN must be set") {
		t.Fatalf("expected an APP_ORIGIN error, got %v", err)
	}
}

func TestProductionRejectsDevelopmentCustomerPasswordWhenSeeding(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("SEED_DEMO", "true")
	t.Setenv("CUSTOMER_PASSWORD", "azahar-dev-client-2026")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "CUSTOMER_PASSWORD") {
		t.Fatalf("expected the development customer password to be rejected, got %v", err)
	}
}

func TestProductionAcceptsStrongConfiguration(t *testing.T) {
	setProductionEnv(t)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("expected production configuration to load, got %v", err)
	}
	if !cfg.CookieSecure {
		t.Fatal("production must force secure cookies")
	}
	if cfg.AdminPassword != "a-unique-production-password" {
		t.Fatalf("unexpected admin password: %q", cfg.AdminPassword)
	}
}

func TestDevelopmentDefaultsStillLoad(t *testing.T) {
	t.Setenv("APP_ENV", "development")
	t.Setenv("COOKIE_SECURE", "false")
	t.Setenv("SEED_DEMO", "false")
	t.Setenv("SESSION_TTL_HOURS", "168")
	t.Setenv("COURIER_API_URL", "https://freeship.dzbuild.com")
	t.Setenv("COURIER_KEY", "sandbox")
	t.Setenv("COURIER_FROM_WILAYA", "16")
	t.Setenv("COURIER_TIMEOUT_SECONDS", "12")

	if _, err := Load(); err != nil {
		t.Fatalf("development configuration should load: %v", err)
	}
}

func TestProductionRequiresHTTPSForCourier(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("COURIER_API_URL", "http://freeship.example")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "COURIER_API_URL must use https") {
		t.Fatalf("expected the courier URL to require https, got %v", err)
	}
}

func TestCourierFromWilayaMustBeValid(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("COURIER_FROM_WILAYA", "0")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "COURIER_FROM_WILAYA") {
		t.Fatalf("expected an invalid origin wilaya error, got %v", err)
	}
}

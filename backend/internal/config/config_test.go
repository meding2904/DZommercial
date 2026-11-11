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
}

func TestProductionRequiresAdminPassword(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("ADMIN_PASSWORD", "")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "ADMIN_PASSWORD must be set") {
		t.Fatalf("expected an explicit ADMIN_PASSWORD error, got %v", err)
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

	if _, err := Load(); err != nil {
		t.Fatalf("development configuration should load: %v", err)
	}
}

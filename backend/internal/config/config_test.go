package config

import (
	"strings"
	"testing"
	"time"
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
	t.Setenv("PLUGIN_SECRET_KEY", "a-long-production-plugin-key")
	t.Setenv("AI_BASE_URL", "https://api.openai.com/v1")
	t.Setenv("AI_TIMEOUT_SECONDS", "25")
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

func TestAppEnvMustBeExplicit(t *testing.T) {
	t.Setenv("APP_ENV", "")
	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "APP_ENV must be set") {
		t.Fatalf("expected an explicit APP_ENV error, got %v", err)
	}
}

func TestUnknownAppEnvFailsClosed(t *testing.T) {
	t.Setenv("APP_ENV", "staging")
	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "APP_ENV") {
		t.Fatalf("expected an APP_ENV error, got %v", err)
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

func TestProductionRequiresPluginSecret(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("PLUGIN_SECRET_KEY", "short")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "PLUGIN_SECRET_KEY") {
		t.Fatalf("expected a PLUGIN_SECRET_KEY error, got %v", err)
	}
}

func TestProductionRequiresHTTPSForAI(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("AI_BASE_URL", "http://ai.example.test/v1")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "AI_BASE_URL must use https") {
		t.Fatalf("expected the AI URL to require https, got %v", err)
	}
}

func TestAITimeoutBounds(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("AI_TIMEOUT_SECONDS", "3")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "AI_TIMEOUT_SECONDS") {
		t.Fatalf("expected an AI timeout error, got %v", err)
	}
}

func TestAIURLRejectsCredentials(t *testing.T) {
	setProductionEnv(t)
	t.Setenv("AI_BASE_URL", "https://user:secret@ai.example.test/v1")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "must not contain credentials") {
		t.Fatalf("expected an AI URL credentials error, got %v", err)
	}
}

func TestCourierCredentialsParseLeniently(t *testing.T) {
	cases := []struct {
		name  string
		raw   string
		want  map[string]string
		valid bool
	}{
		{name: "empty", raw: "", want: map[string]string{}, valid: true},
		{name: "blank", raw: "   ", want: map[string]string{}, valid: true},
		{name: "object", raw: `{"apiId":"id","apiToken":"token"}`, want: map[string]string{"apiId": "id", "apiToken": "token"}, valid: true},
		{name: "null", raw: "null", want: nil, valid: true},
		{name: "not json", raw: "{not json", want: map[string]string{}, valid: false},
		{name: "array", raw: `["nope"]`, want: map[string]string{}, valid: false},
		{name: "bare string", raw: `"nope"`, want: map[string]string{}, valid: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg := Config{CourierCredentials: tc.raw}

			got, valid := cfg.CourierCredentialValues()
			if valid != tc.valid {
				t.Fatalf("CourierCredentialValues(%q) valid = %v, want %v", tc.raw, valid, tc.valid)
			}
			if (len(got) != len(tc.want)) {
				t.Fatalf("values = %v, want %v", got, tc.want)
			}
			if (tc.want == nil && got != nil) {
				t.Fatalf("values = %v, want nil", got)
			}
			if tc.want != nil && got == nil {
				t.Fatalf("values = nil, want %v", tc.want)
			}
			for key, want := range tc.want {
				if got[key] != want {
					t.Fatalf("values[%q] = %q, want %q", key, got[key], want)
				}
			}
		})
	}
}

func TestAuditBatchIntervalClamping(t *testing.T) {
	t.Setenv("APP_ENV", "development")
	t.Setenv("SEED_DEMO", "false")

	cases := []struct {
		name  string
		value string
		want  time.Duration
	}{
		{name: "default", value: "", want: 2 * time.Second},
		{name: "below minimum", value: "100", want: 500 * time.Millisecond},
		{name: "at minimum", value: "500", want: 500 * time.Millisecond},
		{name: "custom", value: "2500", want: 2500 * time.Millisecond},
		{name: "at maximum", value: "60000", want: time.Minute},
		{name: "above maximum", value: "90000", want: time.Minute},
		{name: "invalid falls back", value: "not-a-number", want: 2 * time.Second},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv("AUDIT_BATCH_INTERVAL_MS", tc.value)

			cfg, err := Load()
			if err != nil {
				t.Fatalf("load configuration: %v", err)
			}
			if cfg.AuditBatchInterval != tc.want {
				t.Fatalf("AUDIT_BATCH_INTERVAL_MS=%q loaded as %v, want %v", tc.value, cfg.AuditBatchInterval, tc.want)
			}
		})
	}
}

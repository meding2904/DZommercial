// Command oracle-config answers newline-delimited JSON requests about the Go
// config package so the Rust port can be diff-tested against it.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"azahar/internal/config"
)

var configEnvKeys = []string{
	"APP_ENV",
	"API_ADDR",
	"APP_ORIGIN",
	"TURSO_DATABASE_URL",
	"TURSO_AUTH_TOKEN",
	"ADMIN_EMAIL",
	"ADMIN_PASSWORD",
	"CUSTOMER_EMAIL",
	"CUSTOMER_PASSWORD",
	"EMPLOYEE_EMAIL",
	"EMPLOYEE_PASSWORD",
	"SESSION_TTL_HOURS",
	"COURIER_API_URL",
	"COURIER_KEY",
	"COURIER_CREDENTIALS",
	"COURIER_FROM_WILAYA",
	"COURIER_TIMEOUT_SECONDS",
	"TURNSTILE_SECRET_KEY",
	"PLUGIN_SECRET_KEY",
	"AI_BASE_URL",
	"AI_TIMEOUT_SECONDS",
	"AUDIT_BATCH_INTERVAL_MS",
	"SEED_DEMO",
	"COOKIE_SECURE",
	"TRUST_PROXY",
}

type request struct {
	Env                map[string]string `json:"env"`
	Op                 string            `json:"op"`
	TursoURL           *string           `json:"turso_url,omitempty"`
	TursoToken         *string           `json:"turso_token,omitempty"`
	CourierCredentials *string           `json:"courier_credentials,omitempty"`
	AppEnv             *string           `json:"app_env,omitempty"`
	SessionTTLNs       *int64            `json:"session_ttl_ns,omitempty"`
}

type configDTO struct {
	AppEnv             string `json:"app_env"`
	APIAddr            string `json:"api_addr"`
	AppOrigin          string `json:"app_origin"`
	TursoURL           string `json:"turso_url"`
	TursoToken         string `json:"turso_token"`
	CookieSecure       bool   `json:"cookie_secure"`
	TrustProxy         bool   `json:"trust_proxy"`
	SeedDemo           bool   `json:"seed_demo"`
	AdminEmail         string `json:"admin_email"`
	AdminPassword      string `json:"admin_password"`
	CustomerEmail      string `json:"customer_email"`
	CustomerPassword   string `json:"customer_password"`
	EmployeeEmail      string `json:"employee_email"`
	EmployeePassword   string `json:"employee_password"`
	SessionTTLNs       int64  `json:"session_ttl_ns"`
	CourierAPIURL      string `json:"courier_api_url"`
	CourierKey         string `json:"courier_key"`
	CourierCredentials string `json:"courier_credentials"`
	CourierFromWilaya  int    `json:"courier_from_wilaya"`
	CourierTimeoutNs   int64  `json:"courier_timeout_ns"`
	TurnstileSecret    string `json:"turnstile_secret"`
	PluginSecretKey    string `json:"plugin_secret_key"`
	AIBaseURL          string `json:"ai_base_url"`
	AITimeoutNs        int64  `json:"ai_timeout_ns"`
	AuditBatchInterval int64  `json:"audit_batch_interval_ns"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	encoder := json.NewEncoder(os.Stdout)

	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}

		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			if err := encoder.Encode(errorResponse(fmt.Errorf("invalid request: %w", err))); err != nil {
				fmt.Fprintln(os.Stderr, err)
				os.Exit(1)
			}
			continue
		}

		if err := encoder.Encode(handle(req)); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}

	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func handle(req request) map[string]any {
	if err := applyEnv(req.Env); err != nil {
		return errorResponse(err)
	}

	switch req.Op {
	case "load":
		cfg, err := config.Load()
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "config": toDTO(cfg)}

	case "dsn":
		cfg, err := config.Load()
		if err != nil {
			return errorResponse(err)
		}
		if req.TursoURL != nil {
			cfg.TursoURL = *req.TursoURL
		}
		if req.TursoToken != nil {
			cfg.TursoToken = *req.TursoToken
		}
		return map[string]any{"ok": true, "dsn": cfg.DSN()}

	case "credentials":
		cfg, err := config.Load()
		if err != nil {
			return errorResponse(err)
		}
		if req.CourierCredentials != nil {
			cfg.CourierCredentials = *req.CourierCredentials
		}
		values, valid := cfg.CourierCredentialValues()
		return map[string]any{"ok": true, "valid": valid, "credentials": values}

	case "flags":
		cfg, err := config.Load()
		if err != nil {
			return errorResponse(err)
		}
		if req.AppEnv != nil {
			cfg.AppEnv = *req.AppEnv
		}
		return map[string]any{
			"ok":             true,
			"is_production":  cfg.IsProduction(),
			"is_development": cfg.IsDevelopment(),
		}

	case "admin_ttl":
		cfg, err := config.Load()
		if err != nil {
			return errorResponse(err)
		}
		if req.SessionTTLNs != nil {
			cfg.SessionTTL = time.Duration(*req.SessionTTLNs)
		}
		return map[string]any{
			"ok":                   true,
			"admin_session_ttl_ns": int64(cfg.AdminSessionTTL()),
		}
	}

	return errorResponse(fmt.Errorf("unknown op %q", req.Op))
}

func applyEnv(env map[string]string) error {
	for _, key := range configEnvKeys {
		if err := os.Unsetenv(key); err != nil {
			return err
		}
	}
	for key, value := range env {
		if err := os.Setenv(key, value); err != nil {
			return fmt.Errorf("set %s: %w", key, err)
		}
	}
	return nil
}

func errorResponse(err error) map[string]any {
	return map[string]any{"ok": false, "error": err.Error()}
}

func toDTO(cfg config.Config) configDTO {
	return configDTO{
		AppEnv:             cfg.AppEnv,
		APIAddr:            cfg.APIAddr,
		AppOrigin:          cfg.AppOrigin,
		TursoURL:           cfg.TursoURL,
		TursoToken:         cfg.TursoToken,
		CookieSecure:       cfg.CookieSecure,
		TrustProxy:         cfg.TrustProxy,
		SeedDemo:           cfg.SeedDemo,
		AdminEmail:         cfg.AdminEmail,
		AdminPassword:      cfg.AdminPassword,
		CustomerEmail:      cfg.CustomerEmail,
		CustomerPassword:   cfg.CustomerPassword,
		EmployeeEmail:      cfg.EmployeeEmail,
		EmployeePassword:   cfg.EmployeePassword,
		SessionTTLNs:       int64(cfg.SessionTTL),
		CourierAPIURL:      cfg.CourierAPIURL,
		CourierKey:         cfg.CourierKey,
		CourierCredentials: cfg.CourierCredentials,
		CourierFromWilaya:  cfg.CourierFromWilaya,
		CourierTimeoutNs:   int64(cfg.CourierTimeout),
		TurnstileSecret:    cfg.TurnstileSecret,
		PluginSecretKey:    cfg.PluginSecretKey,
		AIBaseURL:          cfg.AIBaseURL,
		AITimeoutNs:        int64(cfg.AITimeout),
		AuditBatchInterval: int64(cfg.AuditBatchInterval),
	}
}

package httpapi

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"azahar/internal/config"
	"azahar/internal/plugins"
)

type webhookTestPlugin struct{}

func (webhookTestPlugin) Manifest() plugins.Manifest {
	return plugins.Manifest{ID: "hooked", Name: "Hooked", Version: "1.0.0"}
}

func (webhookTestPlugin) Routes() []plugins.Route {
	return []plugins.Route{{
		Method: http.MethodPost, Pattern: "/hook", Webhook: true,
		Handler: func(*plugins.Context) {},
	}}
}

func TestCSRFExemptsRegisteredWebhookRoutes(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(webhookTestPlugin{}); err != nil {
		t.Fatalf("register plugin: %v", err)
	}
	server := &Server{
		cfg:     config.Config{AppEnv: "production", AppOrigin: "https://shop.example"},
		plugins: registry,
	}
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
	handler := server.enforceCSRF(next)

	webhook := httptest.NewRequest(http.MethodPost, "/api/plugins/hooked/hook", nil)
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, webhook)
	if recorder.Code != http.StatusNoContent {
		t.Fatalf("webhook status = %d, want 204", recorder.Code)
	}

	ordinary := httptest.NewRequest(http.MethodPost, "/api/checkout", nil)
	recorder = httptest.NewRecorder()
	handler.ServeHTTP(recorder, ordinary)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("ordinary post status = %d, want 403", recorder.Code)
	}
}

func TestCSRFAllowsLocalIngressInDevelopment(t *testing.T) {
	const token = "csrf-token-0123456789abcdef"
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})

	tests := []struct {
		name   string
		config config.Config
		origin string
		want   int
	}{
		{
			name:   "development kiri ingress",
			config: config.Config{AppEnv: "development", AppOrigin: "http://localhost:3000"},
			origin: "http://azahar-web.kiri.localhost",
			want:   http.StatusNoContent,
		},
		{
			name:   "development loopback",
			config: config.Config{AppEnv: "development", AppOrigin: "http://localhost:3000"},
			origin: "http://127.0.0.1:3000",
			want:   http.StatusNoContent,
		},
		{
			name:   "development remote origin",
			config: config.Config{AppEnv: "development", AppOrigin: "http://localhost:3000"},
			origin: "https://evil.example",
			want:   http.StatusForbidden,
		},
		{
			name:   "production exact origin",
			config: config.Config{AppEnv: "production", AppOrigin: "https://shop.example"},
			origin: "https://shop.example",
			want:   http.StatusNoContent,
		},
		{
			name:   "production kiri ingress",
			config: config.Config{AppEnv: "production", AppOrigin: "https://shop.example"},
			origin: "http://azahar-web.kiri.localhost",
			want:   http.StatusForbidden,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := &Server{cfg: test.config}
			request := httptest.NewRequest(http.MethodPost, "/api/auth/login", nil)
			request.Header.Set("Origin", test.origin)
			request.Header.Set("X-Requested-With", "azahar-web")
			request.Header.Set("X-Csrf-Token", token)
			request.AddCookie(&http.Cookie{Name: csrfCookieName, Value: token})
			recorder := httptest.NewRecorder()
			server.enforceCSRF(next).ServeHTTP(recorder, request)
			if recorder.Code != test.want {
				t.Fatalf("status = %d, want %d (%s)", recorder.Code, test.want, recorder.Body.String())
			}
		})
	}
}

func TestCSRFRequiresCustomHeaderOriginAndMatchingToken(t *testing.T) {
	server := &Server{cfg: config.Config{AppEnv: "development", AppOrigin: "http://localhost:3000"}}
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
	handler := server.enforceCSRF(next)
	const token = "csrf-token-0123456789abcdef"

	tests := []struct {
		name   string
		method string
		header http.Header
		cookie *http.Cookie
		want   int
	}{
		{
			name:   "safe request",
			method: http.MethodGet,
			want:   http.StatusNoContent,
		},
		{
			name:   "missing custom header",
			method: http.MethodPost,
			header: http.Header{"Origin": []string{"http://localhost:3000"}},
			want:   http.StatusForbidden,
		},
		{
			name:   "missing token",
			method: http.MethodPost,
			header: http.Header{
				"Origin":           []string{"http://localhost:3000"},
				"X-Requested-With": []string{"azahar-web"},
			},
			want: http.StatusForbidden,
		},
		{
			name:   "mismatched token",
			method: http.MethodPost,
			header: http.Header{
				"Origin":           []string{"http://localhost:3000"},
				"X-Requested-With": []string{"azahar-web"},
				"X-Csrf-Token":     []string{"wrong"},
			},
			cookie: &http.Cookie{Name: csrfCookieName, Value: token},
			want:   http.StatusForbidden,
		},
		{
			name:   "matching token",
			method: http.MethodPost,
			header: http.Header{
				"Origin":           []string{"http://localhost:3000"},
				"X-Requested-With": []string{"azahar-web"},
				"X-Csrf-Token":     []string{token},
			},
			cookie: &http.Cookie{Name: csrfCookieName, Value: token},
			want:   http.StatusNoContent,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := httptest.NewRequest(test.method, "/api/checkout", nil)
			for key, values := range test.header {
				for _, value := range values {
					request.Header.Add(key, value)
				}
			}
			if test.cookie != nil {
				request.AddCookie(test.cookie)
			}
			recorder := httptest.NewRecorder()
			handler.ServeHTTP(recorder, request)
			if recorder.Code != test.want {
				t.Fatalf("status = %d, want %d (%s)", recorder.Code, test.want, recorder.Body.String())
			}
		})
	}
}

package httpapi

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"azahar/internal/config"
)

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

//go:build oracle

// Command oracle-api is the differential oracle for the Rust desktop API port.
// It reads one JSON command per line from stdin and writes one JSON result per
// line to stdout. All HTTP work happens against a local httptest server; the
// real Azahar API is never contacted.
package main

import (
	"bufio"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"time"

	"azahar/desktop/internal/api"
)

type command struct {
	Op             string          `json:"op"`
	Raw            string          `json:"raw"`
	APIURL         string          `json:"api_url"`
	AllowedHost    string          `json:"allowed_host"`
	Origin         string          `json:"origin"`
	ResolvedOrigin string          `json:"resolved_origin"`
	AllowNetwork   bool            `json:"allow_network"`
	Platform       string          `json:"platform"`
	Host           string          `json:"host"`
	Status         int             `json:"status"`
	BodyString     string          `json:"body_string"`
	Method         string          `json:"method"`
	Session        string          `json:"session"`
	CSRF           string          `json:"csrf"`
	SessionExpires string          `json:"session_expires"`
	CSRFExpires    string          `json:"csrf_expires"`
	Now            string          `json:"now"`
	NowUnix        int64           `json:"now_unix"`
	SessionExpUnix int64           `json:"session_expires_unix"`
	CSRFExpUnix    int64           `json:"csrf_expires_unix"`
	IncludeOrigin  bool            `json:"include_origin"`
	IncludeCSRF    bool            `json:"include_csrf"`
	HasBody        bool            `json:"has_body"`
	UserAgent      string          `json:"user_agent"`
	Generation     uint64          `json:"generation"`
	Profile        string          `json:"profile"`
	SetCookie      string          `json:"set_cookie"`
	Value          string          `json:"value"`
	Kind           string          `json:"kind"`
	Settings       *api.Settings   `json:"settings"`
	Payload        json.RawMessage `json:"payload"`
	PayloadPresent bool            `json:"payload_present"`
}

type result struct {
	OK    bool       `json:"ok"`
	Value any        `json:"value,omitempty"`
	Error *api.Error `json:"error,omitempty"`
}

func main() {
	reader := bufio.NewReader(os.Stdin)
	writer := bufio.NewWriter(os.Stdout)
	defer writer.Flush()

	for {
		line, err := reader.ReadString('\n')
		if len(line) == 0 && err != nil {
			return
		}
		line = strings.TrimSpace(line)
		if line == "" {
			if err != nil {
				return
			}
			continue
		}
		var cmd command
		if decodeErr := json.Unmarshal([]byte(line), &cmd); decodeErr != nil {
			writeResult(writer, result{OK: false, Error: &api.Error{
				Code:    "oracle_bad_command",
				Message: decodeErr.Error(),
				Fields:  map[string]string{},
			}})
			continue
		}
		writeResult(writer, handle(cmd))
		if err != nil {
			return
		}
	}
}

func writeResult(writer *bufio.Writer, res result) {
	data, err := json.Marshal(res)
	if err != nil {
		data = []byte(`{"ok":false,"error":{"status":0,"code":"oracle_encode","message":"encode failed","fields":{}}}`)
	}
	_, _ = writer.Write(data)
	_ = writer.WriteByte('\n')
	_ = writer.Flush()
}

func handle(cmd command) result {
	switch cmd.Op {
	case "normalize_api_url":
		value, apiErr := api.OracleNormalizeAPIURL(cmd.Raw)
		return wrap(value, apiErr)
	case "normalize_origin":
		value, apiErr := api.OracleNormalizeOrigin(cmd.Raw)
		return wrap(value, apiErr)
	case "validate_server_address":
		return wrap(true, api.OracleValidateServerAddress(cmd.Raw, cmd.AllowNetwork))
	case "api_host":
		value, apiErr := api.OracleAPIHost(cmd.Raw)
		return wrap(value, apiErr)
	case "host_rules":
		loopback, local := api.OracleHostRules(cmd.Host)
		return wrap(map[string]bool{"loopback": loopback, "local": local}, nil)
	case "normalize_host":
		return wrap(api.OracleNormalizeHost(cmd.Host), nil)
	case "canonical_origin":
		return wrap(api.OracleCanonicalOrigin(cmd.Raw), nil)
	case "validate_path":
		path, rawQuery, csrf, apiErr := api.OracleValidateAPIPath(cmd.Raw)
		if apiErr != nil {
			return wrap(nil, apiErr)
		}
		return wrap(map[string]any{"path": path, "raw_query": rawQuery, "csrf": csrf}, nil)
	case "target_url":
		target, generation, apiErr := api.OracleTargetURL(cmd.APIURL, cmd.AllowedHost, cmd.Generation, cmd.Raw)
		if apiErr != nil {
			return wrap(nil, apiErr)
		}
		return wrap(map[string]any{"url": target, "generation": generation}, nil)
	case "request_headers":
		now := parseUnixOrTime(cmd.NowUnix, cmd.Now)
		headers := api.OracleRequestHeaders(
			cmd.UserAgent,
			cmd.APIURL,
			cmd.Origin,
			cmd.ResolvedOrigin,
			cmd.AllowedHost,
			cmd.Session,
			cmd.CSRF,
			parseUnixOrTime(cmd.SessionExpUnix, cmd.SessionExpires),
			parseUnixOrTime(cmd.CSRFExpUnix, cmd.CSRFExpires),
			now,
			cmd.IncludeOrigin,
			cmd.IncludeCSRF,
			cmd.HasBody,
		)
		return wrap(headers, nil)
	case "error_from_result":
		return wrap(api.OracleErrorFromResult(cmd.Status, cmd.BodyString), nil)
	case "error_code":
		return wrap(api.OracleErrorCode(cmd.BodyString), nil)
	case "decode_json":
		decoded := api.OracleDecodeJSON(cmd.BodyString)
		return wrap(map[string]any{"present": decoded != nil, "value": decoded}, nil)
	case "encode_body":
		var payload any
		if cmd.PayloadPresent {
			if err := json.Unmarshal(cmd.Payload, &payload); err != nil {
				return wrap(nil, &api.Error{Code: "invalid_body", Message: err.Error(), Fields: map[string]string{}})
			}
		}
		value, apiErr := api.OracleEncodeRequestBody(cmd.Method, payload)
		return wrap(value, apiErr)
	case "cookie_from_set_cookie":
		now := parseUnixOrTime(cmd.NowUnix, cmd.Now)
		value, expires, ok := api.OracleCookieFromSetCookie(cmd.SetCookie, now)
		expiresUnix := int64(0)
		if expires != "" {
			if parsed, parseErr := time.Parse(time.RFC3339Nano, expires); parseErr == nil {
				expiresUnix = parsed.Unix()
			}
		}
		return wrap(map[string]any{
			"present":      ok,
			"value":        value,
			"expires_unix": expiresUnix,
		}, nil)
	case "valid_cookie_value":
		return wrap(api.OracleValidCookieValue(cmd.Value), nil)
	case "session_scope":
		return wrap(api.OracleSessionScope(cmd.Profile, cmd.APIURL), nil)
	case "device_hash":
		return wrap(api.OracleDeviceHash(cmd.Profile), nil)
	case "firewall":
		return wrap(api.OracleFirewallGuide(cmd.Platform), nil)
	case "dto":
		return dto(cmd.Kind)
	case "settings_json":
		settings := api.Settings{}
		if cmd.Settings != nil {
			settings = *cmd.Settings
		}
		return marshal(settings)
	case "session_script":
		return sessionScript()
	default:
		return wrap(nil, &api.Error{
			Code:    "oracle_unknown_op",
			Message: "unknown op " + cmd.Op,
			Fields:  map[string]string{},
		})
	}
}

func wrap(value any, apiErr *api.Error) result {
	if apiErr != nil {
		return result{OK: false, Error: apiErr}
	}
	return result{OK: true, Value: value}
}

func marshal(value any) result {
	data, err := json.Marshal(value)
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_encode", Message: err.Error(), Fields: map[string]string{}})
	}
	return result{OK: true, Value: json.RawMessage(data)}
}

func dto(kind string) result {
	switch kind {
	case "info":
		return marshal(api.Info{
			Version:        "1",
			Platform:       "linux",
			APIURL:         "http://127.0.0.1:8080",
			Origin:         "",
			ResolvedOrigin: "",
			NetworkAllowed: false,
		})
	case "health":
		return marshal(api.Health{
			OK:         true,
			Status:     200,
			Env:        "test",
			Origin:     "http://localhost:3000",
			ServerTime: "now",
			Message:    "",
		})
	case "error":
		return marshal(api.Error{
			Status:  422,
			Code:    "validation_error",
			Message: "Check the fields.",
			Fields:  map[string]string{"email": "Required."},
		})
	case "response_success":
		return marshal(api.Response{OK: true, Data: map[string]any{"user": "Amina"}})
	case "response_empty":
		return marshal(api.Response{OK: true})
	case "response_failure":
		return marshal(api.Response{OK: false, Error: &api.Error{
			Status:  403,
			Code:    "csrf_failed",
			Message: "No.",
			Fields:  map[string]string{},
		}})
	case "settings_result":
		return marshal(api.SettingsResult{
			APIURL:         "http://127.0.0.1:8080",
			Origin:         "",
			ResolvedOrigin: "http://localhost:3000",
			NetworkAllowed: false,
		})
	case "settings_result_warning":
		return marshal(api.SettingsResult{
			APIURL:         "http://127.0.0.1:8080",
			Origin:         "http://admin.example",
			ResolvedOrigin: "http://admin.example",
			NetworkAllowed: true,
			Warning:        "warning",
		})
	case "settings":
		return marshal(api.Settings{
			APIURL:             "http://127.0.0.1:8080",
			Origin:             "",
			NetworkAllowed:     false,
			NetworkAllowedHost: "",
			Window:             api.WindowBounds{Width: 1, Height: 2, X: 3, Y: 4},
		})
	case "window":
		return marshal(api.WindowBounds{Width: 1, Height: 2, X: 3, Y: 4})
	case "firewall":
		return marshal(api.FirewallGuide{Platform: "linux", Title: "t", Intro: "i", Steps: []string{"s"}, Note: "n"})
	default:
		return wrap(nil, &api.Error{Code: "oracle_unknown_dto", Message: kind, Fields: map[string]string{}})
	}
}

func parseTime(value string) time.Time {
	if value == "" {
		return time.Time{}
	}
	parsed, err := time.Parse(time.RFC3339Nano, value)
	if err != nil {
		return time.Time{}
	}
	return parsed
}

func parseUnixOrTime(unix int64, value string) time.Time {
	if unix != 0 {
		return time.Unix(unix, 0).UTC()
	}
	return parseTime(value)
}

type memorySecrets struct {
	mu      sync.Mutex
	value   string
	present bool
}

func (s *memorySecrets) Load() (string, bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.value, s.present, nil
}

func (s *memorySecrets) Save(value string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.value = value
	s.present = true
	return nil
}

func (s *memorySecrets) Delete() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.value = ""
	s.present = false
	return nil
}

func (s *memorySecrets) stored() (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.value, s.present
}

type observation struct {
	Method        string `json:"method"`
	Path          string `json:"path"`
	RawQuery      string `json:"raw_query"`
	Cookie        string `json:"cookie"`
	CSRF          string `json:"csrf"`
	Origin        string `json:"origin"`
	RequestedWith string `json:"requested_with"`
	Accept        string `json:"accept"`
	ContentType   string `json:"content_type"`
	Body          string `json:"body"`
}

func sessionScript() result {
	var mu sync.Mutex
	var observations []observation
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		mu.Lock()
		observations = append(observations, observation{
			Method:        r.Method,
			Path:          r.URL.Path,
			RawQuery:      r.URL.RawQuery,
			Cookie:        r.Header.Get("Cookie"),
			CSRF:          r.Header.Get("X-CSRF-Token"),
			Origin:        r.Header.Get("Origin"),
			RequestedWith: r.Header.Get("X-Requested-With"),
			Accept:        r.Header.Get("Accept"),
			ContentType:   r.Header.Get("Content-Type"),
			Body:          string(body),
		})
		mu.Unlock()

		switch r.URL.Path {
		case "/api/health":
			writeJSON(w, http.StatusOK, map[string]any{
				"origin":      "http://localhost:3000",
				"env":         "test",
				"server_time": "2026-11-21T10:46:28Z",
			})
		case "/api/session":
			http.SetCookie(w, &http.Cookie{Name: "azahar_session", Value: "session-one", Path: "/"})
			http.SetCookie(w, &http.Cookie{Name: "azahar_csrf", Value: "csrf-one", Path: "/"})
			writeJSON(w, http.StatusOK, map[string]any{"user": map[string]any{"name": "Amina"}})
		case "/api/me":
			writeJSON(w, http.StatusOK, map[string]any{"user": map[string]any{"name": "Amina"}})
		case "/api/logout":
			http.SetCookie(w, &http.Cookie{Name: "azahar_session", Value: "", MaxAge: -1, Path: "/"})
			http.SetCookie(w, &http.Cookie{Name: "azahar_csrf", Value: "", MaxAge: -1, Path: "/"})
			writeJSON(w, http.StatusOK, map[string]any{"ok": true})
		default:
			writeJSON(w, http.StatusNotFound, map[string]any{"error": map[string]any{
				"code": "not_found", "message": "Not found.", "fields": map[string]string{},
			}})
		}
	})
	server := httptest.NewServer(handler)
	defer server.Close()

	profile, err := os.MkdirTemp("", "oracle-api-profile-")
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_profile", Message: err.Error(), Fields: map[string]string{}})
	}
	defer os.RemoveAll(profile)

	store := &memorySecrets{}
	client, err := api.OracleNewClient("2.0.0-test", profile, server.URL, store)
	if err != nil {
		return wrap(nil, &api.Error{Code: "oracle_client", Message: err.Error(), Fields: map[string]string{}})
	}

	calls := []struct {
		method string
		path   string
		body   any
	}{
		{method: http.MethodGet, path: "/api/session"},
		{method: http.MethodGet, path: "/api/me"},
		{method: http.MethodPost, path: "/api/logout", body: map[string]any{"reason": "done"}},
		{method: http.MethodGet, path: "/api/me"},
	}
	responses := make([]json.RawMessage, 0, len(calls))
	for _, call := range calls {
		response := client.Request(call.method, call.path, call.body)
		data, marshalErr := json.Marshal(response)
		if marshalErr != nil {
			return wrap(nil, &api.Error{Code: "oracle_encode", Message: marshalErr.Error(), Fields: map[string]string{}})
		}
		responses = append(responses, data)
	}

	session, csrf := api.OracleSessionState(client)
	value, present := store.stored()
	mu.Lock()
	defer mu.Unlock()
	return wrap(map[string]any{
		"observations": observations,
		"responses":    responses,
		"session":      session,
		"csrf":         csrf,
		"secret":       map[string]any{"value": value, "present": present},
	}, nil)
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

package api

import (
	"encoding/json"
	"testing"
)

func TestJSONContract(t *testing.T) {
	cases := []struct {
		name   string
		value  any
		fields []string
	}{
		{
			name:   "info",
			value:  Info{Version: "1", Platform: "linux", APIURL: "http://127.0.0.1:8080", Origin: "", ResolvedOrigin: "", NetworkAllowed: false},
			fields: []string{"version", "platform", "apiUrl", "origin", "resolvedOrigin", "networkAllowed"},
		},
		{
			name:   "health",
			value:  Health{OK: true, Status: 200, Env: "test", Origin: "http://localhost:3000", ServerTime: "now", Message: ""},
			fields: []string{"ok", "status", "env", "origin", "serverTime", "message"},
		},
		{
			name:   "error",
			value:  Error{Status: 422, Code: "validation_error", Message: "Check the fields.", Fields: map[string]string{"email": "Required."}},
			fields: []string{"status", "code", "message", "fields"},
		},
		{
			name:   "response success",
			value:  Response{OK: true, Data: map[string]any{"user": "Amina"}},
			fields: []string{"ok", "data"},
		},
		{
			name:   "response failure",
			value:  Response{OK: false, Error: &Error{Status: 403, Code: "csrf_failed", Message: "No.", Fields: map[string]string{}}},
			fields: []string{"ok", "error"},
		},
		{
			name:   "settings result",
			value:  SettingsResult{APIURL: "http://127.0.0.1:8080", Origin: "", ResolvedOrigin: "http://localhost:3000", NetworkAllowed: false},
			fields: []string{"apiUrl", "origin", "resolvedOrigin", "networkAllowed"},
		},
		{
			name:   "settings",
			value:  Settings{APIURL: "http://127.0.0.1:8080", Window: WindowBounds{Width: 1, Height: 2, X: 3, Y: 4}},
			fields: []string{"apiUrl", "origin", "networkAllowed", "networkAllowedHost", "window"},
		},
		{
			name:   "window",
			value:  WindowBounds{Width: 1, Height: 2, X: 3, Y: 4},
			fields: []string{"width", "height", "x", "y"},
		},
		{
			name:   "firewall guide",
			value:  FirewallGuide{Platform: "linux", Title: "t", Intro: "i", Steps: []string{"s"}, Note: "n"},
			fields: []string{"platform", "title", "intro", "steps", "note"},
		},
	}

	for _, tt := range cases {
		t.Run(tt.name, func(t *testing.T) {
			data, err := json.Marshal(tt.value)
			if err != nil {
				t.Fatal(err)
			}
			var fields map[string]any
			if err := json.Unmarshal(data, &fields); err != nil {
				t.Fatal(err)
			}
			for _, field := range tt.fields {
				if _, ok := fields[field]; !ok {
					t.Fatalf("%s is missing from %s", field, data)
				}
			}
		})
	}
}

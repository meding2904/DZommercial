package whatsapp

import (
	"testing"
)

func validSettings() map[string]string {
	return map[string]string{
		"enabled":         "true",
		"phone_number_id": "123456789012345",
		"access_token":    "EAAG-test-token-0123456789",
		"app_secret":      "0123456789abcdef0123456789abcdef",
		"verify_token":    "verify-me-please",
		"graph_version":   "v23.0",
		"order_template":  "order_confirmation",
		"status_template": "order_update",
	}
}

func TestParseSettingsDefaultsAndFlags(t *testing.T) {
	config := parseSettings(validSettings())
	if !config.active() {
		t.Fatalf("valid settings are not active: %#v", config)
	}
	if config.Version != "v23.0" || config.CountryCode != "213" {
		t.Fatalf("defaults not applied: %#v", config)
	}
	if !config.NotifyOrderCreated || !config.NotifyShipped || !config.NotifyDelivered || !config.NotifyCancelled {
		t.Fatalf("notifications default on: %#v", config)
	}

	values := validSettings()
	values["enabled"] = "false"
	values["notify_cancelled"] = "false"
	config = parseSettings(values)
	if config.active() || config.Enabled {
		t.Fatalf("disabled flag was lost: %#v", config)
	}
	if config.NotifyCancelled {
		t.Fatalf("explicit false was replaced: %#v", config)
	}
}

func TestParseSettingsFallsBackOnInvalidOptions(t *testing.T) {
	values := validSettings()
	values["graph_version"] = "23"
	values["order_template"] = "Order Confirmation"
	values["status_template_language"] = "english"
	values["default_country_code"] = "abc"
	config := parseSettings(values)
	if config.Version != defaultVersion || config.OrderTemplate != defaultOrderTemplate ||
		config.StatusTemplateLanguage != defaultTemplateLanguage || config.CountryCode != defaultCountryCode {
		t.Fatalf("invalid values did not fall back: %#v", config)
	}
}

func TestIncompleteCredentialsAreInactive(t *testing.T) {
	for _, key := range []string{"phone_number_id", "access_token", "app_secret", "verify_token"} {
		values := validSettings()
		delete(values, key)
		config := parseSettings(values)
		if config.active() {
			t.Fatalf("missing %s must make the plugin inactive", key)
		}
	}
}

func TestValidateSettings(t *testing.T) {
	plugin := New()
	if fields := plugin.ValidateSettings(validSettings()); len(fields) != 0 {
		t.Fatalf("valid settings were refused: %#v", fields)
	}

	tests := []struct {
		key   string
		value string
	}{
		{key: "phone_number_id", value: "phone-123"},
		{key: "access_token", value: "short"},
		{key: "app_secret", value: "short"},
		{key: "verify_token", value: "short"},
		{key: "graph_version", value: "23"},
		{key: "default_country_code", value: "2133x"},
		{key: "order_template", value: "Order Confirmation"},
		{key: "status_template", value: "status-update"},
		{key: "order_template_language", value: "english"},
		{key: "status_template_language", value: "EN"},
	}
	for _, test := range tests {
		values := validSettings()
		values[test.key] = test.value
		fields := plugin.ValidateSettings(values)
		if fields[test.key] == "" {
			t.Errorf("%s = %q was accepted: %#v", test.key, test.value, fields)
		}
	}
}

func TestManifestDeclaresWebhookAndRequiredCredentials(t *testing.T) {
	manifest := New().Manifest()
	if manifest.ID != pluginID || manifest.AdminPath != "/admin/plugins/whatsapp" {
		t.Fatalf("manifest identity is wrong: %#v", manifest)
	}
	required := map[string]bool{}
	for _, field := range manifest.Settings {
		if field.Required {
			required[field.Key] = true
		}
	}
	for _, key := range []string{"phone_number_id", "access_token", "app_secret", "verify_token"} {
		if !required[key] {
			t.Errorf("%s must be required so the plugin reports unconfigured", key)
		}
	}
	routes := New().Routes()
	webhooks := 0
	for _, route := range routes {
		if route.Webhook {
			webhooks++
			if route.Admin {
				t.Fatalf("webhook route %s is also admin", route.Pattern)
			}
		}
	}
	if webhooks != 2 {
		t.Fatalf("expected GET and POST webhook routes, got %d", webhooks)
	}
}

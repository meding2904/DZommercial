package plugins_test

import (
	"reflect"
	"testing"

	"azahar/internal/plugins"
)

func settingsManifest() plugins.Manifest {
	return plugins.Manifest{
		ID:      "sample",
		Name:    "Sample",
		Version: "1.0.0",
		Settings: []plugins.SettingField{
			{Key: "api_key", Label: "API key", Type: "secret", Required: true, MaxLength: 512},
			{Key: "model", Label: "Model", Type: "text", Required: true, Default: "gpt-4o-mini"},
			{Key: "enabled", Label: "Enabled", Type: "boolean", Default: "true"},
		},
	}
}

func TestMergeKeepsBlankSecret(t *testing.T) {
	existing := map[string]string{"api_key": "sk-old", "model": "gpt-4o-mini", "enabled": "true"}
	merged, fields := plugins.MergeSettings(settingsManifest(), existing,
		map[string]string{"api_key": "", "model": "gpt-4.1-mini", "enabled": "false"})
	if len(fields) != 0 {
		t.Fatalf("unexpected fields: %#v", fields)
	}
	if merged["api_key"] != "sk-old" {
		t.Fatalf("blank secret must keep the stored key, got %q", merged["api_key"])
	}
	if merged["model"] != "gpt-4.1-mini" || merged["enabled"] != "false" {
		t.Fatalf("submitted values did not apply: %#v", merged)
	}
}

func TestMergeRejectsUnknownAndMissing(t *testing.T) {
	_, fields := plugins.MergeSettings(settingsManifest(), map[string]string{},
		map[string]string{"nonsense": "x"})
	if fields["nonsense"] == "" {
		t.Fatalf("unknown setting must be reported: %#v", fields)
	}
	if fields["api_key"] == "" || fields["model"] == "" {
		t.Fatalf("required settings must be reported: %#v", fields)
	}
}

func TestMaskHidesSecrets(t *testing.T) {
	masked, set := plugins.MaskSettings(settingsManifest(),
		map[string]string{"api_key": "sk-secret", "model": "gpt-4o-mini", "enabled": "true"})
	if masked["api_key"] != "" {
		t.Fatalf("secret leaked in masked settings: %#v", masked)
	}
	if !reflect.DeepEqual(set, []string{"api_key"}) {
		t.Fatalf("unexpected secret list: %#v", set)
	}
	if masked["model"] != "gpt-4o-mini" {
		t.Fatalf("non-secret value missing: %#v", masked)
	}
}

func TestSettingsComplete(t *testing.T) {
	manifest := settingsManifest()
	if plugins.SettingsComplete(manifest, map[string]string{"api_key": "x", "model": "m"}) != true {
		t.Fatal("complete settings should report complete")
	}
	if plugins.SettingsComplete(manifest, map[string]string{"model": "m"}) {
		t.Fatal("missing secret should report incomplete")
	}
}

func TestDefaultFillsMissingDisplayValue(t *testing.T) {
	masked, _ := plugins.MaskSettings(settingsManifest(), map[string]string{})
	if masked["model"] != "gpt-4o-mini" || masked["enabled"] != "true" {
		t.Fatalf("defaults were not applied: %#v", masked)
	}
}

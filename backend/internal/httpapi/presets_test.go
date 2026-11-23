package httpapi

import "testing"

func TestThemePresets(t *testing.T) {
	for _, preset := range themePresets {
		if !knownPreset(preset) {
			t.Errorf("%s should be known", preset)
		}
	}
	for _, preset := range []string{"", "dark", "Cellar", "plaster "} {
		if knownPreset(preset) {
			t.Errorf("%q should not be known", preset)
		}
	}
}

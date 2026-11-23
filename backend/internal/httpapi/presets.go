package httpapi

import "slices"

var themePresets = []string{"plaster", "cellar", "terracotta", "lagoon"}

func knownPreset(preset string) bool {
	return slices.Contains(themePresets, preset)
}

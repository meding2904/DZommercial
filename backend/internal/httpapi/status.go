package httpapi

import "slices"

var orderStatuses = []string{"pending", "confirmed", "packed", "shipped", "delivered", "cancelled"}

var allowedTransitions = map[string][]string{
	"pending":   {"confirmed", "cancelled"},
	"confirmed": {"packed", "cancelled"},
	"packed":    {"shipped", "cancelled"},
	"shipped":   {"delivered"},
	"delivered": {},
	"cancelled": {},
}

func knownStatus(status string) bool {
	return slices.Contains(orderStatuses, status)
}

func canTransition(from, to string) bool {
	return slices.Contains(allowedTransitions[from], to)
}

var themePresets = []string{"plaster", "cellar", "terracotta", "lagoon"}

func knownPreset(preset string) bool {
	return slices.Contains(themePresets, preset)
}

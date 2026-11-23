package orderintake

import (
	"net/url"
	"sort"
	"strings"
)

// Line is one basket line. ExpectedPriceCents is nil when the caller does not
// pin a price, as the counter sale does not.
type Line struct {
	ProductID          string
	Quantity           int
	Options            map[string]string
	ExpectedPriceCents *int64
}

// BuildLines trims the product ids and options, merges repeated lines and
// returns the "items" field message the callers send when a line is invalid.
func BuildLines(items []Line) ([]Line, map[string]string) {
	fields := make(map[string]string)
	lines := make([]Line, 0, len(items))
	index := make(map[string]int)
	for _, item := range items {
		productID := strings.TrimSpace(item.ProductID)
		if productID == "" {
			fields["items"] = "Choose a product for every line."
			continue
		}
		if item.Quantity < 1 || item.Quantity > 10 {
			fields["items"] = "Use between 1 and 10 units per product."
			continue
		}
		options := normalizeOptions(item.Options)
		lineKey := productID + "|" + optionsKey(options)
		if existing, ok := index[lineKey]; ok {
			lines[existing].Quantity += item.Quantity
			if lines[existing].Quantity > 10 {
				fields["items"] = "Use between 1 and 10 units per product."
			}
			continue
		}
		index[lineKey] = len(lines)
		lines = append(lines, Line{
			ProductID:          productID,
			Quantity:           item.Quantity,
			Options:            options,
			ExpectedPriceCents: item.ExpectedPriceCents,
		})
	}
	return lines, fields
}

func normalizeOptions(options map[string]string) map[string]string {
	if len(options) == 0 {
		return map[string]string{}
	}
	out := make(map[string]string, len(options))
	for key, value := range options {
		out[strings.TrimSpace(key)] = strings.TrimSpace(value)
	}
	return out
}

func optionsKey(options map[string]string) string {
	keys := make([]string, 0, len(options))
	for key := range options {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		parts = append(parts, key+"="+url.QueryEscape(options[key]))
	}
	return strings.Join(parts, "&")
}

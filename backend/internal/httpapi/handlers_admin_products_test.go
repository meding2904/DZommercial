package httpapi

import (
	"encoding/json"
	"testing"
)

func TestFilterFieldProblemsKeepsOnlyTouchedFields(t *testing.T) {
	raw := map[string]json.RawMessage{"stock": json.RawMessage("12")}
	got := filterFieldProblems(map[string]string{
		"stock":             "bad stock",
		"name":              "bad name",
		"attributes.config": "bad config",
	}, raw)
	if len(got) != 1 || got["stock"] != "bad stock" {
		t.Fatalf("filtered = %#v, want only the stock problem", got)
	}
}

func TestFilterFieldProblemsKeepsTheWholeSaleWindow(t *testing.T) {
	raw := map[string]json.RawMessage{"sale_ends_at": json.RawMessage(`"2026-11-30T00:00:00Z"`)}
	got := filterFieldProblems(map[string]string{
		"sale_price_cents": "missing price",
		"sale_ends_at":     "not after start",
		"name":             "bad name",
	}, raw)
	if len(got) != 2 || got["sale_price_cents"] == "" || got["sale_ends_at"] == "" {
		t.Fatalf("filtered = %#v, want both sale problems", got)
	}
}

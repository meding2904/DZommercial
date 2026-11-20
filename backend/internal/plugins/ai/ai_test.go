package ai

import (
	"strings"
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

func TestPluginRegisters(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(New()); err != nil {
		t.Fatalf("register plugin: %v", err)
	}
}

func TestValidateSettingsModel(t *testing.T) {
	plugin := New()

	validModels := []string{
		"gpt-4o-mini",
		"gpt-4o",
		"llama3.1:8b",
		"Qwen/Qwen2.5-7B-Instruct",
		"ft:gpt-4o-mini:azahar:analyst:id",
	}
	for _, model := range validModels {
		fields := plugin.ValidateSettings(map[string]string{"model": model, "language": "en"})
		if message, ok := fields["model"]; ok {
			t.Errorf("model %q rejected: %s", model, message)
		}
	}

	invalidModels := []string{
		"",
		"gpt 4o",
		"gpt;4o",
		"gpt\t4o",
		"modèle",
		strings.Repeat("a", 65),
	}
	for _, model := range invalidModels {
		fields := plugin.ValidateSettings(map[string]string{"model": model, "language": "en"})
		if fields["model"] == "" {
			t.Errorf("model %q accepted", model)
		}
	}
}

func TestValidateSettingsLanguage(t *testing.T) {
	plugin := New()

	for _, language := range []string{"en", "fr", "ar"} {
		fields := plugin.ValidateSettings(map[string]string{"model": "gpt-4o-mini", "language": language})
		if message, ok := fields["language"]; ok {
			t.Errorf("language %q rejected: %s", language, message)
		}
	}
	for _, language := range []string{"", "de", "EN", "fr-FR"} {
		fields := plugin.ValidateSettings(map[string]string{"model": "gpt-4o-mini", "language": language})
		if fields["language"] == "" {
			t.Errorf("language %q accepted", language)
		}
	}
}

func TestProductPromptUsesFactsWithoutCustomerData(t *testing.T) {
	product := store.Product{
		Name:        "Hydrating serum",
		Subtitle:    "Light daily serum",
		Description: "A fragrance-free serum with hyaluronic acid.",
		PriceCents:  450000,
		Size:        "30 ml",
		Stock:       12,
		Category:    store.Category{Name: "Skincare"},
		Concerns:    []string{"dryness"},
		Ingredients: []string{"hyaluronic acid"},
		Attributes: map[string][]string{
			"scent": {"rose", "unscented"},
		},
	}

	system, user := buildProductPrompt("fr", product, productSales{
		Days: 30, Units: 9, Orders: 4, RevenueCents: 405000,
	})
	prompt := system + "\n" + user

	expected := []string{
		"Hydrating serum",
		"Light daily serum",
		"A fragrance-free serum with hyaluronic acid.",
		"Skincare",
		"DZD 4500.00",
		"30 ml",
		"Stock: 12",
		"Sales, last 30 days: 9 units, DZD 4050.00 revenue, 4 orders.",
		"dryness",
		"hyaluronic acid",
		"scent",
		"rose",
		"unscented",
		"French",
	}
	for _, value := range expected {
		if !strings.Contains(prompt, value) {
			t.Errorf("prompt is missing %q", value)
		}
	}

	lowered := strings.ToLower(prompt)
	for _, field := range []string{
		"email", "phone", "address", "token", "customer", "order number", "order_number",
	} {
		if strings.Contains(lowered, field) {
			t.Errorf("prompt contains customer data field %q", field)
		}
	}
}

func TestStorePromptUsesAggregates(t *testing.T) {
	metrics := store.Metrics{
		RevenueCents:         123456,
		PreviousRevenueCents: 100000,
		OrdersCount:          9,
		UnitsSold:            14,
		NewCustomers:         3,
		TopProducts: []store.TopProduct{
			{Name: "Serum", Units: 6, RevenueCents: 90000},
		},
		LowStock: []store.LowStockItem{
			{Name: "Cream", Stock: 2},
		},
		StatusSplit: map[string]int{"paid": 7, "pending": 2},
	}

	_, user := buildStorePrompt("en", 30, metrics)

	for _, value := range []string{
		"last 30 days",
		"DZD 1234.56",
		"DZD 1000.00",
		"Orders: 9",
		"Units sold: 14",
		"New customers: 3",
		"Serum: 6 units",
		"Cream: 2 in stock",
		"paid: 7 orders",
		"pending: 2 orders",
	} {
		if !strings.Contains(user, value) {
			t.Errorf("store prompt is missing %q", value)
		}
	}

	lowered := strings.ToLower(user)
	for _, field := range []string{"email", "phone", "address", "token", "order number", "order_number"} {
		if strings.Contains(lowered, field) {
			t.Errorf("store prompt contains customer data field %q", field)
		}
	}
}

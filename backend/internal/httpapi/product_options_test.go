package httpapi

import (
	"testing"

	"azahar/internal/plugins"
)

func phoneSchema() []plugins.OptionField {
	return []plugins.OptionField{
		{Key: "storage", Label: "Storage", Type: "select", Required: true},
		{Key: "ram", Label: "RAM", Type: "select", Required: true},
		{Key: "color", Label: "Color", Type: "select", Required: true},
	}
}

func phoneAttributes() map[string][]string {
	return map[string][]string{
		"storage": {"128 GB", "256 GB"},
		"ram":     {"8 GB"},
		"color":   {"Midnight", "Silver"},
	}
}

func TestSelectedOptionsAcceptsAllowedValues(t *testing.T) {
	options := map[string]string{"storage": "256 GB", "ram": "8 GB", "color": "Silver"}
	if fields := validateSelectedOptions(phoneSchema(), phoneAttributes(), options); len(fields) != 0 {
		t.Fatalf("allowed options were rejected: %#v", fields)
	}
}

func TestSelectedOptionsRequiresEveryField(t *testing.T) {
	options := map[string]string{"storage": "256 GB", "ram": "8 GB"}
	if fields := validateSelectedOptions(phoneSchema(), phoneAttributes(), options); fields["items"] == "" {
		t.Fatalf("a missing option must be reported: %#v", fields)
	}
}

func TestSelectedOptionsRejectsUnlistedValue(t *testing.T) {
	options := map[string]string{"storage": "512 GB", "ram": "8 GB", "color": "Silver"}
	if fields := validateSelectedOptions(phoneSchema(), phoneAttributes(), options); fields["items"] == "" {
		t.Fatalf("an unlisted option must be reported: %#v", fields)
	}
}

func TestSelectedOptionsRejectsUnknownKey(t *testing.T) {
	options := map[string]string{"storage": "256 GB", "ram": "8 GB", "color": "Silver", "battery": "x"}
	if fields := validateSelectedOptions(phoneSchema(), phoneAttributes(), options); fields["items"] == "" {
		t.Fatalf("an unknown option must be reported: %#v", fields)
	}
}

func TestCoreProductTakesNoOptions(t *testing.T) {
	if fields := validateSelectedOptions(nil, nil, map[string]string{}); len(fields) != 0 {
		t.Fatalf("an empty selection should pass: %#v", fields)
	}
	if fields := validateSelectedOptions(nil, nil, map[string]string{"size": "large"}); fields["items"] == "" {
		t.Fatal("a core product must reject options")
	}
}

func TestNonSelectPluginFieldsAreNotCustomerOptions(t *testing.T) {
	schema := []plugins.OptionField{
		{Key: "size", Label: "Size", Type: "text", Required: true},
		{Key: "concerns", Label: "Concerns", Type: "list"},
		{Key: "how_to", Label: "How to use", Type: "textarea"},
	}
	if fields := validateSelectedOptions(schema, nil, map[string]string{}); len(fields) != 0 {
		t.Fatalf("an oil product takes no customer options: %#v", fields)
	}
	if fields := validateSelectedOptions(schema, nil, map[string]string{"size": "100 ml"}); fields["items"] == "" {
		t.Fatal("descriptive fields must not be accepted as customer options")
	}
}

func TestOptionLabelFollowsSchemaOrder(t *testing.T) {
	options := map[string]string{"color": "Silver", "ram": "8 GB", "storage": "256 GB"}
	got := optionLabel(phoneSchema(), options)
	want := "Storage: 256 GB · RAM: 8 GB · Color: Silver"
	if got != want {
		t.Fatalf("label = %q, want %q", got, want)
	}
}

func TestOptionsKeyIsStable(t *testing.T) {
	first := optionsKey(map[string]string{"b": "two", "a": "one"})
	second := optionsKey(map[string]string{"a": "one", "b": "two"})
	if first != second {
		t.Fatalf("keys differ: %q vs %q", first, second)
	}
	if first == optionsKey(map[string]string{"a": "one"}) {
		t.Fatal("different options must produce different keys")
	}
}

func TestProductAttributesRequireEveryRequiredField(t *testing.T) {
	server := &Server{plugins: plugins.NewRegistry()}
	fields := server.validateProductAttributes("phone", phoneAttributes())
	if fields["plugin_id"] == "" {
		t.Fatalf("an unregistered plugin must be rejected: %#v", fields)
	}
}

func TestCoreProductRejectsAttributes(t *testing.T) {
	server := &Server{plugins: plugins.NewRegistry()}
	fields := server.validateProductAttributes("core", phoneAttributes())
	if fields["attributes"] == "" {
		t.Fatalf("core attributes must be rejected: %#v", fields)
	}
}

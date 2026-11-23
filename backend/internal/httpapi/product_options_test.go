package httpapi

import (
	"testing"

	"azahar/internal/plugins"
)

func phoneAttributes() map[string][]string {
	return map[string][]string{
		"storage": {"128 GB", "256 GB"},
		"ram":     {"8 GB"},
		"color":   {"Midnight", "Silver"},
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

func TestProductOptionFieldsPreferKeyProblems(t *testing.T) {
	productType := &plugins.ProductType{
		Label: "Oil",
		Fields: []plugins.OptionField{
			{Key: "storage", Label: "Size", Type: "select", Required: true},
		},
	}

	// Two keys that trim onto the same name are refused specifically, not
	// silently collapsed by the normalizer.
	fields := productOptionFields(productType, map[string][]string{
		" storage": {"100 ml"},
		"storage":  {"50 ml"},
	})
	if fields["attributes.storage"] != "Each option name can appear once." {
		t.Fatalf("duplicate keys must be refused specifically: %#v", fields)
	}

	// An unnamed option is refused with the key problem, not the generic
	// unknown-field wording that attributeFields also puts on "attributes".
	fields = productOptionFields(productType, map[string][]string{"  ": {"100 ml"}})
	if fields["attributes"] != "Give every option a name." {
		t.Fatalf("empty option names must be refused specifically: %#v", fields)
	}

	// The valid shape still passes untouched.
	fields = productOptionFields(productType, map[string][]string{"storage": {"100 ml", "50 ml"}})
	if len(fields) != 0 {
		t.Fatalf("valid options must pass: %#v", fields)
	}
}

package orderintake

import (
	"strings"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

// OptionSchema returns the option fields a product's plugin exposes.
func OptionSchema(registry *plugins.Registry, product store.Product) []plugins.OptionField {
	if product.PluginID == "" || product.PluginID == store.CorePluginID {
		return []plugins.OptionField{}
	}
	productType := registry.ProductType(product.PluginID)
	if productType == nil {
		return []plugins.OptionField{}
	}
	return productType.Fields
}

// selectedOptionFields checks the choices a customer sends with a line.
func selectedOptionFields(schema []plugins.OptionField, attributes map[string][]string, options map[string]string) map[string]string {
	fields := make(map[string]string)
	selectFields := make([]plugins.OptionField, 0, len(schema))
	for _, field := range schema {
		if field.Type == "select" {
			selectFields = append(selectFields, field)
		}
	}
	if len(selectFields) == 0 {
		if len(options) > 0 {
			fields["items"] = "This product does not take options."
		}
		return fields
	}
	known := make(map[string]bool, len(selectFields))
	for _, field := range selectFields {
		known[field.Key] = true
		value := strings.TrimSpace(options[field.Key])
		if value == "" {
			if field.Required {
				fields["items"] = "Choose a " + strings.ToLower(field.Label) + " for every product."
			}
			continue
		}
		allowed := false
		for _, candidate := range attributes[field.Key] {
			if candidate == value {
				allowed = true
				break
			}
		}
		if !allowed {
			fields["items"] = "Choose an available " + strings.ToLower(field.Label) + "."
		}
	}
	for key := range options {
		if !known[key] {
			fields["items"] = "This product does not take that option."
			break
		}
	}
	return fields
}

func optionLabel(schema []plugins.OptionField, options map[string]string) string {
	parts := make([]string, 0, len(schema))
	for _, field := range schema {
		if field.Type != "select" {
			continue
		}
		value := strings.TrimSpace(options[field.Key])
		if value == "" {
			continue
		}
		parts = append(parts, field.Label+": "+value)
	}
	return strings.Join(parts, " · ")
}

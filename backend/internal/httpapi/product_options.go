package httpapi

import (
	"net/url"
	"sort"
	"strconv"
	"strings"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

func (s *Server) optionSchema(product store.Product) []plugins.OptionField {
	if product.PluginID == "" || product.PluginID == store.CorePluginID {
		return []plugins.OptionField{}
	}
	productType := s.plugins.ProductType(product.PluginID)
	if productType == nil {
		return []plugins.OptionField{}
	}
	return productType.Fields
}

// validateProductAttributes checks the options an admin stores on a product.
func (s *Server) validateProductAttributes(pluginID string, attributes map[string][]string) map[string]string {
	fields := make(map[string]string)
	pluginID = strings.TrimSpace(pluginID)
	if pluginID == "" {
		pluginID = store.CorePluginID
	}
	if pluginID == store.CorePluginID {
		if len(attributes) > 0 {
			fields["attributes"] = "A core product does not take plugin options."
		}
		return fields
	}
	plugin, ok := s.plugins.Get(pluginID)
	if !ok {
		fields["plugin_id"] = "Choose an installed plugin."
		return fields
	}
	productType := plugin.Manifest().ProductType
	if productType == nil {
		fields["plugin_id"] = "That plugin does not define a product type."
		return fields
	}

	known := make(map[string]plugins.OptionField, len(productType.Fields))
	for _, field := range productType.Fields {
		known[field.Key] = field
		values := attributes[field.Key]
		if field.Required && len(values) == 0 {
			fields["attributes."+field.Key] = "Add at least one " + strings.ToLower(field.Label) + " value."
			continue
		}
		switch field.Type {
		case "text", "textarea":
			max := 120
			if field.Type == "textarea" {
				max = 2000
			}
			if len(values) > 1 {
				fields["attributes."+field.Key] = "Use one value for this field."
				continue
			}
			if len(values) == 1 {
				value := strings.TrimSpace(values[0])
				if value == "" || len([]rune(value)) > max || hasControlRune(value) {
					fields["attributes."+field.Key] = "Use plain text under " + strconv.Itoa(max+1) + " characters."
				}
			}
		case "select", "list":
			if len(values) > 12 {
				fields["attributes."+field.Key] = "Use at most 12 values."
				continue
			}
			seen := make(map[string]bool, len(values))
			for _, value := range values {
				value = strings.TrimSpace(value)
				if value == "" || len([]rune(value)) > 80 || hasControlRune(value) {
					fields["attributes."+field.Key] = "Use plain values under 80 characters."
					break
				}
				if seen[value] {
					fields["attributes."+field.Key] = "Each value can appear once."
					break
				}
				seen[value] = true
			}
		}
	}
	for key := range attributes {
		if _, ok := known[key]; !ok {
			fields["attributes"] = "Remove options this product type does not define."
			break
		}
	}
	return fields
}

// validateSelectedOptions checks the choices a customer sends with a cart line.
func validateSelectedOptions(schema []plugins.OptionField, attributes map[string][]string, options map[string]string) map[string]string {
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

func normalizeAttributes(attributes map[string][]string) map[string][]string {
	if len(attributes) == 0 {
		return map[string][]string{}
	}
	out := make(map[string][]string, len(attributes))
	for key, values := range attributes {
		trimmed := make([]string, 0, len(values))
		for _, value := range values {
			trimmed = append(trimmed, strings.TrimSpace(value))
		}
		out[strings.TrimSpace(key)] = trimmed
	}
	return out
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

func hasControlRune(value string) bool {
	for _, r := range value {
		if r == '\n' || r == '\t' {
			continue
		}
		if r < 0x20 || r == 0x7f {
			return true
		}
	}
	return false
}

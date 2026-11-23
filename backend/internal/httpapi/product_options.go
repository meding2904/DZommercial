package httpapi

import (
	"strconv"
	"strings"

	"azahar/internal/orderintake"
	"azahar/internal/plugins"
	"azahar/internal/productrules"
	"azahar/internal/store"
)

func (s *Server) optionSchema(product store.Product) []plugins.OptionField {
	return orderintake.OptionSchema(s.plugins, product)
}

// validateProductAttributes checks the options an admin stores on a product.
// The raw map is validated first so duplicate or unnamed options are refused
// rather than silently collapsed by the normalizer.
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
	return productOptionFields(productType, attributes)
}

// productOptionFields composes the attribute rules and the option-name rules
// for a product type: the shared field problems first, then the more specific
// unnamed or repeated option-name problems.
func productOptionFields(productType *plugins.ProductType, attributes map[string][]string) map[string]string {
	normalized, keyProblems := productrules.NormalizeAttributes(attributes)
	fields := attributeFields(productrules.ValidateAttributes(productType, normalized))
	// Key problems are the more specific diagnosis, so they win over the
	// generic unknown-field message for the same attributes key.
	for key, message := range optionKeyFields(keyProblems) {
		fields[key] = message
	}
	return fields
}

// optionKeyFields words the option-name problems NormalizeAttributes reports.
func optionKeyFields(problems []productrules.Problem) map[string]string {
	fields := make(map[string]string)
	for _, problem := range problems {
		switch problem.Kind {
		case productrules.KindEmptyOptionName:
			fields["attributes"] = "Give every option a name."
		case productrules.KindDuplicateOptionName:
			fields["attributes."+problem.Field] = "Each option name can appear once."
		}
	}
	return fields
}

// attributeFields words the shared option problems for the admin form.
func attributeFields(problems []productrules.Problem) map[string]string {
	fields := make(map[string]string)
	for _, problem := range problems {
		if problem.Kind == productrules.KindUnknownField {
			fields["attributes"] = "Remove options this product type does not define."
			continue
		}
		key := "attributes." + problem.Field
		switch problem.Kind {
		case productrules.KindMissingRequired:
			fields[key] = "Add at least one " + strings.ToLower(problem.Label) + " value."
		case productrules.KindSingleValue:
			fields[key] = "Use one value for this field."
		case productrules.KindInvalidText:
			fields[key] = "Use plain text under " + strconv.Itoa(problem.Limit+1) + " characters."
		case productrules.KindTooManyValues:
			fields[key] = "Use at most 12 values."
		case productrules.KindInvalidValue:
			fields[key] = "Use plain values under 80 characters."
		case productrules.KindDuplicateValue:
			fields[key] = "Each value can appear once."
		}
	}
	return fields
}

// normalizeProductAttributes trims an admin form's options before storage.
// The caller validates the raw map first, so duplicate or unnamed keys are
// refused with a field problem instead of being silently collapsed here.
func normalizeProductAttributes(attributes map[string][]string) map[string][]string {
	normalized, _ := productrules.NormalizeAttributes(attributes)
	return normalized
}

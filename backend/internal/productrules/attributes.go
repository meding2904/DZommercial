package productrules

import (
	"strings"
	"unicode/utf8"

	"azahar/internal/plugins"
)

// Kind names the option rule a Problem breaks.
type Kind int

const (
	// KindMissingRequired means a required field has no value.
	KindMissingRequired Kind = iota + 1
	// KindSingleValue means a text field carries more than one value.
	KindSingleValue
	// KindTooManyValues means a select or list field carries more than twelve.
	KindTooManyValues
	// KindInvalidText means a text value is empty, too long or not plain text.
	KindInvalidText
	// KindInvalidValue means a select or list value is empty, too long or not
	// plain text.
	KindInvalidValue
	// KindDuplicateValue means a select or list value repeats.
	KindDuplicateValue
	// KindUnknownField means the map carries an option the product type does
	// not define.
	KindUnknownField
	// KindEmptyOptionName means an option key is empty once trimmed.
	KindEmptyOptionName
	// KindDuplicateOptionName means two option keys collapse onto one name.
	KindDuplicateOptionName
)

// Problem is one option rule a product's attributes break.
type Problem struct {
	// Field is the option key, or empty when the whole map is at fault.
	Field string
	Kind  Kind
	// Label is the option label as the plugin wrote it, for messages.
	Label string
	// Limit is the rune limit KindInvalidText broke, zero otherwise.
	Limit int
}

// NormalizeAttributes trims option keys and values. An empty or duplicate key
// is reported but every entry is still returned, so a caller that ignores
// problems keeps the plain overwrite it had before.
func NormalizeAttributes(attributes map[string][]string) (map[string][]string, []Problem) {
	if len(attributes) == 0 {
		return map[string][]string{}, nil
	}
	normalized := make(map[string][]string, len(attributes))
	var problems []Problem
	for key, values := range attributes {
		trimmed := make([]string, 0, len(values))
		for _, value := range values {
			trimmed = append(trimmed, strings.TrimSpace(value))
		}
		key = strings.TrimSpace(key)
		if key == "" {
			problems = append(problems, Problem{Kind: KindEmptyOptionName})
		} else if _, exists := normalized[key]; exists {
			problems = append(problems, Problem{Field: key, Kind: KindDuplicateOptionName})
		}
		normalized[key] = trimmed
	}
	return normalized, problems
}

// ValidateAttributes checks options against a product type. It returns at most
// one Problem per field, in the manifest's field order, and reports an unknown
// key last.
func ValidateAttributes(productType *plugins.ProductType, attributes map[string][]string) []Problem {
	known := make(map[string]plugins.OptionField, len(productType.Fields))
	var problems []Problem
	for _, field := range productType.Fields {
		known[field.Key] = field
		values := attributes[field.Key]
		if field.Required && len(values) == 0 {
			problems = append(problems, Problem{Field: field.Key, Kind: KindMissingRequired, Label: field.Label})
			continue
		}
		switch field.Type {
		case "text", "textarea":
			limit := maxTextRunes
			if field.Type == "textarea" {
				limit = maxTextAreaRunes
			}
			if len(values) > 1 {
				problems = append(problems, Problem{Field: field.Key, Kind: KindSingleValue, Label: field.Label})
				continue
			}
			if len(values) == 1 && !validOptionValue(strings.TrimSpace(values[0]), limit) {
				problems = append(problems, Problem{Field: field.Key, Kind: KindInvalidText, Label: field.Label, Limit: limit})
			}
		case "select", "list":
			if len(values) > maxOptionValues {
				problems = append(problems, Problem{Field: field.Key, Kind: KindTooManyValues, Label: field.Label})
				continue
			}
			seen := make(map[string]bool, len(values))
			for _, value := range values {
				value = strings.TrimSpace(value)
				if !validOptionValue(value, maxOptionValueRunes) {
					problems = append(problems, Problem{Field: field.Key, Kind: KindInvalidValue, Label: field.Label})
					break
				}
				if seen[value] {
					problems = append(problems, Problem{Field: field.Key, Kind: KindDuplicateValue, Label: field.Label})
					break
				}
				seen[value] = true
			}
		}
	}
	for key := range attributes {
		if _, ok := known[key]; !ok {
			problems = append(problems, Problem{Kind: KindUnknownField})
			break
		}
	}
	return problems
}

func validOptionValue(value string, limit int) bool {
	return value != "" && utf8.RuneCountInString(value) <= limit && PlainText(value)
}

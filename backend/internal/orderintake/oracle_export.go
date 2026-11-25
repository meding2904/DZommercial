//go:build oracle

// Oracle-only exports of the pure option helpers for differential tests. The
// `oracle` build tag keeps these adapters out of the normal API binary.
package orderintake

import (
	"azahar/internal/plugins"
	"azahar/internal/store"
)

func OracleOptionSchema(registry *plugins.Registry, product store.Product) []plugins.OptionField {
	return OptionSchema(registry, product)
}

func OracleSelectedOptionFields(schema []plugins.OptionField, attributes map[string][]string, options map[string]string) map[string]string {
	return selectedOptionFields(schema, attributes, options)
}

func OracleOptionLabel(schema []plugins.OptionField, options map[string]string) string {
	return optionLabel(schema, options)
}

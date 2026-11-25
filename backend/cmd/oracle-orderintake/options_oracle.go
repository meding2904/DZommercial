//go:build oracle

package main

import (
	"azahar/internal/orderintake"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

// optionsResult exercises the pure option helpers directly. It needs the
// oracle build tag because the helpers are unexported.
func optionsResult(registry *plugins.Registry, req request) map[string]any {
	product := store.Product{PluginID: req.ProductPluginID}
	schema := orderintake.OracleOptionSchema(registry, product)
	return map[string]any{
		"ok":     true,
		"schema": schema,
		"fields": orderintake.OracleSelectedOptionFields(schema, req.Attributes, req.Options),
		"label":  orderintake.OracleOptionLabel(schema, req.Options),
	}
}

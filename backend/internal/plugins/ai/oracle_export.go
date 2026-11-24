//go:build oracle

// Oracle-only exports for the AI plugin differential harness. The normal API
// binary does not compile this file.
package ai

import (
	"errors"

	"azahar/internal/store"
)

// OracleSettingsConfig is the wire form of the unexported settingsConfig.
type OracleSettingsConfig struct {
	APIKey   string `json:"api_key"`
	Model    string `json:"model"`
	Language string `json:"language"`
}

// OracleConfig reports the config class: "", "disabled" or "not_configured".
func OracleConfig(settings map[string]string) (OracleSettingsConfig, string) {
	config, err := configFromSettings(settings)
	switch {
	case errors.Is(err, errDisabled):
		return OracleSettingsConfig{}, "disabled"
	case errors.Is(err, errNotConfigured):
		return OracleSettingsConfig{}, "not_configured"
	case err != nil:
		return OracleSettingsConfig{}, "error"
	}
	return OracleSettingsConfig{
		APIKey:   config.apiKey,
		Model:    config.model,
		Language: config.language,
	}, ""
}

// OracleEnabled exposes enabledFromSettings.
func OracleEnabled(settings map[string]string) bool {
	return enabledFromSettings(settings)
}

// OracleAnalysisInstruction exposes analysisInstruction.
func OracleAnalysisInstruction(language string) string {
	return analysisInstruction(language)
}

// OracleBuildStorePrompt exposes buildStorePrompt.
func OracleBuildStorePrompt(language string, days int, metrics store.Metrics) (string, string) {
	return buildStorePrompt(language, days, metrics)
}

// OracleProductSales builds the unexported productSales value.
func OracleProductSales(days, units, orders int, revenueCents int64) productSales {
	return productSales{Days: days, Units: units, Orders: orders, RevenueCents: revenueCents}
}

// OracleBuildProductPrompt exposes buildProductPrompt.
func OracleBuildProductPrompt(language string, product store.Product, sales productSales) (string, string) {
	return buildProductPrompt(language, product, sales)
}

// OracleFormatDZD exposes formatDZD.
func OracleFormatDZD(cents int64) string {
	return formatDZD(cents)
}

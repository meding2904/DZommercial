//go:build oracle

// Oracle-only exports for the differential harness. The `oracle` build tag
// keeps these adapters out of the normal API binary.
package httpapi

func OracleRuneLen(value string) int                   { return runeLen(value) }
func OracleValidEmail(value string) bool               { return validEmail(value) }
func OracleValidSlug(value string) bool                { return validSlug(value) }
func OracleValidPhone(value string) bool               { return validPhone(value) }
func OracleValidAlgerianMobile(value string) bool      { return validAlgerianMobile(value) }
func OracleTruncateRunes(value string, max int) string { return truncateRunes(value, max) }
func OracleTrimAll(values []string) []string           { return trimAll(values) }

func OracleNormalizedAlgerianMobile(value string) (string, bool) {
	return normalizedAlgerianMobile(value)
}

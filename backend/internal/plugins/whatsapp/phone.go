package whatsapp

import (
	"strings"
	"unicode"
)

// normalizePhone turns a locally written number into the E.164 form WhatsApp
// expects. Numbers already carrying a country code keep it; a local number is
// completed with the configured default country code.
func normalizePhone(raw, countryCode string) (string, bool) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", false
	}
	countryCode = strings.TrimSpace(countryCode)
	if !countryPattern.MatchString(countryCode) {
		countryCode = ""
	}

	var builder strings.Builder
	for _, character := range raw {
		switch {
		case character >= '0' && character <= '9':
			builder.WriteRune(character)
		case character == '+':
			if builder.Len() > 0 {
				return "", false
			}
			builder.WriteRune(character)
		case unicode.IsSpace(character), character == '-', character == '(', character == ')', character == '.':
			continue
		default:
			return "", false
		}
	}

	value := builder.String()
	if value == "" {
		return "", false
	}
	international := strings.HasPrefix(value, "+")
	digits := strings.TrimPrefix(value, "+")
	if strings.HasPrefix(digits, "00") {
		international = true
		digits = strings.TrimPrefix(digits, "00")
	}
	if !international && countryCode != "" && !strings.HasPrefix(digits, countryCode) {
		if strings.HasPrefix(digits, "0") {
			digits = strings.TrimPrefix(digits, "0")
		}
		digits = countryCode + digits
	}
	// A number written without + but already starting with the country code
	// is left alone, so 213550778899 is not doubled.
	if len(digits) < 8 || len(digits) > 15 || strings.HasPrefix(digits, "0") {
		return "", false
	}
	return "+" + digits, true
}

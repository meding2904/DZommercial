package phone

import (
	"net/url"
	"strings"
)

var optionKeys = []string{"config", "color"}

var optionLabels = map[string]string{
	"config": "Memory and storage",
	"color":  "Color",
}

// VariantKey is order-independent: the option keys always appear in the same
// order, so the same handset always maps to one stock bucket.
func VariantKey(options map[string]string) string {
	if len(options) == 0 {
		return ""
	}
	parts := make([]string, 0, len(optionKeys))
	for _, key := range optionKeys {
		parts = append(parts, key+"="+url.QueryEscape(strings.TrimSpace(options[key])))
	}
	return strings.Join(parts, "&")
}

func VariantLabel(options map[string]string) string {
	parts := make([]string, 0, len(optionKeys))
	for _, key := range optionKeys {
		value := strings.TrimSpace(options[key])
		if value == "" {
			continue
		}
		parts = append(parts, optionLabels[key]+": "+value)
	}
	return strings.Join(parts, " · ")
}

func ValidIMEI(imei string) bool {
	if len(imei) != 15 {
		return false
	}
	sum := 0
	for i := 0; i < len(imei); i++ {
		digit := int(imei[i] - '0')
		if digit < 0 || digit > 9 {
			return false
		}
		if (len(imei)-1-i)%2 == 1 {
			digit *= 2
			if digit > 9 {
				digit -= 9
			}
		}
		sum += digit
	}
	return sum%10 == 0
}

func allowedOption(values []string, value string) bool {
	for _, candidate := range values {
		if candidate == value {
			return true
		}
	}
	return false
}

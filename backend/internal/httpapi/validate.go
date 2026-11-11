package httpapi

import (
	"net/mail"
	"net/url"
	"regexp"
	"strings"
	"unicode/utf8"
)

var slugPattern = regexp.MustCompile(`^[a-z0-9]+(?:-[a-z0-9]+)*$`)
var phonePattern = regexp.MustCompile(`^[0-9 +()\-]{6,30}$`)

func runeLen(value string) int {
	return utf8.RuneCountInString(value)
}

func validEmail(value string) bool {
	if value == "" || len(value) > 254 || strings.ContainsAny(value, " \t\r\n") {
		return false
	}
	parsed, err := mail.ParseAddress(value)
	if err != nil {
		return false
	}
	return parsed.Address == value && strings.Contains(value, "@")
}

func validSlug(value string) bool {
	return len(value) >= 1 && len(value) <= 120 && slugPattern.MatchString(value)
}

func validImagePath(value string) bool {
	if value == "" || len(value) > 500 {
		return false
	}
	if strings.HasPrefix(value, "https://") {
		parsed, err := url.Parse(value)
		return err == nil && parsed.Host != ""
	}
	if !strings.HasPrefix(value, "/images/") {
		return false
	}
	return !strings.Contains(value, "..")
}

func validPhone(value string) bool {
	return phonePattern.MatchString(value)
}

// normalizedAlgerianMobile returns a local 10 digit mobile number. The courier
// API expects 05, 06 or 07 prefixes; spaces and punctuation are accepted on the
// way in and removed here.
func normalizedAlgerianMobile(value string) (string, bool) {
	var b strings.Builder
	for _, r := range value {
		switch {
		case r >= '0' && r <= '9':
			b.WriteRune(r)
		case r == '+' && b.Len() == 0:
			b.WriteRune(r)
		case r == ' ' || r == '-' || r == '(' || r == ')':
			continue
		default:
			return "", false
		}
	}

	digits := strings.TrimPrefix(b.String(), "+")
	if strings.HasPrefix(digits, "213") && len(digits) == 12 {
		digits = "0" + digits[3:]
	}
	if len(digits) != 10 {
		return "", false
	}
	if digits[0] != '0' || (digits[1] != '5' && digits[1] != '6' && digits[1] != '7') {
		return "", false
	}
	return digits, true
}

func validAlgerianMobile(value string) bool {
	_, ok := normalizedAlgerianMobile(value)
	return ok
}

func truncateRunes(value string, max int) string {
	if max <= 0 {
		return ""
	}
	runes := []rune(value)
	if len(runes) <= max {
		return value
	}
	return string(runes[:max])
}

func trimAll(values []string) []string {
	result := make([]string, 0, len(values))
	for _, value := range values {
		value = strings.TrimSpace(value)
		if value != "" {
			result = append(result, value)
		}
	}
	return result
}

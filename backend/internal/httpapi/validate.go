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

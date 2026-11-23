// Package productrules holds the product write rules the admin API and the
// codemode planner must agree on: name, price, stock, uploaded image paths and
// plugin option values. Each adapter keeps its own wording for the messages it
// returns, so an HTTP form and a planner error stay in their own voice.
package productrules

import (
	"regexp"
	"strings"
	"unicode/utf8"
)

const (
	// MinNameRunes and MaxNameRunes bound a product name.
	MinNameRunes = 2
	MaxNameRunes = 120
	// MaxSubtitleRunes and MaxDescriptionRunes bound the two long copy fields.
	MaxSubtitleRunes    = 160
	MaxDescriptionRunes = 4000
	// MaxPriceCents bounds a price and a compare-at price. The admin form reads
	// the same ceiling as "below 100000000".
	MaxPriceCents = 99999999
	// MaxStock bounds a stock count.
	MaxStock = 100000

	maxImagePathBytes   = 500
	maxOptionValues     = 12
	maxOptionValueRunes = 80
	maxTextRunes        = 120
	maxTextAreaRunes    = 2000
)

var imageIDPattern = regexp.MustCompile(`^img_[a-f0-9]{24}$`)

// ValidName reports whether name has the rune length of a product name.
func ValidName(name string) bool {
	length := utf8.RuneCountInString(name)
	return length >= MinNameRunes && length <= MaxNameRunes
}

// ValidPrice reports whether cents is a price the shop accepts.
func ValidPrice(cents int64) bool {
	return cents >= 1 && cents <= MaxPriceCents
}

// ValidStock reports whether stock is a stock count the shop accepts.
func ValidStock(stock int) bool {
	return stock >= 0 && stock <= MaxStock
}

// ValidImageID reports whether id names an uploaded image.
func ValidImageID(id string) bool {
	return imageIDPattern.MatchString(id)
}

// ValidImagePath reports whether value points at a bundled image under
// /images/ or at an uploaded one under /api/images/. The upload endpoint is the
// admin allowance; the storefront theme does not get it.
func ValidImagePath(value string) bool {
	if value == "" || len(value) > maxImagePathBytes {
		return false
	}
	if strings.ContainsAny(value, "\\%?#") || strings.Contains(value, "..") {
		return false
	}
	if strings.HasPrefix(value, "/api/images/") {
		return ValidImageID(strings.TrimPrefix(value, "/api/images/"))
	}
	return strings.HasPrefix(value, "/images/")
}

// PlainText reports whether value is free of control runes other than the
// newline and tab that textareas allow.
func PlainText(value string) bool {
	for _, r := range value {
		if r == '\n' || r == '\t' {
			continue
		}
		if r < 0x20 || r == 0x7f {
			return false
		}
	}
	return true
}

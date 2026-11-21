package store

import "time"

// SaleProblem identifies a way a sale window breaks the product sale rules.
type SaleProblem uint8

const (
	SalePriceInvalid SaleProblem = 1 << iota
	SalePriceMissing
	SaleStartMissing
	SaleEndMissing
	SaleEndNotAfterStart
)

// SaleWindow is a product's optional sale price and the interval during which
// it applies. A zero start or end means that endpoint is not set.
type SaleWindow struct {
	PriceCents *int64
	StartsAt   time.Time
	EndsAt     time.Time
}

// SaleWindow returns the product's sale settings.
func (p Product) SaleWindow() SaleWindow {
	return SaleWindow{
		PriceCents: p.SalePriceCents,
		StartsAt:   p.SaleStartsAt,
		EndsAt:     p.SaleEndsAt,
	}
}

// PricePositive reports whether a sale price is set above zero. The comparison
// with the regular price is part of PriceValid.
func (w SaleWindow) PricePositive() bool {
	return w.PriceCents != nil && *w.PriceCents > 0
}

// PriceValid reports whether the sale price is above zero and below the regular
// price.
func (w SaleWindow) PriceValid(regularPriceCents int64) bool {
	return w.PricePositive() && *w.PriceCents < regularPriceCents
}

// EndsAfterStarts reports whether both endpoints are set and the window ends
// after it starts. The start is inclusive and the end is exclusive.
func (w SaleWindow) EndsAfterStarts() bool {
	return !w.StartsAt.IsZero() && !w.EndsAt.IsZero() && w.EndsAt.After(w.StartsAt)
}

// Problems reports every sale rule the window breaks for a product priced at
// regularPriceCents. A window without a sale price only reports
// SalePriceMissing; a sale price without a complete, ordered window reports the
// missing endpoint or ordering problem.
func (w SaleWindow) Problems(regularPriceCents int64) []SaleProblem {
	if w.PriceCents == nil {
		if !w.StartsAt.IsZero() || !w.EndsAt.IsZero() {
			return []SaleProblem{SalePriceMissing}
		}
		return nil
	}

	problems := make([]SaleProblem, 0, 4)
	if !w.PriceValid(regularPriceCents) {
		problems = append(problems, SalePriceInvalid)
	}
	if w.StartsAt.IsZero() {
		problems = append(problems, SaleStartMissing)
	}
	if w.EndsAt.IsZero() {
		problems = append(problems, SaleEndMissing)
	} else if !w.StartsAt.IsZero() && !w.EndsAfterStarts() {
		problems = append(problems, SaleEndNotAfterStart)
	}
	return problems
}

// Valid reports whether the whole sale window is valid data. Expired and future
// windows are valid; use Active to tell whether the sale is running.
func (w SaleWindow) Valid(regularPriceCents int64) bool {
	return len(w.Problems(regularPriceCents)) == 0
}

// Active reports whether the product has a valid limited-time price at the
// given instant. The start is inclusive and the end is exclusive.
func (w SaleWindow) Active(now time.Time, regularPriceCents int64) bool {
	if !w.Valid(regularPriceCents) {
		return false
	}
	return !now.Before(w.StartsAt) && now.Before(w.EndsAt)
}

// SaleActive reports whether the product has a valid limited-time price at the
// given instant.
func (p Product) SaleActive(now time.Time) bool {
	return p.SaleWindow().Active(now, p.PriceCents)
}

// EffectivePriceCents is the price a customer pays at the given instant.
func (p Product) EffectivePriceCents(now time.Time) int64 {
	if p.SaleActive(now) {
		return *p.SalePriceCents
	}
	return p.PriceCents
}

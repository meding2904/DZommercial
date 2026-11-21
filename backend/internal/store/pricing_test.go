package store

import (
	"slices"
	"testing"
	"time"
)

func TestEffectivePriceUsesActiveSaleWindow(t *testing.T) {
	start := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	end := time.Date(2026, 11, 27, 10, 0, 0, 0, time.UTC)
	product := Product{
		PriceCents:     120000,
		SalePriceCents: int64Ptr(90000),
		SaleStartsAt:   start,
		SaleEndsAt:     end,
	}

	if got := product.EffectivePriceCents(start.Add(-time.Second)); got != 120000 {
		t.Fatalf("before sale = %d, want 120000", got)
	}
	if got := product.EffectivePriceCents(start); got != 90000 {
		t.Fatalf("at start = %d, want 90000", got)
	}
	if got := product.EffectivePriceCents(end.Add(-time.Second)); got != 90000 {
		t.Fatalf("before end = %d, want 90000", got)
	}
	if got := product.EffectivePriceCents(end); got != 120000 {
		t.Fatalf("at end = %d, want 120000", got)
	}
}

func TestEffectivePriceRejectsInvalidSale(t *testing.T) {
	start := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	end := start.Add(24 * time.Hour)
	cases := []Product{
		{PriceCents: 10000},
		{PriceCents: 10000, SalePriceCents: int64Ptr(0), SaleStartsAt: start, SaleEndsAt: end},
		{PriceCents: 10000, SalePriceCents: int64Ptr(10000), SaleStartsAt: start, SaleEndsAt: end},
		{PriceCents: 10000, SalePriceCents: int64Ptr(9000)},
		{PriceCents: 10000, SalePriceCents: int64Ptr(9000), SaleStartsAt: start},
	}
	for index, product := range cases {
		if got := product.EffectivePriceCents(start.Add(time.Hour)); got != product.PriceCents {
			t.Fatalf("case %d = %d, want regular price %d", index, got, product.PriceCents)
		}
	}
}

func TestSaleWindowProblems(t *testing.T) {
	start := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	end := start.Add(24 * time.Hour)
	const regularPrice = int64(10000)
	cases := []struct {
		name   string
		window SaleWindow
		want   []SaleProblem
	}{
		{name: "no sale"},
		{
			name:   "valid",
			window: SaleWindow{PriceCents: int64Ptr(9000), StartsAt: start, EndsAt: end},
		},
		{
			name:   "missing price with start",
			window: SaleWindow{StartsAt: start},
			want:   []SaleProblem{SalePriceMissing},
		},
		{
			name:   "missing price with end",
			window: SaleWindow{EndsAt: end},
			want:   []SaleProblem{SalePriceMissing},
		},
		{
			name:   "missing price with full window",
			window: SaleWindow{StartsAt: start, EndsAt: end},
			want:   []SaleProblem{SalePriceMissing},
		},
		{
			name:   "zero price",
			window: SaleWindow{PriceCents: int64Ptr(0), StartsAt: start, EndsAt: end},
			want:   []SaleProblem{SalePriceInvalid},
		},
		{
			name:   "negative price",
			window: SaleWindow{PriceCents: int64Ptr(-1), StartsAt: start, EndsAt: end},
			want:   []SaleProblem{SalePriceInvalid},
		},
		{
			name:   "price equal to regular",
			window: SaleWindow{PriceCents: int64Ptr(regularPrice), StartsAt: start, EndsAt: end},
			want:   []SaleProblem{SalePriceInvalid},
		},
		{
			name:   "price above regular",
			window: SaleWindow{PriceCents: int64Ptr(regularPrice + 1), StartsAt: start, EndsAt: end},
			want:   []SaleProblem{SalePriceInvalid},
		},
		{
			name:   "missing start",
			window: SaleWindow{PriceCents: int64Ptr(9000), EndsAt: end},
			want:   []SaleProblem{SaleStartMissing},
		},
		{
			name:   "missing end",
			window: SaleWindow{PriceCents: int64Ptr(9000), StartsAt: start},
			want:   []SaleProblem{SaleEndMissing},
		},
		{
			name:   "missing window",
			window: SaleWindow{PriceCents: int64Ptr(9000)},
			want:   []SaleProblem{SaleStartMissing, SaleEndMissing},
		},
		{
			name:   "reversed window",
			window: SaleWindow{PriceCents: int64Ptr(9000), StartsAt: end, EndsAt: start},
			want:   []SaleProblem{SaleEndNotAfterStart},
		},
		{
			name:   "equal endpoints",
			window: SaleWindow{PriceCents: int64Ptr(9000), StartsAt: start, EndsAt: start},
			want:   []SaleProblem{SaleEndNotAfterStart},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.window.Problems(regularPrice); !slices.Equal(got, tc.want) {
				t.Fatalf("problems = %v, want %v", got, tc.want)
			}
			if got := tc.window.Valid(regularPrice); got != (len(tc.want) == 0) {
				t.Fatalf("valid = %t, want %t", got, len(tc.want) == 0)
			}
		})
	}
}

func TestSaleWindowPriceRules(t *testing.T) {
	const regularPrice = int64(10000)
	cases := []struct {
		name         string
		price        *int64
		wantPositive bool
		wantValid    bool
	}{
		{name: "unset", price: nil},
		{name: "zero", price: int64Ptr(0)},
		{name: "negative", price: int64Ptr(-1)},
		{name: "positive below regular", price: int64Ptr(9000), wantPositive: true, wantValid: true},
		{name: "equal to regular", price: int64Ptr(regularPrice), wantPositive: true},
		{name: "above regular", price: int64Ptr(regularPrice + 1), wantPositive: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			window := SaleWindow{PriceCents: tc.price}
			if got := window.PricePositive(); got != tc.wantPositive {
				t.Fatalf("positive = %t, want %t", got, tc.wantPositive)
			}
			if got := window.PriceValid(regularPrice); got != tc.wantValid {
				t.Fatalf("valid = %t, want %t", got, tc.wantValid)
			}
		})
	}
}

func TestSaleWindowActiveBoundaries(t *testing.T) {
	start := time.Date(2026, 11, 20, 10, 0, 0, 0, time.UTC)
	end := start.Add(24 * time.Hour)
	window := SaleWindow{PriceCents: int64Ptr(9000), StartsAt: start, EndsAt: end}
	cases := []struct {
		name string
		now  time.Time
		want bool
	}{
		{name: "before start", now: start.Add(-time.Nanosecond)},
		{name: "at start", now: start, want: true},
		{name: "inside", now: start.Add(time.Hour), want: true},
		{name: "before end", now: end.Add(-time.Nanosecond), want: true},
		{name: "at end", now: end},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := window.Active(tc.now, 10000); got != tc.want {
				t.Fatalf("active = %t, want %t", got, tc.want)
			}
		})
	}

	future := SaleWindow{PriceCents: int64Ptr(9000), StartsAt: end, EndsAt: end.Add(time.Hour)}
	if !future.Valid(10000) {
		t.Fatal("future window should be valid data")
	}
	if future.Active(start, 10000) {
		t.Fatal("future window should not be active")
	}

	expired := SaleWindow{PriceCents: int64Ptr(9000), StartsAt: start.Add(-time.Hour), EndsAt: start}
	if !expired.Valid(10000) {
		t.Fatal("expired window should be valid data")
	}
	if expired.Active(start, 10000) {
		t.Fatal("expired window should not be active")
	}
}

func int64Ptr(value int64) *int64 { return &value }

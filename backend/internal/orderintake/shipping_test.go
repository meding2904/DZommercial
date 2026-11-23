package orderintake

import "testing"

func TestShippingCents(t *testing.T) {
	cases := []struct {
		name     string
		subtotal int64
		want     int64
	}{
		{"empty", 0, standardShippingCents},
		{"under the line", 123400, standardShippingCents},
		{"exactly on the line", freeShippingOverCents, standardShippingCents},
		{"one cent over", freeShippingOverCents + 1, 0},
		{"well over", 1200000, 0},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if got := ShippingCents(test.subtotal); got != test.want {
				t.Fatalf("ShippingCents(%d) = %d, want %d", test.subtotal, got, test.want)
			}
		})
	}
}

func TestNoShipping(t *testing.T) {
	for _, subtotal := range []int64{0, 123400, freeShippingOverCents, 1200000} {
		if got := NoShipping(subtotal); got != 0 {
			t.Fatalf("NoShipping(%d) = %d, want 0", subtotal, got)
		}
	}
}

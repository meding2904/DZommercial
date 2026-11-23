package orderintake

const (
	// standardShippingCents is the flat delivery fee: 600 DZD.
	standardShippingCents int64 = 60000
	// freeShippingOverCents is the subtotal above which delivery is free:
	// 8 000 DZD. A subtotal exactly on the line still pays the fee.
	freeShippingOverCents int64 = 800000
)

// ShippingCents is the storefront delivery rule: a flat 600 DZD, free over
// 8 000 DZD.
func ShippingCents(subtotalCents int64) int64 {
	if subtotalCents > freeShippingOverCents {
		return 0
	}
	return standardShippingCents
}

// NoShipping is the counter sale rule: the customer carries the goods away.
func NoShipping(int64) int64 {
	return 0
}

package orderintake

import "testing"

func BenchmarkShippingCents(b *testing.B) {
	b.ReportAllocs()
	for b.Loop() {
		ShippingCents(123400)
	}
}

package orderstatus

import "testing"

func BenchmarkCanTransition(b *testing.B) {
	for b.Loop() {
		CanTransition("packed", "shipped")
	}
}

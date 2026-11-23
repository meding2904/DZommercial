package courier

import (
	"context"
	"testing"
	"time"
)

// BenchmarkSnapshotDeliveryMap measures the bundled delivery map with no
// network: the read-only path checkout falls back to when dzship cannot answer.
func BenchmarkSnapshotDeliveryMap(b *testing.B) {
	snapshot := snapshotSource{now: time.Now}
	ctx := context.Background()
	for b.Loop() {
		if _, err := snapshot.wilayas(ctx); err != nil {
			b.Fatal(err)
		}
		if _, err := snapshot.communes(ctx, 16); err != nil {
			b.Fatal(err)
		}
	}
}

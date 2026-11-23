package courier

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"testing"
	"time"
)

// scriptedSource stands in for dzship in resolver tests. err is returned by
// every call so a test can pick the failure it cares about.
type scriptedSource struct {
	wilayasValue  []Wilaya
	communesValue []Commune
	desksValue    []Desk
	shipmentValue Shipment
	trackingValue Tracking
	err           error
	calls         int
}

func (s *scriptedSource) wilayas(context.Context) ([]Wilaya, error) {
	s.calls++
	return s.wilayasValue, s.err
}

func (s *scriptedSource) communes(context.Context, int) ([]Commune, error) {
	s.calls++
	return s.communesValue, s.err
}

func (s *scriptedSource) desks(context.Context, int) ([]Desk, error) {
	s.calls++
	return s.desksValue, s.err
}

func (s *scriptedSource) createOrder(context.Context, OrderRequest) (Shipment, error) {
	s.calls++
	return s.shipmentValue, s.err
}

func (s *scriptedSource) track(context.Context, string, string) (Tracking, error) {
	s.calls++
	return s.trackingValue, s.err
}

func newResolverTestClient(t *testing.T, key string, sandbox bool) (*Client, *scriptedSource) {
	t.Helper()
	client := New(Options{
		CourierKey:        key,
		AllowLocalSandbox: sandbox,
		CacheTTL:          time.Hour,
	})
	remote := &scriptedSource{}
	client.remote = remote
	return client, remote
}

func TestDeliveryMapFallsBackToTheSnapshotForARealCourier(t *testing.T) {
	client, remote := newResolverTestClient(t, "yalidine", false)
	remote.err = fmt.Errorf("%w: dial tcp: no route to host", ErrTransport)
	var notified []string
	client.onFallback = func(resource string) { notified = append(notified, resource) }

	wilayas, err := client.Wilayas(context.Background())
	if err != nil {
		t.Fatalf("wilayas: %v", err)
	}
	if len(wilayas) != 58 {
		t.Fatalf("wilayas = %d, want the 58 bundled wilayas", len(wilayas))
	}
	if remote.calls != 1 {
		t.Fatalf("remote calls = %d, want 1", remote.calls)
	}
	if len(notified) != 1 || notified[0] != "wilayas" {
		t.Fatalf("notified = %v, want [wilayas]", notified)
	}
}

func TestStaleDeliveryMapBeatsTheSnapshot(t *testing.T) {
	clock := time.Now()
	client, remote := newResolverTestClient(t, "yalidine", false)
	client.now = func() time.Time { return clock }
	remote.wilayasValue = []Wilaya{{Code: 16, NameFr: "Alger"}}

	if _, err := client.Wilayas(context.Background()); err != nil {
		t.Fatalf("prime cache: %v", err)
	}

	clock = clock.Add(2 * time.Hour)
	remote.calls = 0
	remote.err = errors.New("courier is gone")
	notified := 0
	client.onFallback = func(string) { notified++ }

	wilayas, err := client.Wilayas(context.Background())
	if err != nil {
		t.Fatalf("stale wilayas: %v", err)
	}
	if len(wilayas) != 1 || wilayas[0].Code != 16 {
		t.Fatalf("wilayas = %+v, want the stale cached wilaya", wilayas)
	}
	if remote.calls != 1 {
		t.Fatalf("remote calls = %d, want the expired cache to retry once", remote.calls)
	}
	if notified != 0 {
		t.Fatalf("notified = %d, want no snapshot fallback while a stale cache exists", notified)
	}
}

func TestCommunesAreCachedPerWilaya(t *testing.T) {
	client, remote := newResolverTestClient(t, "yalidine", false)
	remote.communesValue = []Commune{{WilayaCode: 16, NameFr: "Bab Ezzouar"}}

	for _, wilaya := range []int{16, 31, 16} {
		communes, err := client.Communes(context.Background(), wilaya)
		if err != nil || len(communes) != 1 {
			t.Fatalf("communes(%d) = %v, %v", wilaya, communes, err)
		}
	}
	if remote.calls != 2 {
		t.Fatalf("remote calls = %d, want one per wilaya", remote.calls)
	}
}

func TestSandboxDesksFallBackLocallyWhenTheCourierIsUnavailable(t *testing.T) {
	client, remote := newResolverTestClient(t, "sandbox", true)
	remote.err = &APIError{Status: http.StatusServiceUnavailable, Code: "courier_error", Message: "down"}
	var notified []string
	client.onFallback = func(resource string) { notified = append(notified, resource) }

	desks, err := client.Desks(context.Background(), 16)
	if err != nil {
		t.Fatalf("desks: %v", err)
	}
	if len(desks) != 1 || desks[0].ID != "SBX-16" || desks[0].CommuneName == "" {
		t.Fatalf("desks = %+v, want the sandbox desk for wilaya 16", desks)
	}
	if len(notified) != 1 || notified[0] != "desks" {
		t.Fatalf("notified = %v, want [desks]", notified)
	}

	if _, err := client.Desks(context.Background(), 16); err != nil {
		t.Fatalf("cached desks: %v", err)
	}
	if remote.calls != 1 {
		t.Fatalf("remote calls = %d, want the fallback cached for five minutes", remote.calls)
	}
}

func TestRealCourierNeverGetsTheLocalSandbox(t *testing.T) {
	client, remote := newResolverTestClient(t, "yalidine", true)
	remote.err = &APIError{Status: http.StatusServiceUnavailable, Code: "courier_error", Message: "down"}
	notified := 0
	client.onFallback = func(string) { notified++ }

	if _, err := client.CreateOrder(context.Background(), OrderRequest{Reference: "AZ-2026-0041"}); err == nil {
		t.Fatal("a real courier must not create a local fake shipment")
	}
	if _, err := client.Track(context.Background(), "yalidine", "YAL-1"); err == nil {
		t.Fatal("a real courier must not serve local tracking")
	}
	if _, err := client.Desks(context.Background(), 16); err == nil {
		t.Fatal("a real courier must not serve the local sandbox desk")
	}
	if notified != 0 {
		t.Fatalf("notified = %d, want no fallback notification for a real courier", notified)
	}
}

func TestSandboxDoesNotMaskACourierRejection(t *testing.T) {
	client, remote := newResolverTestClient(t, "sandbox", true)
	remote.err = &APIError{Status: http.StatusUnprocessableEntity, Code: "invalid_phone", Message: "Invalid phone."}

	_, err := client.CreateOrder(context.Background(), OrderRequest{Reference: "AZ-2026-0041"})
	var apiErr *APIError
	if !errors.As(err, &apiErr) || apiErr.Status != http.StatusUnprocessableEntity {
		t.Fatalf("error = %v, want the courier's 422", err)
	}
}

func TestOfflineSandboxSkipsTheNetwork(t *testing.T) {
	client, remote := newResolverTestClient(t, "sandbox", true)
	var notified []string
	client.onFallback = func(resource string) { notified = append(notified, resource) }
	client.markOffline()

	wilayas, err := client.Wilayas(context.Background())
	if err != nil || len(wilayas) != 58 {
		t.Fatalf("wilayas = %d, %v", len(wilayas), err)
	}
	shipment, err := client.CreateOrder(context.Background(), OrderRequest{Reference: "AZ-2026-0041"})
	if err != nil || shipment.TrackingNumber != "SBX-AZ-2026-0041" {
		t.Fatalf("shipment = %+v, %v", shipment, err)
	}
	tracking, err := client.Track(context.Background(), "sandbox", shipment.TrackingNumber)
	if err != nil || tracking.Status != "in_transit" || len(tracking.Events) != 3 {
		t.Fatalf("tracking = %+v, %v", tracking, err)
	}
	if remote.calls != 0 {
		t.Fatalf("remote calls = %d, want none while the sandbox is offline", remote.calls)
	}
	if len(notified) != 3 {
		t.Fatalf("notified = %v, want one per fallback", notified)
	}
}

func TestSnapshotResolvesACommuneWithoutTheNetwork(t *testing.T) {
	client, remote := newResolverTestClient(t, "sandbox", true)
	client.markOffline()

	commune, err := client.Commune(context.Background(), 16, "  bab ezzouar ")
	if err != nil || commune == "" {
		t.Fatalf("commune = %q, %v", commune, err)
	}
	if remote.calls != 0 {
		t.Fatalf("remote calls = %d, want none", remote.calls)
	}
}

package courier

import (
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"
)

//go:embed data/wilayas.json
var wilayaSnapshot []byte

//go:embed data/communes.json
var communeSnapshot []byte

var errNoSnapshot = errors.New("no bundled snapshot for this resource")

var (
	fallbackWilayas  = sync.OnceValue(loadWilayaSnapshot)
	fallbackCommunes = sync.OnceValue(loadCommuneSnapshot)
)

func loadWilayaSnapshot() []Wilaya {
	var value []Wilaya
	if err := json.Unmarshal(wilayaSnapshot, &value); err != nil {
		return nil
	}
	return value
}

func loadCommuneSnapshot() []Commune {
	var value []Commune
	if err := json.Unmarshal(communeSnapshot, &value); err != nil {
		return nil
	}
	return value
}

// snapshotSource is the bundled delivery map. Wilayas and their communes are
// always on disk, so the delivery map degrades to a read-only list instead of
// a 503. The sandbox placeholders for desks, parcels and tracking exist only
// when Client allows them; Client never asks a real courier key for those.
type snapshotSource struct {
	now func() time.Time
}

func (snapshotSource) wilayas(context.Context) ([]Wilaya, error) {
	value := fallbackWilayas()
	if len(value) == 0 {
		return nil, errNoSnapshot
	}
	return value, nil
}

func (snapshotSource) communes(_ context.Context, wilayaCode int) ([]Commune, error) {
	value := fallbackCommunesFor(wilayaCode)
	if len(value) == 0 {
		return nil, errNoSnapshot
	}
	return value, nil
}

func (snapshotSource) desks(_ context.Context, wilayaCode int) ([]Desk, error) {
	return localDesks(wilayaCode), nil
}

func (snapshotSource) createOrder(_ context.Context, req OrderRequest) (Shipment, error) {
	return localShipment(req), nil
}

func (s snapshotSource) track(_ context.Context, _, trackingNumber string) (Tracking, error) {
	return localTracking(trackingNumber, s.now()), nil
}

func fallbackCommunesFor(wilayaCode int) []Commune {
	all := fallbackCommunes()
	result := make([]Commune, 0)
	for _, commune := range all {
		if commune.WilayaCode == wilayaCode {
			result = append(result, commune)
		}
	}
	return result
}

func localDesks(wilayaCode int) []Desk {
	wilayaName := fmt.Sprintf("Wilaya %d", wilayaCode)
	for _, wilaya := range fallbackWilayas() {
		if wilaya.Code == wilayaCode {
			wilayaName = wilaya.NameFr
			break
		}
	}

	commune := ""
	for _, candidate := range fallbackCommunesFor(wilayaCode) {
		if candidate.NameFr != "" {
			commune = candidate.NameFr
			break
		}
	}

	address := "Sandbox address"
	if commune != "" {
		address = "Sandbox address, " + commune
	}
	return []Desk{{
		ID:          fmt.Sprintf("SBX-%d", wilayaCode),
		Name:        "Sandbox desk " + wilayaName,
		WilayaCode:  wilayaCode,
		CommuneName: commune,
		Address:     address,
	}}
}

func localShipment(req OrderRequest) Shipment {
	tracking := "SBX-" + sanitizeTracking(req.Reference)
	if tracking == "SBX-" {
		tracking = "SBX-LOCAL"
	}
	return Shipment{
		TrackingNumber: tracking,
		Status:         "created",
		Reference:      req.Reference,
	}
}

func sanitizeTracking(value string) string {
	var b strings.Builder
	for _, r := range strings.ToUpper(strings.TrimSpace(value)) {
		if (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '-' {
			b.WriteRune(r)
		}
		if b.Len() >= 60 {
			break
		}
	}
	return b.String()
}

func localTracking(trackingNumber string, now time.Time) Tracking {
	now = now.UTC()
	return Tracking{
		TrackingNumber: trackingNumber,
		Status:         "in_transit",
		Events: []TrackingEvent{
			{Status: "created", RawStatus: "Créé (sandbox local)", Timestamp: now.Add(-2 * time.Hour).Format(time.RFC3339)},
			{Status: "picked_up", RawStatus: "Ramassé (sandbox local)", Timestamp: now.Add(-time.Hour).Format(time.RFC3339)},
			{Status: "in_transit", RawStatus: "En transit (sandbox local)", Timestamp: now.Format(time.RFC3339)},
		},
	}
}

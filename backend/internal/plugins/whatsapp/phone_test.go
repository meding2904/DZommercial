package whatsapp

import "testing"

func TestNormalizePhone(t *testing.T) {
	tests := []struct {
		raw     string
		country string
		want    string
		ok      bool
	}{
		{raw: "0550 77 88 99", country: "213", want: "+213550778899", ok: true},
		{raw: "550778899", country: "213", want: "+213550778899", ok: true},
		{raw: "+213 550 77 88 99", country: "213", want: "+213550778899", ok: true},
		{raw: "00213550778899", country: "213", want: "+213550778899", ok: true},
		{raw: "213550778899", country: "213", want: "+213550778899", ok: true},
		{raw: "+33 6 12 34 56 78", country: "213", want: "+33612345678", ok: true},
		{raw: "(0550) 77-88-99", country: "213", want: "+213550778899", ok: true},
		{raw: "", country: "213", ok: false},
		{raw: "0550", country: "213", ok: false},
		{raw: "not a phone", country: "213", ok: false},
		{raw: "++213550778899", country: "213", ok: false},
		{raw: "+213550778899999999", country: "213", ok: false},
	}
	for _, test := range tests {
		got, ok := normalizePhone(test.raw, test.country)
		if ok != test.ok || got != test.want {
			t.Errorf("normalizePhone(%q, %q) = %q, %v; want %q, %v",
				test.raw, test.country, got, ok, test.want, test.ok)
		}
	}
}

func TestOptInState(t *testing.T) {
	tests := []struct {
		body     string
		optIn    bool
		explicit bool
	}{
		{body: "STOP", optIn: false, explicit: true},
		{body: " stop all ", optIn: false, explicit: true},
		{body: "ARRÊT", optIn: false, explicit: true},
		{body: "START", optIn: true, explicit: true},
		{body: "Unstop", optIn: true, explicit: true},
		{body: "Bonjour", optIn: true, explicit: false},
		{body: "stop by later", optIn: true, explicit: false},
	}
	for _, test := range tests {
		optIn, explicit := optInState(test.body)
		if optIn != test.optIn || explicit != test.explicit {
			t.Errorf("optInState(%q) = %v, %v; want %v, %v",
				test.body, optIn, explicit, test.optIn, test.explicit)
		}
	}
}

func TestStatusSupersedes(t *testing.T) {
	tests := []struct {
		current string
		next    string
		want    bool
	}{
		{current: "queued", next: "sent", want: true},
		{current: "sent", next: "delivered", want: true},
		{current: "delivered", next: "read", want: true},
		{current: "read", next: "delivered", want: false},
		{current: "delivered", next: "failed", want: false},
		{current: "sent", next: "failed", want: true},
		{current: "failed", next: "delivered", want: true},
		{current: "queued", next: "queued", want: false},
	}
	for _, test := range tests {
		if got := statusSupersedes(test.current, test.next); got != test.want {
			t.Errorf("statusSupersedes(%q, %q) = %v, want %v",
				test.current, test.next, got, test.want)
		}
	}
}

func TestFormatMoney(t *testing.T) {
	tests := []struct {
		cents    int64
		currency string
		want     string
	}{
		{cents: 540000, currency: "DZD", want: "5,400 DZD"},
		{cents: 123456, currency: "DZD", want: "1,234.56 DZD"},
		{cents: 0, currency: "", want: "0 DZD"},
		{cents: -100, currency: "DZD", want: "-1 DZD"},
	}
	for _, test := range tests {
		if got := formatMoney(test.cents, test.currency); got != test.want {
			t.Errorf("formatMoney(%d, %q) = %q, want %q", test.cents, test.currency, got, test.want)
		}
	}
}

func TestFirstName(t *testing.T) {
	if got := firstName("  Amina   Benali "); got != "Amina" {
		t.Fatalf("firstName = %q", got)
	}
	if got := firstName("   "); got != "there" {
		t.Fatalf("empty firstName = %q", got)
	}
}

package orderstatus

import "testing"

func TestAllowedTransitions(t *testing.T) {
	valid := map[string][]string{
		"pending":   {"confirmed", "cancelled"},
		"confirmed": {"packed", "cancelled"},
		"packed":    {"shipped", "cancelled"},
		"shipped":   {"delivered"},
	}
	for from, targets := range valid {
		for _, to := range targets {
			if !CanTransition(from, to) {
				t.Errorf("expected %s -> %s to be allowed", from, to)
			}
		}
	}
}

func TestTerminalStatusesHaveNoTransitions(t *testing.T) {
	for _, from := range []string{"delivered", "cancelled"} {
		for _, to := range Statuses {
			if CanTransition(from, to) {
				t.Errorf("expected %s -> %s to be rejected", from, to)
			}
		}
	}
}

func TestInvalidTransitions(t *testing.T) {
	cases := [][2]string{
		{"pending", "shipped"},
		{"pending", "delivered"},
		{"confirmed", "shipped"},
		{"packed", "delivered"},
		{"shipped", "cancelled"},
		{"delivered", "cancelled"},
		{"cancelled", "pending"},
		{"pending", "pending"},
		{"unknown", "pending"},
	}
	for _, pair := range cases {
		if CanTransition(pair[0], pair[1]) {
			t.Errorf("expected %s -> %s to be rejected", pair[0], pair[1])
		}
	}
}

func TestKnownStatus(t *testing.T) {
	for _, status := range Statuses {
		if !Known(status) {
			t.Errorf("%s should be known", status)
		}
	}
	for _, status := range []string{"", "Pending", "refunded", "new"} {
		if Known(status) {
			t.Errorf("%s should not be known", status)
		}
	}
}

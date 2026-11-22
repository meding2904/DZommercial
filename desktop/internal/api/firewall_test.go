package api

import (
	"runtime"
	"strings"
	"testing"
)

func TestFirewallGuidePlatforms(t *testing.T) {
	tests := []struct {
		platform string
		contains []string
	}{
		{platform: "windows", contains: []string{"Windows Security", "Allow an app through firewall"}},
		{platform: "darwin", contains: []string{"System Settings", "Network", "Firewall", "Options"}},
		{platform: "linux", contains: []string{"ufw", "firewalld"}},
	}
	for _, tt := range tests {
		t.Run(tt.platform, func(t *testing.T) {
			guide := firewallGuide(tt.platform)
			if guide.Platform != tt.platform || guide.Title == "" || guide.Intro == "" ||
				guide.Note == "" || len(guide.Steps) == 0 {
				t.Fatalf("guide = %#v", guide)
			}
			if !strings.Contains(guide.Note, "never changes firewall rules") {
				t.Fatalf("note = %q", guide.Note)
			}
			joined := strings.Join(guide.Steps, "\n")
			for _, want := range tt.contains {
				if !strings.Contains(joined, want) {
					t.Fatalf("%s steps do not mention %q: %s", tt.platform, want, joined)
				}
			}
		})
	}
}

func TestFirewallGuideUsesCurrentPlatform(t *testing.T) {
	c, _ := testClient(t, "")
	guide := c.FirewallGuide()
	if guide.Platform != runtime.GOOS {
		t.Fatalf("platform = %q, want %q", guide.Platform, runtime.GOOS)
	}
}

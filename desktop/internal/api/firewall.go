package api

import "runtime"

func (c *Client) FirewallGuide() FirewallGuide {
	return firewallGuide(runtime.GOOS)
}

func firewallGuide(platform string) FirewallGuide {
	guide := FirewallGuide{
		Platform: platform,
		Title:    "Allow Azahar through your firewall",
		Intro:    "This app talks to your API from Go, not from the page.",
		Note:     "Azahar makes outbound connections only and never changes firewall rules for you.",
	}

	switch platform {
	case "windows":
		guide.Steps = []string{
			"Open Windows Security > Firewall & network protection (Windows Firewall).",
			"Choose Allow an app through firewall.",
			"Select Change settings, then Allow another app, and browse to the Azahar Admin executable.",
			"Tick the network profile where your API runs, usually Private, then confirm with OK.",
			"If the API runs on another computer, allow the app or the API port on that computer's firewall too.",
		}
	case "darwin":
		guide.Steps = []string{
			"Open System Settings > Network > Firewall > Options.",
			"Outbound connections are allowed by default. If a third-party firewall blocks them, allow Azahar Admin in that firewall.",
			"If the API runs on another Mac, allow incoming connections for the API process on that Mac, not for this client.",
		}
	case "linux":
		guide.Steps = []string{
			"Outbound traffic is usually allowed by default. If it is not, allow the API host and port in your firewall.",
			"With ufw, check the outgoing policy and allow traffic to the API address and port.",
			"With firewalld, check the active zone and allow outbound traffic to the API address and port.",
			"If the API runs on this machine, allow incoming connections to the API port for your local network.",
		}
	default:
		guide.Steps = []string{
			"Allow outbound connections from Azahar to your API host and port.",
			"If the API runs on another machine, allow the API port on that machine's firewall.",
		}
	}

	return guide
}

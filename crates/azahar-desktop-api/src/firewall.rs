//! Platform-specific firewall guidance.

use crate::client::Client;
use crate::types::{FirewallGuide, go_platform};

impl Client {
    pub fn firewall_guide(&self) -> FirewallGuide {
        firewall_guide(go_platform())
    }
}

pub fn firewall_guide(platform: &str) -> FirewallGuide {
    let mut guide = FirewallGuide {
        platform: platform.to_owned(),
        title: "Allow Azahar through your firewall".to_owned(),
        intro: "This app talks to your API from Go, not from the page.".to_owned(),
        steps: Vec::new(),
        note: "Azahar makes outbound connections only and never changes firewall rules for you."
            .to_owned(),
    };

    guide.steps = match platform {
        "windows" => vec![
            "Open Windows Security > Firewall & network protection (Windows Firewall).",
            "Choose Allow an app through firewall.",
            "Select Change settings, then Allow another app, and browse to the Azahar Admin executable.",
            "Tick the network profile where your API runs, usually Private, then confirm with OK.",
            "If the API runs on another computer, allow the app or the API port on that computer's firewall too.",
        ],
        "darwin" => vec![
            "Open System Settings > Network > Firewall > Options.",
            "Outbound connections are allowed by default. If a third-party firewall blocks them, allow Azahar Admin in that firewall.",
            "If the API runs on another Mac, allow incoming connections for the API process on that Mac, not for this client.",
        ],
        "linux" => vec![
            "Outbound traffic is usually allowed by default. If it is not, allow the API host and port in your firewall.",
            "With ufw, check the outgoing policy and allow traffic to the API address and port.",
            "With firewalld, check the active zone and allow outbound traffic to the API address and port.",
            "If the API runs on this machine, allow incoming connections to the API port for your local network.",
        ],
        _ => vec![
            "Allow outbound connections from Azahar to your API host and port.",
            "If the API runs on another machine, allow the API port on that machine's firewall.",
        ],
    }
    .into_iter()
    .map(str::to_owned)
    .collect();

    guide
}

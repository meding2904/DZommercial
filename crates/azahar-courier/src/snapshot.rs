//! The bundled delivery map. Wilayas and communes are always available, so the
//! map degrades to a read-only list instead of a 503. The sandbox placeholders
//! for desks, parcels and tracking are only reachable when `Client` allows
//! them; `Client` never asks a real courier key for those.

use std::sync::OnceLock;

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};

use crate::client::Source;
use crate::options::Clock;
use crate::types::{Commune, Desk, Error, OrderRequest, Shipment, Tracking, TrackingEvent, Wilaya};

const WILAYA_SNAPSHOT: &str = include_str!("../data/wilayas.json");
const COMMUNE_SNAPSHOT: &str = include_str!("../data/communes.json");

static WILAYAS: OnceLock<Vec<Wilaya>> = OnceLock::new();
static COMMUNES: OnceLock<Vec<Commune>> = OnceLock::new();

fn fallback_wilayas() -> &'static [Wilaya] {
    WILAYAS
        .get_or_init(|| serde_json::from_str(WILAYA_SNAPSHOT).unwrap_or_default())
        .as_slice()
}

fn fallback_communes() -> &'static [Commune] {
    COMMUNES
        .get_or_init(|| serde_json::from_str(COMMUNE_SNAPSHOT).unwrap_or_default())
        .as_slice()
}

fn fallback_communes_for(wilaya_code: i32) -> Vec<Commune> {
    fallback_communes()
        .iter()
        .filter(|commune| commune.wilaya_code == wilaya_code)
        .cloned()
        .collect()
}

pub(crate) struct SnapshotSource {
    now: Clock,
}

impl SnapshotSource {
    pub(crate) fn new(now: Clock) -> Self {
        Self { now }
    }
}

#[async_trait]
impl Source for SnapshotSource {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, Error> {
        let value = fallback_wilayas();
        if value.is_empty() {
            return Err(Error::NoSnapshot);
        }
        Ok(value.to_vec())
    }

    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, Error> {
        let value = fallback_communes_for(wilaya_code);
        if value.is_empty() {
            return Err(Error::NoSnapshot);
        }
        Ok(value)
    }

    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, Error> {
        Ok(local_desks(wilaya_code))
    }

    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, Error> {
        Ok(local_shipment(&request))
    }

    async fn track(&self, _courier_key: &str, tracking_number: &str) -> Result<Tracking, Error> {
        Ok(local_tracking(tracking_number, (self.now)()))
    }
}

fn local_desks(wilaya_code: i32) -> Vec<Desk> {
    let mut wilaya_name = format!("Wilaya {wilaya_code}");
    if let Some(wilaya) = fallback_wilayas()
        .iter()
        .find(|wilaya| wilaya.code == wilaya_code)
    {
        wilaya_name = wilaya.name_fr.clone();
    }

    let commune = fallback_communes_for(wilaya_code)
        .into_iter()
        .find(|commune| !commune.name_fr.is_empty())
        .map(|commune| commune.name_fr)
        .unwrap_or_default();

    let address = if commune.is_empty() {
        "Sandbox address".to_owned()
    } else {
        format!("Sandbox address, {commune}")
    };

    vec![Desk {
        id: format!("SBX-{wilaya_code}"),
        name: format!("Sandbox desk {wilaya_name}"),
        wilaya_code,
        commune_name: commune,
        address,
    }]
}

fn local_shipment(request: &OrderRequest) -> Shipment {
    let mut tracking = format!("SBX-{}", sanitize_tracking(&request.reference));
    if tracking == "SBX-" {
        tracking = "SBX-LOCAL".to_owned();
    }
    Shipment {
        tracking_number: tracking,
        status: "created".to_owned(),
        reference: request.reference.clone(),
        courier_reference: String::new(),
    }
}

fn sanitize_tracking(value: &str) -> String {
    let mut result = String::new();
    for character in value.trim().chars() {
        // `char::to_uppercase` performs full case mapping, which expands ß to
        // SS; Go's strings.ToUpper uses the simple mapping and leaves it.
        let mut upper = character.to_uppercase();
        let upper = match (upper.next(), upper.next()) {
            (Some(single), None) => single,
            _ => character,
        };
        if upper.is_ascii_uppercase() || upper.is_ascii_digit() || upper == '-' {
            result.push(upper);
        }
        if result.len() >= 60 {
            break;
        }
    }
    result
}

fn local_tracking(tracking_number: &str, now: DateTime<Utc>) -> Tracking {
    let now = now.with_timezone(&Utc);
    Tracking {
        tracking_number: tracking_number.to_owned(),
        status: "in_transit".to_owned(),
        events: vec![
            TrackingEvent {
                status: "created".to_owned(),
                raw_status: "Créé (sandbox local)".to_owned(),
                timestamp: rfc3339(now, -2),
            },
            TrackingEvent {
                status: "picked_up".to_owned(),
                raw_status: "Ramassé (sandbox local)".to_owned(),
                timestamp: rfc3339(now, -1),
            },
            TrackingEvent {
                status: "in_transit".to_owned(),
                raw_status: "En transit (sandbox local)".to_owned(),
                timestamp: rfc3339(now, 0),
            },
        ],
    }
}

fn rfc3339(now: DateTime<Utc>, hours: i64) -> String {
    let shifted = now
        .checked_add_signed(TimeDelta::hours(hours))
        .unwrap_or(now);
    shifted.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_carries_all_58_wilayas() {
        assert_eq!(fallback_wilayas().len(), 58);
    }

    #[test]
    fn commune_snapshots_are_grouped_by_wilaya() {
        assert_eq!(fallback_communes().len(), 1541);
        assert!(!fallback_communes_for(16).is_empty());
        assert!(fallback_communes_for(999).is_empty());
    }

    #[test]
    fn local_desks_use_the_wilaya_and_first_commune() {
        let desks = local_desks(16);
        assert_eq!(desks.len(), 1);
        assert_eq!(desks[0].id, "SBX-16");
        assert_eq!(desks[0].name, "Sandbox desk Alger");
        assert_eq!(desks[0].wilaya_code, 16);
        assert_eq!(desks[0].commune_name, "Ain Benian");
        assert_eq!(desks[0].address, "Sandbox address, Ain Benian");
    }

    #[test]
    fn local_desks_fall_back_for_an_unknown_wilaya() {
        let desks = local_desks(999);
        assert_eq!(desks.len(), 1);
        assert_eq!(desks[0].name, "Sandbox desk Wilaya 999");
        assert_eq!(desks[0].commune_name, "");
        assert_eq!(desks[0].address, "Sandbox address");
    }

    #[test]
    fn tracking_is_sanitized_and_capped_at_sixty_characters() {
        assert_eq!(sanitize_tracking(" az/2026-0041 é! "), "AZ2026-0041");
        assert_eq!(sanitize_tracking(""), "");
        let long = "a".repeat(100);
        assert_eq!(sanitize_tracking(&long), "A".repeat(60));
        assert_eq!(sanitize_tracking("ß"), "");
        assert_eq!(sanitize_tracking("ſ"), "S");
    }

    #[test]
    fn local_shipments_keep_the_reference_and_fall_back_when_empty() {
        let shipment = local_shipment(&OrderRequest {
            reference: "AZ-2026-0041".into(),
            ..OrderRequest::default()
        });
        assert_eq!(shipment.tracking_number, "SBX-AZ-2026-0041");
        assert_eq!(shipment.status, "created");
        assert_eq!(shipment.reference, "AZ-2026-0041");
        assert_eq!(shipment.courier_reference, "");

        let empty = local_shipment(&OrderRequest::default());
        assert_eq!(empty.tracking_number, "SBX-LOCAL");
    }

    #[test]
    fn local_tracking_uses_utc_second_precision() {
        let now = DateTime::parse_from_rfc3339("2026-11-11T10:00:00Z")
            .expect("fixed clock")
            .with_timezone(&Utc);
        let tracking = local_tracking("SBX-1", now);
        assert_eq!(tracking.status, "in_transit");
        assert_eq!(tracking.events.len(), 3);
        assert_eq!(tracking.events[0].timestamp, "2026-11-11T08:00:00Z");
        assert_eq!(tracking.events[1].timestamp, "2026-11-11T09:00:00Z");
        assert_eq!(tracking.events[2].timestamp, "2026-11-11T10:00:00Z");
    }
}

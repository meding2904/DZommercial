//! Wire and domain types shared by the delivery map, the parcel client and the
//! dzship adapter.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Wilaya {
    pub code: i32,
    pub name_ar: String,
    pub name_fr: String,
    pub is_deep_south: bool,
    pub commune_count: i32,
    pub courier_supported: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Commune {
    pub wilaya_code: i32,
    pub name_fr: String,
    pub name_ar: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Desk {
    pub id: String,
    pub name: String,
    pub wilaya_code: i32,
    pub commune_name: String,
    pub address: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shipment {
    pub tracking_number: String,
    pub courier_reference: String,
    pub status: String,
    pub reference: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackingEvent {
    pub status: String,
    pub raw_status: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tracking {
    pub tracking_number: String,
    pub status: String,
    pub events: Vec<TrackingEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recipient {
    pub full_name: String,
    pub phone: String,
    pub phone_alt: String,
    pub wilaya_code: i32,
    pub commune_name: String,
    pub address_line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderRequest {
    pub reference: String,
    pub recipient: Recipient,
    pub delivery_type: String,
    pub stop_desk_id: String,
    pub product_list: String,
    pub cod_amount: i64,
    pub free_shipping: bool,
    pub notes: String,
}

/// A rejection or server failure returned by the courier API. `status`, `code`,
/// `fields` and `retry_after` are carried through untouched so the HTTP layer
/// can render the same response the Go service did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct APIError {
    pub status: i32,
    pub code: String,
    pub message: String,
    pub fields: BTreeMap<String, String>,
    pub retry_after: Duration,
}

impl fmt::Display for APIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "courier returned HTTP {}", self.status)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for APIError {}

/// The detail attached to [`Error::Transport`]. Keeping it optional preserves
/// the bare `courier transport` sentinel message and the wrapped
/// `courier transport: <detail>` form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportError {
    detail: Option<String>,
}

impl TransportError {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_detail(detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
        }
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail.as_deref() {
            Some(detail) if !detail.is_empty() => write!(f, "courier transport: {detail}"),
            _ => f.write_str("courier transport"),
        }
    }
}

impl std::error::Error for TransportError {}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("unknown wilaya")]
    UnknownWilaya,
    #[error("unknown commune")]
    UnknownCommune,
    #[error("unknown stop desk")]
    UnknownDesk,
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Api(#[from] APIError),
    #[error("no bundled snapshot for this resource")]
    NoSnapshot,
    #[error("encode courier request: {0}")]
    Encode(String),
    #[error("build courier request: {0}")]
    BuildRequest(String),
}

impl Error {
    pub fn transport(detail: impl Into<String>) -> Self {
        Self::Transport(TransportError::with_detail(detail))
    }

    pub fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    pub fn api(&self) -> Option<&APIError> {
        match self {
            Self::Api(error) => Some(error),
            _ => None,
        }
    }

    /// Mirrors `isUnavailable` in the Go resolver: a transport failure or any
    /// 5xx is worth falling back on, a 4xx rejection is not.
    pub fn is_unavailable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api(error) => error.status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display_prefers_the_message() {
        let error = APIError {
            status: 422,
            message: "Invalid phone.".into(),
            ..APIError::default()
        };
        assert_eq!(error.to_string(), "Invalid phone.");
    }

    #[test]
    fn api_error_display_falls_back_to_the_status() {
        let error = APIError {
            status: 502,
            ..APIError::default()
        };
        assert_eq!(error.to_string(), "courier returned HTTP 502");
    }

    #[test]
    fn transport_error_display_has_bare_and_detailed_forms() {
        assert_eq!(TransportError::new().to_string(), "courier transport");
        assert_eq!(
            TransportError::with_detail("dial tcp: no route to host").to_string(),
            "courier transport: dial tcp: no route to host"
        );
    }

    #[test]
    fn error_unavailable_matches_transport_and_5xx_only() {
        assert!(Error::Transport(TransportError::new()).is_unavailable());
        assert!(
            Error::Api(APIError {
                status: 503,
                ..APIError::default()
            })
            .is_unavailable()
        );
        assert!(
            !Error::Api(APIError {
                status: 422,
                ..APIError::default()
            })
            .is_unavailable()
        );
        assert!(!Error::UnknownWilaya.is_unavailable());
    }
}

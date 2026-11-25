//! Refusals the HTTP layer renders as problem responses.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problem {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub fields: Option<HashMap<String, String>>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Problem {}

pub fn refuse(status: u16, code: &str, message: &str) -> Problem {
    Problem {
        status,
        code: code.to_string(),
        message: message.to_string(),
        fields: None,
    }
}

pub fn refuse_fields(
    status: u16,
    code: &str,
    message: &str,
    fields: HashMap<String, String>,
) -> Problem {
    Problem {
        status,
        code: code.to_string(),
        message: message.to_string(),
        fields: Some(fields),
    }
}

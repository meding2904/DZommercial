//! Environment-driven configuration, ported from Go's `internal/config`.

mod gourl;
mod jsonmap;

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

const SECOND_NS: i64 = 1_000_000_000;
const HOUR_NS: i64 = 3_600 * SECOND_NS;
const MILLI_NS: i64 = 1_000_000;

/// A configuration loading failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigError(&'static str);

impl ConfigError {
    /// The exact, user-facing error text.
    pub fn message(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ConfigError {}

const ERR_APP_ENV_MISSING: ConfigError =
    ConfigError("APP_ENV must be set to development or production");
const ERR_APP_ENV_UNKNOWN: ConfigError = ConfigError("APP_ENV must be development or production");
const ERR_TURSO_URL: ConfigError =
    ConfigError("TURSO_DATABASE_URL must be set when APP_ENV=production");
const ERR_TURSO_TOKEN: ConfigError =
    ConfigError("TURSO_AUTH_TOKEN must be set when APP_ENV=production");
const ERR_APP_ORIGIN_REQUIRED: ConfigError =
    ConfigError("APP_ORIGIN must be set when APP_ENV=production");
const ERR_ADMIN_EMAIL: ConfigError = ConfigError("ADMIN_EMAIL must be set when APP_ENV=production");
const ERR_ADMIN_PASSWORD_REQUIRED: ConfigError =
    ConfigError("ADMIN_PASSWORD must be set when APP_ENV=production");
const ERR_ADMIN_PASSWORD_DEFAULT: ConfigError =
    ConfigError("ADMIN_PASSWORD must not be the development default when APP_ENV=production");
const ERR_ADMIN_PASSWORD_SHORT: ConfigError =
    ConfigError("ADMIN_PASSWORD must be at least 14 characters when APP_ENV=production");
const ERR_CUSTOMER_PASSWORD_DEFAULT: ConfigError =
    ConfigError("CUSTOMER_PASSWORD must not be the development default when SEED_DEMO=true");
const ERR_EMPLOYEE_PASSWORD_DEFAULT: ConfigError =
    ConfigError("EMPLOYEE_PASSWORD must not be the development default when SEED_DEMO=true");
const ERR_EMPLOYEE_PASSWORD_SHORT: ConfigError =
    ConfigError("EMPLOYEE_PASSWORD must be at least 10 characters when SEED_DEMO=true");
const ERR_TURNSTILE_REQUIRED: ConfigError =
    ConfigError("TURNSTILE_SECRET_KEY must be set when APP_ENV=production");
const ERR_TURNSTILE_TEST_KEY: ConfigError =
    ConfigError("TURNSTILE_SECRET_KEY must not be a Turnstile test key");
const ERR_PLUGIN_SECRET_REQUIRED: ConfigError =
    ConfigError("PLUGIN_SECRET_KEY must be set to at least 16 characters when APP_ENV=production");
const ERR_PLUGIN_SECRET_DEFAULT: ConfigError =
    ConfigError("PLUGIN_SECRET_KEY must not be the development default when APP_ENV=production");
const ERR_APP_ORIGIN_INVALID: ConfigError =
    ConfigError("APP_ORIGIN must be an absolute http(s) URL");
const ERR_SESSION_TTL_MAX: ConfigError = ConfigError("SESSION_TTL_HOURS must be 720 or less");
const ERR_API_ADDR: ConfigError = ConfigError("API_ADDR must not be empty");
const ERR_COURIER_URL: ConfigError = ConfigError("COURIER_API_URL must be an absolute http(s) URL");
const ERR_COURIER_HTTPS: ConfigError =
    ConfigError("COURIER_API_URL must use https when APP_ENV=production");
const ERR_COURIER_KEY: ConfigError = ConfigError("COURIER_KEY must be between 1 and 64 characters");
const ERR_COURIER_WILAYA: ConfigError = ConfigError("COURIER_FROM_WILAYA must be between 1 and 58");
const ERR_COURIER_TIMEOUT: ConfigError =
    ConfigError("COURIER_TIMEOUT_SECONDS must be between 1 and 30");
const ERR_PLUGIN_SECRET_SHORT: ConfigError =
    ConfigError("PLUGIN_SECRET_KEY must be at least 16 characters");
const ERR_AI_URL: ConfigError = ConfigError("AI_BASE_URL must be an absolute http(s) URL");
const ERR_AI_CREDENTIALS: ConfigError = ConfigError("AI_BASE_URL must not contain credentials");
const ERR_AI_HTTPS: ConfigError = ConfigError("AI_BASE_URL must use https when APP_ENV=production");
const ERR_AI_TIMEOUT: ConfigError = ConfigError("AI_TIMEOUT_SECONDS must be between 5 and 60");

const DEFAULT_ADMIN_PASSWORD: &str = "azahar-dev-admin-2026";
const DEFAULT_CUSTOMER_PASSWORD: &str = "azahar-dev-client-2026";
const DEFAULT_EMPLOYEE_PASSWORD: &str = "azahar-dev-staff-2026";
const DEFAULT_PLUGIN_SECRET_KEY: &str = "azahar-dev-plugin-key-2026";

/// Runtime configuration, loaded from the process environment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub app_env: String,
    pub api_addr: String,
    pub app_origin: String,
    pub turso_url: String,
    pub turso_token: String,
    pub cookie_secure: bool,
    pub trust_proxy: bool,
    pub seed_demo: bool,
    pub admin_email: String,
    pub admin_password: String,
    pub customer_email: String,
    pub customer_password: String,
    pub employee_email: String,
    pub employee_password: String,
    pub session_ttl: Duration,
    pub courier_api_url: String,
    pub courier_key: String,
    pub courier_credentials: String,
    pub courier_from_wilaya: i64,
    pub courier_timeout: Duration,
    pub turnstile_secret: String,
    pub plugin_secret_key: String,
    pub ai_base_url: String,
    pub ai_timeout: Duration,
    pub audit_batch_interval: Duration,
}

impl Config {
    /// Load configuration from the real process environment.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(|key| {
            std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
        })
    }

    /// Load configuration from a lookup function.
    ///
    /// This is the seam used by tests to model `os.LookupEnv`: `None` means the
    /// variable is unset, `Some("")` means it is set but empty.
    pub fn load_from<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let raw_app_env = match lookup("APP_ENV") {
            Some(value) if !value.trim().is_empty() => value,
            _ => return Err(ERR_APP_ENV_MISSING),
        };
        let app_env = go_to_lower(raw_app_env.trim());
        if app_env != "development" && app_env != "production" {
            return Err(ERR_APP_ENV_UNKNOWN);
        }
        let is_production = app_env == "production";

        let api_addr = getenv(&lookup, "API_ADDR", ":8080");
        let app_origin = getenv(&lookup, "APP_ORIGIN", "http://localhost:5173")
            .trim_end_matches('/')
            .to_string();
        let turso_url = getenv(&lookup, "TURSO_DATABASE_URL", "http://127.0.0.1:8880");
        let turso_token = lookup("TURSO_AUTH_TOKEN").unwrap_or_default();
        let admin_email =
            go_to_lower(getenv(&lookup, "ADMIN_EMAIL", "admin@azahar.example").trim());
        let admin_password = getenv(&lookup, "ADMIN_PASSWORD", DEFAULT_ADMIN_PASSWORD);
        let customer_email =
            go_to_lower(getenv(&lookup, "CUSTOMER_EMAIL", "client@azahar.example").trim());
        let customer_password = getenv(&lookup, "CUSTOMER_PASSWORD", DEFAULT_CUSTOMER_PASSWORD);
        let employee_email =
            go_to_lower(getenv(&lookup, "EMPLOYEE_EMAIL", "staff@azahar.example").trim());
        let employee_password = getenv(&lookup, "EMPLOYEE_PASSWORD", DEFAULT_EMPLOYEE_PASSWORD);
        let mut session_ttl_ns = getint(&lookup, "SESSION_TTL_HOURS", 168).wrapping_mul(HOUR_NS);

        let courier_api_url = getenv(&lookup, "COURIER_API_URL", "https://freeship.dzbuild.com")
            .trim()
            .trim_end_matches('/')
            .to_string();
        let courier_key = go_to_lower(getenv(&lookup, "COURIER_KEY", "sandbox").trim());
        let courier_credentials = lookup("COURIER_CREDENTIALS")
            .unwrap_or_default()
            .trim()
            .to_string();
        let courier_from_wilaya = getint(&lookup, "COURIER_FROM_WILAYA", 16);
        let courier_timeout_ns =
            getint(&lookup, "COURIER_TIMEOUT_SECONDS", 12).wrapping_mul(SECOND_NS);
        let turnstile_secret = lookup("TURNSTILE_SECRET_KEY")
            .unwrap_or_default()
            .trim()
            .to_string();
        let plugin_secret_key = getenv(&lookup, "PLUGIN_SECRET_KEY", DEFAULT_PLUGIN_SECRET_KEY);
        let ai_base_url = getenv(&lookup, "AI_BASE_URL", "https://api.openai.com/v1")
            .trim()
            .trim_end_matches('/')
            .to_string();
        let ai_timeout_ns = getint(&lookup, "AI_TIMEOUT_SECONDS", 25).wrapping_mul(SECOND_NS);
        let mut audit_batch_interval_ns =
            getint(&lookup, "AUDIT_BATCH_INTERVAL_MS", 2000).wrapping_mul(MILLI_NS);

        let seed_demo = getbool(&lookup, "SEED_DEMO", app_env == "development");
        let mut cookie_secure = getbool(&lookup, "COOKIE_SECURE", is_production);
        let trust_proxy = getbool(&lookup, "TRUST_PROXY", false);

        if is_production {
            if lookup("TURSO_DATABASE_URL").is_none() || turso_url.trim().is_empty() {
                return Err(ERR_TURSO_URL);
            }
            if lookup("TURSO_AUTH_TOKEN").is_none() || turso_token.trim().is_empty() {
                return Err(ERR_TURSO_TOKEN);
            }
            if lookup("APP_ORIGIN").is_none() || app_origin.trim().is_empty() {
                return Err(ERR_APP_ORIGIN_REQUIRED);
            }
            if lookup("ADMIN_EMAIL").is_none() || admin_email.trim().is_empty() {
                return Err(ERR_ADMIN_EMAIL);
            }
            if lookup("ADMIN_PASSWORD").is_none() || admin_password.trim().is_empty() {
                return Err(ERR_ADMIN_PASSWORD_REQUIRED);
            }
            if admin_password == DEFAULT_ADMIN_PASSWORD {
                return Err(ERR_ADMIN_PASSWORD_DEFAULT);
            }
            if admin_password.len() < 14 {
                return Err(ERR_ADMIN_PASSWORD_SHORT);
            }
            if seed_demo && customer_password == DEFAULT_CUSTOMER_PASSWORD {
                return Err(ERR_CUSTOMER_PASSWORD_DEFAULT);
            }
            if seed_demo && employee_password == DEFAULT_EMPLOYEE_PASSWORD {
                return Err(ERR_EMPLOYEE_PASSWORD_DEFAULT);
            }
            if seed_demo && employee_password.len() < 10 {
                return Err(ERR_EMPLOYEE_PASSWORD_SHORT);
            }
            if turnstile_secret.is_empty() {
                return Err(ERR_TURNSTILE_REQUIRED);
            }
            if turnstile_secret.starts_with("1x")
                || turnstile_secret.starts_with("2x")
                || turnstile_secret.starts_with("3x")
            {
                return Err(ERR_TURNSTILE_TEST_KEY);
            }
            if lookup("PLUGIN_SECRET_KEY").is_none() || plugin_secret_key.len() < 16 {
                return Err(ERR_PLUGIN_SECRET_REQUIRED);
            }
            if plugin_secret_key == DEFAULT_PLUGIN_SECRET_KEY {
                return Err(ERR_PLUGIN_SECRET_DEFAULT);
            }
            cookie_secure = true;
        }

        if !app_origin.is_empty() && !is_absolute_http_url(&app_origin) {
            return Err(ERR_APP_ORIGIN_INVALID);
        }
        if session_ttl_ns <= 0 {
            session_ttl_ns = 168 * HOUR_NS;
        }
        if session_ttl_ns > 720 * HOUR_NS {
            return Err(ERR_SESSION_TTL_MAX);
        }
        if api_addr.trim().is_empty() {
            return Err(ERR_API_ADDR);
        }

        let Ok(courier_url) = gourl::parse(&courier_api_url) else {
            return Err(ERR_COURIER_URL);
        };
        if !is_http_scheme(courier_url.scheme()) || courier_url.host().is_empty() {
            return Err(ERR_COURIER_URL);
        }
        if is_production && courier_url.scheme() != b"https" {
            return Err(ERR_COURIER_HTTPS);
        }
        if courier_key.is_empty() || courier_key.len() > 64 {
            return Err(ERR_COURIER_KEY);
        }
        if !(1..=58).contains(&courier_from_wilaya) {
            return Err(ERR_COURIER_WILAYA);
        }
        if !(SECOND_NS..=30 * SECOND_NS).contains(&courier_timeout_ns) {
            return Err(ERR_COURIER_TIMEOUT);
        }
        if plugin_secret_key.len() < 16 {
            return Err(ERR_PLUGIN_SECRET_SHORT);
        }

        let Ok(ai_url) = gourl::parse(&ai_base_url) else {
            return Err(ERR_AI_URL);
        };
        if !is_http_scheme(ai_url.scheme()) || ai_url.host().is_empty() {
            return Err(ERR_AI_URL);
        }
        if ai_url.has_user() {
            return Err(ERR_AI_CREDENTIALS);
        }
        if is_production && ai_url.scheme() != b"https" {
            return Err(ERR_AI_HTTPS);
        }
        if !(5 * SECOND_NS..=60 * SECOND_NS).contains(&ai_timeout_ns) {
            return Err(ERR_AI_TIMEOUT);
        }

        audit_batch_interval_ns = audit_batch_interval_ns.clamp(500 * MILLI_NS, 60 * SECOND_NS);

        Ok(Self {
            app_env,
            api_addr,
            app_origin,
            turso_url,
            turso_token,
            cookie_secure,
            trust_proxy,
            seed_demo,
            admin_email,
            admin_password,
            customer_email,
            customer_password,
            employee_email,
            employee_password,
            session_ttl: ns_to_duration(session_ttl_ns),
            courier_api_url,
            courier_key,
            courier_credentials,
            courier_from_wilaya,
            courier_timeout: ns_to_duration(courier_timeout_ns),
            turnstile_secret,
            plugin_secret_key,
            ai_base_url,
            ai_timeout: ns_to_duration(ai_timeout_ns),
            audit_batch_interval: ns_to_duration(audit_batch_interval_ns),
        })
    }

    /// The libsql DSN, with the auth token added when one is configured.
    pub fn dsn(&self) -> String {
        if self.turso_token.is_empty() {
            return self.turso_url.clone();
        }
        let Ok(mut url) = gourl::parse(&self.turso_url) else {
            return self.turso_url.clone();
        };
        let mut query = url.query();
        let has_token = query
            .get(b"authToken".as_slice())
            .and_then(|values| values.first())
            .is_some_and(|value| !value.is_empty());
        if !has_token {
            query.insert(
                b"authToken".to_vec(),
                vec![self.turso_token.as_bytes().to_vec()],
            );
        }
        url.set_raw_query(gourl::encode_query(&query));
        url.string()
    }

    /// Decode `COURIER_CREDENTIALS`.
    ///
    /// The second value reports whether the raw value parsed. A JSON `null`
    /// yields `None` with `true`, matching Go's nil map. Values that are not a
    /// JSON object yield an empty map with `false`.
    pub fn courier_credential_values(&self) -> (Option<HashMap<String, String>>, bool) {
        let raw = self.courier_credentials.trim();
        if raw.is_empty() {
            return (Some(HashMap::new()), true);
        }
        match jsonmap::parse_credentials_object(raw) {
            Ok(values) => (values, true),
            Err(()) => (Some(HashMap::new()), false),
        }
    }

    pub fn is_production(&self) -> bool {
        self.app_env.eq_ignore_ascii_case("production")
    }

    pub fn is_development(&self) -> bool {
        self.app_env.eq_ignore_ascii_case("development")
    }

    pub fn admin_session_ttl(&self) -> Duration {
        if self.session_ttl < 12 * Duration::from_secs(3600) {
            self.session_ttl
        } else {
            12 * Duration::from_secs(3600)
        }
    }
}

fn getenv<F: Fn(&str) -> Option<String>>(lookup: &F, key: &str, fallback: &str) -> String {
    lookup(key).unwrap_or_else(|| fallback.to_string())
}

fn getbool<F: Fn(&str) -> Option<String>>(lookup: &F, key: &str, fallback: bool) -> bool {
    let Some(value) = lookup(key) else {
        return fallback;
    };
    let value = value.trim();
    if value.is_empty() {
        return fallback;
    }
    parse_go_bool(value).unwrap_or(fallback)
}

fn getint<F: Fn(&str) -> Option<String>>(lookup: &F, key: &str, fallback: i64) -> i64 {
    let Some(value) = lookup(key) else {
        return fallback;
    };
    let value = value.trim();
    if value.is_empty() {
        return fallback;
    }
    value.parse::<i64>().unwrap_or(fallback)
}

/// `strconv.ParseBool` accepts exactly these spellings.
fn parse_go_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// Go's `strings.ToLower` applies the one-rune simple lowercase mapping. Rust's
/// full mapping only differs by expanding U+0130 to two code points.
fn go_to_lower(value: &str) -> String {
    value
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

fn is_http_scheme(scheme: &[u8]) -> bool {
    scheme == b"http" || scheme == b"https"
}

fn is_absolute_http_url(value: &str) -> bool {
    gourl::parse(value).is_ok_and(|url| is_http_scheme(url.scheme()) && !url.host().is_empty())
}

fn ns_to_duration(nanos: i64) -> Duration {
    Duration::from_nanos(nanos.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_spellings_match_strconv() {
        for value in ["1", "t", "T", "TRUE", "true", "True"] {
            assert_eq!(parse_go_bool(value), Some(true), "{value}");
        }
        for value in ["0", "f", "F", "FALSE", "false", "False"] {
            assert_eq!(parse_go_bool(value), Some(false), "{value}");
        }
        for value in ["", "yes", "tRuE", "2", " true"] {
            assert_eq!(parse_go_bool(value), None, "{value}");
        }
    }

    #[test]
    fn lower_matches_go_simple_mapping() {
        assert_eq!(go_to_lower("ADMİN"), "admin");
        assert_eq!(go_to_lower("A B"), "a b");
    }
}

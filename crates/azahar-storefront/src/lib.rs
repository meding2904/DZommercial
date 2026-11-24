//! Storefront theme: brand copy, typography, palette and hero image.
//! Values are validated so the theme can be applied as CSS variables without
//! injecting arbitrary CSS.

mod json;

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    message: String,
}

impl ThemeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ThemeError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    pub ground: String,
    pub surface: String,
    pub ink: String,
    pub muted: String,
    pub accent: String,
    pub accent_ink: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub brand_name: String,
    pub tagline: String,
    pub description: String,
    pub hero_heading: String,
    pub hero_body: String,
    pub hero_alt: String,
    pub hero_cta: String,
    pub hero_image: String,
    pub statement: String,
    pub shelf_heading: String,
    pub shop_title: String,
    pub house_label: String,
    pub about_heading: String,
    pub about_body: String,
    pub about_image: String,
    pub about_alt: String,
    pub closing_heading: String,
    pub footer_note: String,
    pub footer_credit: String,
    pub contact_email: String,
    pub announcement: String,
    pub concerns_enabled: bool,
    pub font_display: String,
    pub font_body: String,
    pub font_scale: f64,
    pub radius: i64,
    pub light: Palette,
    pub dark: Palette,
    pub updated_at: String,
}

const DISPLAY_FONTS: [&str; 4] = ["schibsted", "source_serif", "fraunces", "system"];
const BODY_FONTS: [&str; 4] = ["schibsted", "source_serif", "inter", "system"];

const THEME_FIELDS: [&str; 29] = [
    "brand_name",
    "tagline",
    "description",
    "hero_heading",
    "hero_body",
    "hero_alt",
    "hero_cta",
    "hero_image",
    "statement",
    "shelf_heading",
    "shop_title",
    "house_label",
    "about_heading",
    "about_body",
    "about_image",
    "about_alt",
    "closing_heading",
    "footer_note",
    "footer_credit",
    "contact_email",
    "announcement",
    "concerns_enabled",
    "font_display",
    "font_body",
    "font_scale",
    "radius",
    "light",
    "dark",
    "updated_at",
];

const PALETTE_FIELDS: [&str; 6] = ["ground", "surface", "ink", "muted", "accent", "accent_ink"];

impl Default for Theme {
    fn default() -> Self {
        Self {
            brand_name: "Azahar".to_string(),
            tagline: "Botanical skincare from the Mediterranean edge.".to_string(),
            description: "Oils, clays and blossoms, blended in small batches in Algiers."
                .to_string(),
            hero_heading: "Botanical skincare from the Mediterranean edge.".to_string(),
            hero_body: "Oils, clays and blossoms, blended in small batches in Algiers. Eight formulas for the face, the body and the bath.".to_string(),
            hero_alt: "Amber glass bottles and a linen cloth on a plaster shelf".to_string(),
            hero_cta: "Shop the shelf".to_string(),
            hero_image: "/images/hero-1600.webp".to_string(),
            statement: "We make fewer things, with more of the plant in them. The shelf is short on purpose.".to_string(),
            shelf_heading: "Eight formulas for face, body and the bath.".to_string(),
            shop_title: "The shelf".to_string(),
            house_label: "The house".to_string(),
            about_heading: "A small shelf in Algiers.".to_string(),
            about_body: "Azahar began with the orange blossom that scents the city in spring. We press olives and figs, mix clays with rose, and pour everything into amber glass. The shelf is short on purpose.".to_string(),
            about_image: "/images/atelier-1200.webp".to_string(),
            about_alt: "Amber bottles, ceramic jars, linen and dried botanicals on a work table"
                .to_string(),
            closing_heading: "Take the whole shelf, or one jar.".to_string(),
            footer_note: "Botanical skincare from the Mediterranean edge. Oils, clays and blossoms, blended in small batches in Algiers.".to_string(),
            footer_credit: "Photography from Pexels. Products, prices and story are sample data."
                .to_string(),
            contact_email: "hello@azahar.example".to_string(),
            announcement: "Cash on delivery across Algeria.".to_string(),
            concerns_enabled: true,
            font_display: "schibsted".to_string(),
            font_body: "schibsted".to_string(),
            font_scale: 1.0,
            radius: 4,
            light: Palette {
                ground: "#f6f0eb".to_string(),
                surface: "#ebe0d9".to_string(),
                ink: "#2c2325".to_string(),
                muted: "#6a5d5e".to_string(),
                accent: "#9a3c4c".to_string(),
                accent_ink: "#ffffff".to_string(),
            },
            dark: Palette {
                ground: "#171311".to_string(),
                surface: "#221b18".to_string(),
                ink: "#f4ece5".to_string(),
                muted: "#b9aca4".to_string(),
                accent: "#e0a0a8".to_string(),
                accent_ink: "#241316".to_string(),
            },
            updated_at: String::new(),
        }
    }
}

pub fn parse(raw: &str) -> Result<Theme, ThemeError> {
    let mut theme = Theme::default();
    if raw.trim().is_empty() {
        return Ok(theme);
    }
    let value = json::read_value(raw)
        .map_err(|err| ThemeError::new(format!("read storefront theme: {}", err.message)))?;
    decode_theme(&mut theme, value)
        .map_err(|message| ThemeError::new(format!("read storefront theme: {message}")))?;
    Ok(normalize(&theme))
}

pub fn normalize(theme: &Theme) -> Theme {
    let mut theme = theme.clone();
    theme.brand_name = trim(&theme.brand_name);
    theme.tagline = trim(&theme.tagline);
    theme.description = trim(&theme.description);
    theme.hero_heading = trim(&theme.hero_heading);
    theme.hero_body = trim(&theme.hero_body);
    theme.hero_alt = trim(&theme.hero_alt);
    theme.hero_cta = trim(&theme.hero_cta);
    theme.hero_image = trim(&theme.hero_image);
    theme.statement = trim(&theme.statement);
    theme.shelf_heading = trim(&theme.shelf_heading);
    theme.shop_title = trim(&theme.shop_title);
    theme.house_label = trim(&theme.house_label);
    theme.about_heading = trim(&theme.about_heading);
    theme.about_body = trim(&theme.about_body);
    theme.about_image = trim(&theme.about_image);
    theme.about_alt = trim(&theme.about_alt);
    theme.closing_heading = trim(&theme.closing_heading);
    theme.footer_note = trim(&theme.footer_note);
    theme.footer_credit = trim(&theme.footer_credit);
    theme.contact_email = go_lower(&trim(&theme.contact_email));
    theme.announcement = trim(&theme.announcement);
    theme.font_display = go_lower(&trim(&theme.font_display));
    theme.font_body = go_lower(&trim(&theme.font_body));
    theme.light = normalize_palette(&theme.light);
    theme.dark = normalize_palette(&theme.dark);
    if theme.font_scale == 0.0 {
        theme.font_scale = 1.0;
    }
    theme
}

fn normalize_palette(palette: &Palette) -> Palette {
    Palette {
        ground: go_lower(&trim(&palette.ground)),
        surface: go_lower(&trim(&palette.surface)),
        ink: go_lower(&trim(&palette.ink)),
        muted: go_lower(&trim(&palette.muted)),
        accent: go_lower(&trim(&palette.accent)),
        accent_ink: go_lower(&trim(&palette.accent_ink)),
    }
}

fn trim(value: &str) -> String {
    value.trim().to_string()
}

fn go_lower(value: &str) -> String {
    // Go lowercases one rune at a time; Rust's full mapping can expand
    // (U+0130 becomes "i" plus a combining dot), so keep the simple form.
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii() {
                ch.to_ascii_lowercase()
            } else {
                ch.to_lowercase().next().unwrap_or(ch)
            }
        })
        .collect()
}

pub fn encode(theme: &Theme) -> Result<String, ThemeError> {
    let mut out = StringEncoder {
        out: String::with_capacity(2048),
        first: true,
    };
    out.open();
    out.string_field("brand_name", &theme.brand_name);
    out.string_field("tagline", &theme.tagline);
    out.string_field("description", &theme.description);
    out.string_field("hero_heading", &theme.hero_heading);
    out.string_field("hero_body", &theme.hero_body);
    out.string_field("hero_alt", &theme.hero_alt);
    out.string_field("hero_cta", &theme.hero_cta);
    out.string_field("hero_image", &theme.hero_image);
    out.string_field("statement", &theme.statement);
    out.string_field("shelf_heading", &theme.shelf_heading);
    out.string_field("shop_title", &theme.shop_title);
    out.string_field("house_label", &theme.house_label);
    out.string_field("about_heading", &theme.about_heading);
    out.string_field("about_body", &theme.about_body);
    out.string_field("about_image", &theme.about_image);
    out.string_field("about_alt", &theme.about_alt);
    out.string_field("closing_heading", &theme.closing_heading);
    out.string_field("footer_note", &theme.footer_note);
    out.string_field("footer_credit", &theme.footer_credit);
    out.string_field("contact_email", &theme.contact_email);
    out.string_field("announcement", &theme.announcement);
    out.bool_field("concerns_enabled", theme.concerns_enabled);
    out.string_field("font_display", &theme.font_display);
    out.string_field("font_body", &theme.font_body);
    out.float_field("font_scale", theme.font_scale)?;
    out.int_field("radius", theme.radius);
    out.palette_field("light", &theme.light);
    out.palette_field("dark", &theme.dark);
    if !theme.updated_at.is_empty() {
        out.string_field("updated_at", &theme.updated_at);
    }
    out.close();
    Ok(out.finish())
}

#[derive(Default)]
struct StringEncoder {
    out: String,
    first: bool,
}

impl StringEncoder {
    fn open(&mut self) {
        self.out.push('{');
    }

    fn close(&mut self) {
        self.out.push('}');
    }

    fn finish(self) -> String {
        self.out
    }

    fn begin(&mut self, name: &str) {
        if self.first {
            self.first = false;
        } else {
            self.out.push(',');
        }
        push_json_string(&mut self.out, name);
        self.out.push(':');
    }

    fn string_field(&mut self, name: &str, value: &str) {
        self.begin(name);
        push_json_string(&mut self.out, value);
    }

    fn bool_field(&mut self, name: &str, value: bool) {
        self.begin(name);
        self.out.push_str(if value { "true" } else { "false" });
    }

    fn int_field(&mut self, name: &str, value: i64) {
        self.begin(name);
        self.out.push_str(&value.to_string());
    }

    fn float_field(&mut self, name: &str, value: f64) -> Result<(), ThemeError> {
        let encoded = encode_float(value)?;
        self.begin(name);
        self.out.push_str(&encoded);
        Ok(())
    }

    fn palette_field(&mut self, name: &str, palette: &Palette) {
        self.begin(name);
        self.out.push('{');
        let mut inner = StringEncoder {
            out: String::new(),
            first: true,
        };
        inner.string_field("ground", &palette.ground);
        inner.string_field("surface", &palette.surface);
        inner.string_field("ink", &palette.ink);
        inner.string_field("muted", &palette.muted);
        inner.string_field("accent", &palette.accent);
        inner.string_field("accent_ink", &palette.accent_ink);
        self.out.push_str(&inner.finish());
        self.out.push('}');
    }
}

fn encode_float(value: f64) -> Result<String, ThemeError> {
    if value.is_nan() {
        return Err(ThemeError::new("json: unsupported value: NaN"));
    }
    if value.is_infinite() {
        let rendered = if value > 0.0 { "+Inf" } else { "-Inf" };
        return Err(ThemeError::new(format!(
            "json: unsupported value: {rendered}"
        )));
    }
    let magnitude = value.abs();
    // encoding/json switches to exponent notation outside [1e-6, 1e21).
    if magnitude != 0.0 && (magnitude < 1e-6 || magnitude >= 1e21) {
        let rendered = format!("{value:e}");
        let mut out = String::with_capacity(rendered.len() + 1);
        let mut exponent = false;
        for (index, ch) in rendered.char_indices() {
            out.push(ch);
            if exponent {
                exponent = false;
            } else if ch == 'e' {
                exponent = true;
                let next = rendered[index + ch.len_utf8()..].chars().next();
                if !matches!(next, Some('+' | '-')) {
                    out.push('+');
                }
            }
        }
        Ok(out)
    } else {
        Ok(format!("{value}"))
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

pub fn validate(theme: &Theme) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();

    validate_text(&mut fields, "brand_name", &theme.brand_name, 1, 40);
    validate_text(&mut fields, "tagline", &theme.tagline, 1, 120);
    validate_optional(&mut fields, "description", &theme.description, 400);
    validate_text(&mut fields, "hero_heading", &theme.hero_heading, 1, 120);
    validate_optional(&mut fields, "hero_body", &theme.hero_body, 500);
    validate_optional(&mut fields, "hero_alt", &theme.hero_alt, 160);
    validate_text(&mut fields, "hero_cta", &theme.hero_cta, 1, 60);
    validate_text(&mut fields, "statement", &theme.statement, 1, 220);
    validate_text(&mut fields, "shelf_heading", &theme.shelf_heading, 1, 120);
    validate_text(&mut fields, "shop_title", &theme.shop_title, 1, 80);
    validate_text(&mut fields, "house_label", &theme.house_label, 1, 40);
    validate_optional(&mut fields, "about_heading", &theme.about_heading, 120);
    validate_optional(&mut fields, "about_body", &theme.about_body, 2000);
    validate_optional(&mut fields, "about_alt", &theme.about_alt, 160);
    validate_text(
        &mut fields,
        "closing_heading",
        &theme.closing_heading,
        1,
        120,
    );
    validate_optional(&mut fields, "footer_note", &theme.footer_note, 300);
    validate_optional(&mut fields, "footer_credit", &theme.footer_credit, 300);
    validate_optional(&mut fields, "announcement", &theme.announcement, 140);

    if !valid_image_path(&theme.hero_image) {
        fields.insert(
            "hero_image".to_string(),
            "Use an /images/ path.".to_string(),
        );
    }
    if !valid_image_path(&theme.about_image) {
        fields.insert(
            "about_image".to_string(),
            "Use an /images/ path.".to_string(),
        );
    }
    if !valid_email(&theme.contact_email) || theme.contact_email.len() > 254 {
        fields.insert(
            "contact_email".to_string(),
            "Enter a valid email address.".to_string(),
        );
    }

    if !contains(&DISPLAY_FONTS, &theme.font_display) {
        fields.insert(
            "font_display".to_string(),
            "Choose one of the available display fonts.".to_string(),
        );
    }
    if !contains(&BODY_FONTS, &theme.font_body) {
        fields.insert(
            "font_body".to_string(),
            "Choose one of the available text fonts.".to_string(),
        );
    }
    if theme.font_scale < 0.85 || theme.font_scale > 1.15 {
        fields.insert(
            "font_scale".to_string(),
            "Use a body text scale between 0.85 and 1.15.".to_string(),
        );
    }
    if theme.radius < 0 || theme.radius > 12 {
        fields.insert(
            "radius".to_string(),
            "Use a corner radius between 0 and 12.".to_string(),
        );
    }

    validate_palette("light", &theme.light, &mut fields);
    validate_palette("dark", &theme.dark, &mut fields);
    fields
}

fn validate_text(
    fields: &mut BTreeMap<String, String>,
    key: &str,
    value: &str,
    min: usize,
    max: usize,
) {
    let length = value.chars().count();
    if length < min {
        fields.insert(key.to_string(), format!("Use at least {min} characters."));
        return;
    }
    if length > max {
        fields.insert(
            key.to_string(),
            format!("Keep this under {} characters.", max + 1),
        );
    }
    if has_control(value) {
        fields.insert(
            key.to_string(),
            "Use plain text without control characters.".to_string(),
        );
    }
}

fn validate_optional(fields: &mut BTreeMap<String, String>, key: &str, value: &str, max: usize) {
    if value.chars().count() > max {
        fields.insert(
            key.to_string(),
            format!("Keep this under {} characters.", max + 1),
        );
    }
    if has_control(value) {
        fields.insert(
            key.to_string(),
            "Use plain text without control characters.".to_string(),
        );
    }
}

fn validate_palette(name: &str, palette: &Palette, fields: &mut BTreeMap<String, String>) {
    let colors = [
        ("ground", &palette.ground),
        ("surface", &palette.surface),
        ("ink", &palette.ink),
        ("muted", &palette.muted),
        ("accent", &palette.accent),
        ("accent_ink", &palette.accent_ink),
    ];
    let mut valid = true;
    for (key, value) in colors {
        if !is_hex_color(value) {
            fields.insert(
                format!("{name}.{key}"),
                "Use a six-digit hex colour, for example #2c2325.".to_string(),
            );
            valid = false;
        }
    }
    if !valid {
        return;
    }

    let checks = [
        ("ink", &palette.ink, &palette.ground, 7.0),
        ("muted", &palette.muted, &palette.ground, 4.5),
        ("surface_ink", &palette.ink, &palette.surface, 4.5),
        ("accent", &palette.accent, &palette.ground, 4.5),
        ("accent_ink", &palette.accent_ink, &palette.accent, 4.5),
    ];
    for (field, foreground, background, minimum) in checks {
        let ratio = contrast_ratio(foreground, background);
        if ratio < minimum {
            fields.insert(
                format!("{name}.{field}"),
                format!("This pair measures {ratio:.2}:1. Use at least {minimum:.1}:1."),
            );
        }
    }
}

pub fn contrast_ratio(foreground: &str, background: &str) -> f64 {
    let Some(foreground) = luminance(foreground) else {
        return 0.0;
    };
    let Some(background) = luminance(background) else {
        return 0.0;
    };
    let light = f64::max(foreground, background);
    let dark = f64::min(foreground, background);
    (light + 0.05) / (dark + 0.05)
}

fn luminance(value: &str) -> Option<f64> {
    let lower = value.to_ascii_lowercase();
    if !is_hex_color(&lower) {
        return None;
    }
    let number = u32::from_str_radix(lower.get(1..)?, 16).ok()?;
    let channel = |shift: u32| {
        let value = f64::from((number >> shift) & 0xff) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0))
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_image_path(value: &str) -> bool {
    value.starts_with("/images/")
        && value.len() <= 300
        && !value.contains("..")
        && !value.contains(['\\', '%', '?', '#'])
}

fn has_control(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ch != '\n' && ch != '\t' && (ch < '\u{20}' || ch == '\u{7f}'))
}

fn contains(values: &[&str], value: &str) -> bool {
    values.contains(&value)
}

pub fn parse_lenient(raw: &str) -> Theme {
    parse(raw).unwrap_or_default()
}

fn valid_email(value: &str) -> bool {
    let mut parts = value.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || local.is_empty() {
        return false;
    }
    if !local.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
    }) {
        return false;
    }
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    if host.is_empty() || tld.len() < 2 || !tld.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return false;
    }
    host.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

fn decode_theme(theme: &mut Theme, value: json::Json) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::Object(entries) => {
            for (key, value) in entries {
                let Some(field) = canonical_name(&key, &THEME_FIELDS) else {
                    return Err(unknown_field(&key));
                };
                apply_theme_field(theme, field, value)?;
            }
            Ok(())
        }
        other => Err(type_error_value(&other, "storefront.Theme")),
    }
}

fn apply_theme_field(theme: &mut Theme, field: &str, value: json::Json) -> Result<(), String> {
    match field {
        "brand_name" => assign_string(value, &mut theme.brand_name, "Theme", "brand_name"),
        "tagline" => assign_string(value, &mut theme.tagline, "Theme", "tagline"),
        "description" => assign_string(value, &mut theme.description, "Theme", "description"),
        "hero_heading" => assign_string(value, &mut theme.hero_heading, "Theme", "hero_heading"),
        "hero_body" => assign_string(value, &mut theme.hero_body, "Theme", "hero_body"),
        "hero_alt" => assign_string(value, &mut theme.hero_alt, "Theme", "hero_alt"),
        "hero_cta" => assign_string(value, &mut theme.hero_cta, "Theme", "hero_cta"),
        "hero_image" => assign_string(value, &mut theme.hero_image, "Theme", "hero_image"),
        "statement" => assign_string(value, &mut theme.statement, "Theme", "statement"),
        "shelf_heading" => assign_string(value, &mut theme.shelf_heading, "Theme", "shelf_heading"),
        "shop_title" => assign_string(value, &mut theme.shop_title, "Theme", "shop_title"),
        "house_label" => assign_string(value, &mut theme.house_label, "Theme", "house_label"),
        "about_heading" => assign_string(value, &mut theme.about_heading, "Theme", "about_heading"),
        "about_body" => assign_string(value, &mut theme.about_body, "Theme", "about_body"),
        "about_image" => assign_string(value, &mut theme.about_image, "Theme", "about_image"),
        "about_alt" => assign_string(value, &mut theme.about_alt, "Theme", "about_alt"),
        "closing_heading" => assign_string(
            value,
            &mut theme.closing_heading,
            "Theme",
            "closing_heading",
        ),
        "footer_note" => assign_string(value, &mut theme.footer_note, "Theme", "footer_note"),
        "footer_credit" => assign_string(value, &mut theme.footer_credit, "Theme", "footer_credit"),
        "contact_email" => assign_string(value, &mut theme.contact_email, "Theme", "contact_email"),
        "announcement" => assign_string(value, &mut theme.announcement, "Theme", "announcement"),
        "concerns_enabled" => assign_bool(
            value,
            &mut theme.concerns_enabled,
            "Theme",
            "concerns_enabled",
        ),
        "font_display" => assign_string(value, &mut theme.font_display, "Theme", "font_display"),
        "font_body" => assign_string(value, &mut theme.font_body, "Theme", "font_body"),
        "font_scale" => assign_float(value, &mut theme.font_scale, "Theme", "font_scale"),
        "radius" => assign_int(value, &mut theme.radius, "Theme", "radius"),
        "light" => assign_palette(value, "light", &mut theme.light),
        "dark" => assign_palette(value, "dark", &mut theme.dark),
        "updated_at" => assign_string(value, &mut theme.updated_at, "Theme", "updated_at"),
        _ => Ok(()),
    }
}

fn assign_palette(value: json::Json, name: &str, palette: &mut Palette) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::Object(entries) => {
            for (key, value) in entries {
                let Some(field) = canonical_name(&key, &PALETTE_FIELDS) else {
                    return Err(unknown_field(&key));
                };
                let path = format!("{name}.{field}");
                let slot = match field {
                    "ground" => &mut palette.ground,
                    "surface" => &mut palette.surface,
                    "ink" => &mut palette.ink,
                    "muted" => &mut palette.muted,
                    "accent" => &mut palette.accent,
                    "accent_ink" => &mut palette.accent_ink,
                    _ => return Ok(()),
                };
                assign_string(value, slot, "Theme", &path)?;
            }
            Ok(())
        }
        other => Err(type_error_field(
            "Theme",
            name,
            json_kind(&other),
            "storefront.Palette",
        )),
    }
}

fn assign_string(
    value: json::Json,
    slot: &mut String,
    struct_name: &str,
    path: &str,
) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::String(text) => {
            *slot = text;
            Ok(())
        }
        other => Err(type_error_field(
            struct_name,
            path,
            json_kind(&other),
            "string",
        )),
    }
}

fn assign_bool(
    value: json::Json,
    slot: &mut bool,
    struct_name: &str,
    path: &str,
) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::Bool(flag) => {
            *slot = flag;
            Ok(())
        }
        other => Err(type_error_field(
            struct_name,
            path,
            json_kind(&other),
            "bool",
        )),
    }
}

fn assign_float(
    value: json::Json,
    slot: &mut f64,
    struct_name: &str,
    path: &str,
) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::Number(literal) => match literal.parse::<f64>() {
            Ok(number) if number.is_finite() => {
                *slot = number;
                Ok(())
            }
            _ => Err(type_error_field(
                struct_name,
                path,
                &format!("number {literal}"),
                "float64",
            )),
        },
        other => Err(type_error_field(
            struct_name,
            path,
            json_kind(&other),
            "float64",
        )),
    }
}

fn assign_int(
    value: json::Json,
    slot: &mut i64,
    struct_name: &str,
    path: &str,
) -> Result<(), String> {
    match value {
        json::Json::Null => Ok(()),
        json::Json::Number(literal) => match literal.parse::<i64>() {
            Ok(number) => {
                *slot = number;
                Ok(())
            }
            Err(_) => Err(type_error_field(
                struct_name,
                path,
                &format!("number {literal}"),
                "int",
            )),
        },
        other => Err(type_error_field(
            struct_name,
            path,
            json_kind(&other),
            "int",
        )),
    }
}

fn json_kind(value: &json::Json) -> &'static str {
    match value {
        json::Json::Null => "null",
        json::Json::Bool(_) => "bool",
        json::Json::Number(_) => "number",
        json::Json::String(_) => "string",
        json::Json::Array(_) => "array",
        json::Json::Object(_) => "object",
    }
}

fn type_error_value(value: &json::Json, target: &str) -> String {
    format!(
        "json: cannot unmarshal {} into Go value of type {target}",
        json_kind(value)
    )
}

fn type_error_field(struct_name: &str, path: &str, value: &str, target: &str) -> String {
    format!(
        "json: cannot unmarshal {value} into Go struct field {struct_name}.{path} of type {target}"
    )
}

fn unknown_field(key: &str) -> String {
    format!("json: unknown field {}", json::quote_go_string(key))
}

fn canonical_name<'a>(key: &str, names: &'a [&'a str]) -> Option<&'a str> {
    names
        .iter()
        .find(|name| **name == key)
        .or_else(|| names.iter().find(|name| equal_fold(key, name)))
        .copied()
}

fn equal_fold(input: &str, name: &str) -> bool {
    let mut input = input.chars();
    let mut name = name.chars();
    loop {
        match (input.next(), name.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) => {
                if !(left.eq_ignore_ascii_case(&right)
                    || (right == 'k' && left == '\u{212A}')
                    || (right == 's' && left == '\u{17F}'))
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Theme, contrast_ratio, encode, parse, parse_lenient, validate};

    #[test]
    fn default_theme_is_valid() {
        assert!(validate(&Theme::default()).is_empty());
    }

    #[test]
    fn rejects_low_contrast_muted_text() {
        let mut theme = Theme::default();
        theme.light.muted = "#b8aeb0".to_string();
        assert!(validate(&theme).contains_key("light.muted"));
    }

    #[test]
    fn parse_fills_defaults_and_keeps_palette() {
        let theme = parse(r#"{"brand_name":"Nour","font_body":"inter"}"#).unwrap();
        assert_eq!(theme.brand_name, "Nour");
        assert_eq!(theme.font_body, "inter");
        assert_eq!(theme.light.ground, Theme::default().light.ground);
        assert!(theme.concerns_enabled);
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(parse(r#"{"brand_name":"Nour","css":"body{}"}"#).is_err());
    }

    #[test]
    fn encode_round_trips() {
        let theme = Theme::default();
        let raw = encode(&theme).unwrap();
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed, theme);
    }

    #[test]
    fn lenient_parse_falls_back_to_default() {
        assert_eq!(parse_lenient("{"), Theme::default());
    }

    #[test]
    fn contrast_rejects_bad_colours() {
        assert_eq!(contrast_ratio("red", "#000000"), 0.0);
        assert!(contrast_ratio("#FFFFFF", "#000000") > 20.0);
    }

    #[test]
    fn special_floats_cannot_be_encoded() {
        let nan = Theme {
            font_scale: f64::NAN,
            ..Theme::default()
        };
        assert_eq!(
            encode(&nan).expect_err("NaN").to_string(),
            "json: unsupported value: NaN"
        );
        let positive = Theme {
            font_scale: f64::INFINITY,
            ..Theme::default()
        };
        assert_eq!(
            encode(&positive).expect_err("+Inf").to_string(),
            "json: unsupported value: +Inf"
        );
        let negative = Theme {
            font_scale: f64::NEG_INFINITY,
            ..Theme::default()
        };
        assert_eq!(
            encode(&negative).expect_err("-Inf").to_string(),
            "json: unsupported value: -Inf"
        );
    }
}

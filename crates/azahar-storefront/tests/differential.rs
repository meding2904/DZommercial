//! Differential tests against the Go oracle. Every case is evaluated by both
//! implementations and the results are compared as JSON values, error strings
//! and exact encoded text.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

use azahar_storefront::{
    Palette, Theme, contrast_ratio, encode, normalize, parse, parse_lenient, validate,
};
use proptest::prelude::*;
use serde_json::{Value, json};

struct Oracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate has a parent")
            .parent()
            .expect("workspace root")
            .to_path_buf();
        let backend = root.join("backend");
        let mut child = Command::new("go")
            .arg("run")
            .arg("./cmd/oracle-storefront")
            .current_dir(&backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn go run ./cmd/oracle-storefront");
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = BufReader::new(child.stdout.take().expect("oracle stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn call(&mut self, request: Value) -> Value {
        let line = serde_json::to_string(&request).expect("serialise oracle request");
        writeln!(self.stdin, "{line}").expect("write oracle request");
        self.stdin.flush().expect("flush oracle request");
        let mut response = String::new();
        let read = self.stdout.read_line(&mut response).expect("read oracle");
        assert!(read > 0, "oracle exited before answering {line}");
        serde_json::from_str(&response).expect("oracle response is JSON")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn oracle() -> MutexGuard<'static, Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE
        .get_or_init(|| Mutex::new(Oracle::start()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn theme_json(theme: &Theme) -> Value {
    let raw = encode(theme).expect("encode generated theme");
    serde_json::from_str(&raw).expect("encoded theme is JSON")
}

fn oracle_theme_response(response: &Value, key: &str) -> Result<Value, String> {
    if let Some(error) = response.get("error") {
        return Err(error
            .as_str()
            .expect("oracle error is a string")
            .to_string());
    }
    Ok(response
        .get(key)
        .expect("oracle response carries the payload")
        .clone())
}

fn oracle_parse(raw: &str) -> Result<Value, String> {
    let response = oracle().call(json!({"op": "parse", "raw": raw}));
    oracle_theme_response(&response, "theme")
}

fn oracle_parse_lenient(raw: &str) -> Value {
    oracle().call(json!({"op": "parse_lenient", "raw": raw}))
}

fn oracle_encode(theme: &Theme) -> Result<String, String> {
    let response = oracle().call(json!({"op": "encode", "theme": theme_json(theme)}));
    oracle_theme_response(&response, "raw")
        .map(|value| value.as_str().expect("raw is a string").to_string())
}

fn oracle_normalize(theme: &Theme) -> Value {
    oracle().call(json!({"op": "normalize", "theme": theme_json(theme)}))
}

fn oracle_validate(theme: &Theme) -> Value {
    let response = oracle().call(json!({"op": "validate", "theme": theme_json(theme)}));
    response.get("fields").expect("fields payload").clone()
}

fn oracle_contrast(foreground: &str, background: &str) -> f64 {
    let response = oracle().call(json!({"op": "contrast", "fg": foreground, "bg": background}));
    response
        .get("ratio")
        .expect("ratio payload")
        .as_f64()
        .expect("ratio is a number")
}

fn assert_parse_matches(raw: &str) {
    let rust = match parse(raw) {
        Ok(theme) => Ok(theme_json(&theme)),
        Err(error) => Err(error.to_string()),
    };
    let go = oracle_parse(raw);
    assert_eq!(rust, go, "parse mismatch for {raw:?}");

    assert_eq!(
        theme_json(&parse_lenient(raw)),
        oracle_parse_lenient(raw),
        "parse_lenient mismatch for {raw:?}"
    );
}

fn assert_theme_ops_match(theme: &Theme, label: &str) {
    assert_eq!(
        oracle_encode(theme).unwrap_or_else(|error| panic!("oracle encode {label}: {error}")),
        encode(theme).expect("encode theme"),
        "encode mismatch for {label}"
    );
    assert_eq!(
        oracle_normalize(theme),
        theme_json(&normalize(theme)),
        "normalize mismatch for {label}"
    );
    assert_eq!(
        oracle_validate(theme),
        serde_json::to_value(validate(theme)).expect("validation map is JSON"),
        "validate mismatch for {label}"
    );
}

fn mutate(theme: &Theme, update: impl FnOnce(&mut Theme)) -> Theme {
    let mut copy = theme.clone();
    update(&mut copy);
    copy
}

fn parse_fixtures() -> Vec<&'static str> {
    vec![
        // Fixtures from theme_test.go.
        r#"{"brand_name":"Nour","font_body":"inter"}"#,
        r#"{"concerns_enabled":false}"#,
        r#"{"brand_name":"Nour","css":"body{}"}"#,
        r#"{"brand_name":"Nour"}"#,
        // Missing, null and extra values.
        "null",
        "{}",
        "  \t\n ",
        r#"{"brand_name":null}"#,
        r#"{"radius":null}"#,
        r#"{"font_scale":null}"#,
        r#"{"light":null}"#,
        r#"{"light":{}}"#,
        r##"{"light":{"ground":"#abcdef"}}"##,
        r#"{"light":{"ground":null}}"#,
        r#"{"brand_name":"a","brand_name":"b"}"#,
        r#"{"brand_name":42,"brand_name":"ok"}"#,
        r##"{"light":{"ground":"#fff","ground":"#000"}}"##,
        r#"{"BRAND_NAME":"x"}"#,
        r#"{"Brand_Name":"x"}"#,
        r##"{"LIGHT":{"GROUND":"#ffffff"}}"##,
        r#"{"font_di\u017Fplay":"inter"}"#,
        r#"{"font_diſplay":"inter"}"#,
        r#"{"brandname":"x"}"#,
        r#"{"brand-name":"x"}"#,
        r#"{"updated_at":"2026-01-01T00:00:00Z"}"#,
        r#"{"brand_name":"x"} trailing"#,
        r#"{"brand_name":"x"}{"css":1}"#,
        r#"{"brand_name":"x"}garbage"#,
        r#"{"css":{"deep":[1,2,{"x":null}]},"brand_name":"ok"}"#,
        r#"{"light":{"foo":1},"css":2}"#,
        r#"{"CSS":1}"#,
        r#"{"Light":{"Foo":1}}"#,
        r#"{"\u200b":1}"#,
        r#"{"\ue000":1}"#,
        r#"{"\u0378":1}"#,
        // Type errors.
        "[]",
        "[{}]",
        r#""str""#,
        "123",
        "true",
        r#"{"brand_name":42}"#,
        r#"{"brand_name":true}"#,
        r#"{"brand_name":[]}"#,
        r#"{"brand_name":{}}"#,
        r#"{"light":[]}"#,
        r#"{"light":42}"#,
        r#"{"light":{"ground":42}}"#,
        r#"{"light":{"ground":true}}"#,
        r#"{"concerns_enabled":"yes"}"#,
        r#"{"radius":"4"}"#,
        r#"{"radius":4.5}"#,
        r#"{"radius":1e2}"#,
        r#"{"radius":99999999999999999999}"#,
        r#"{"radius":-0.0}"#,
        r#"{"radius":1.0}"#,
        r#"{"font_scale":"1"}"#,
        r#"{"font_scale":1e400}"#,
        r#"{"font_scale":-1e400}"#,
        r#"{"font_scale":1e-400}"#,
        r#"{"updated_at":123}"#,
        r#"{"css":1,"brand_name":42}"#,
        r#"{"brand_name":42,"css":1}"#,
        // Malformed JSON.
        "{",
        "not json",
        r#"{"brand_name":}"#,
        r#"{"brand_name":"Nour""#,
        r#"{"brand_name":"Nour",}"#,
        r#"{"brand_name" "Nour"}"#,
        r#"{brand_name:"Nour"}"#,
        "[1,]",
        r#"{"a":1,}"#,
        r#"{"a" 1}"#,
        r#"{"a":1 "b":2}"#,
        "1.",
        ".5",
        "01",
        "tru",
        "\"abc",
        r#"{"radius":1.}"#,
        r#"{"radius":1e}"#,
        r#"{"radius":-}"#,
        r#"{"radius":-x}"#,
        r#"{"radius":01}"#,
        r#"{"brand_name":"a\qb"}"#,
        r#"{"brand_name":"a\'b"}"#,
        r#"{"brand_name":"a\u12"#,
        r#"{"brand_name":"a\u12zz"}"#,
        "{\"brand_name\":\"a\\é\"}",
        "{\"brand_name\":\"a\\ué\"}",
        "{\"brand_name\":\"a\\ué12\"}",
        "{\"brand_name\":\"a\\u😀\"}",
        "{\"brand_name\":\"a\\u😀1\"}",
        "{\"brand_name\":\"a\\u€é\"}",
        "{\"brand_name\":\"a\tb\"}",
        "{\"brand_name\":\"a\u{1}b\"}",
        r#"{"brand_name":"a\u0001b"}"#,
        r#"{"brand_name":"a\uD800b"}"#,
        r#"{"brand_name":"a\uDC00b"}"#,
        r#"{"brand_name":"a\uD800\uD800b"}"#,
        r#"{"brand_name":"a\uD800\u0041b"}"#,
        r#"{"brand_name":"a\uD83D\uDE00b"}"#,
        r#"{"brand_name":"a\uD800\uZZZZ"}"#,
        r#"{"a":[1,2,]}"#,
        r#"{"a":[1 2]}"#,
        r#"{"a":tru}"#,
        r#"{"a":fals}"#,
        r#"{"a":TRUE}"#,
        r#"{"a":1, "brand_name":42}"#,
        r#"{"brand_name":42, oops}"#,
        "\u{feff}{}",
        "\u{00a0}{}",
        "\u{2028}{}",
        "\u{200b}{}",
        "\u{e000}{}",
        "\u{0378}{}",
        "\u{1}{}",
        "\u{7f}{}",
        "\u{b}{}",
        "{\"brand_name\":\"a\u{7f}b\"}",
        // Escapes and raw characters that survive decoding.
        r#"{"brand_name":"a<&>b","<tag>&":"x"}"#,
        r#"{"brand_name":"a\/b"}"#,
        r#"{"brand_name":"a\b\f\n\r\tb"}"#,
        r#"{"brand_name":"a\u007fb"}"#,
        r#"{"brand_name":"a\u0080b"}"#,
        r#"{"brand_name":"a😀b"}"#,
        r#"{"brand_name":"a\"b"}"#,
        r#"{"brand_name":"a\\b"}"#,
    ]
}

#[test]
fn fixed_parse_fixtures_match_oracle() {
    for raw in parse_fixtures() {
        assert_parse_matches(raw);
    }
}

#[test]
fn deep_nesting_matches_oracle() {
    let accepted = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert_parse_matches(&accepted);

    let too_deep = format!("{}{}", "[".repeat(10_001), "]".repeat(10_001));
    assert_parse_matches(&too_deep);

    let truncated = "[".repeat(10_001);
    assert_parse_matches(&truncated);

    let objects = format!("{}{}", "{\"\":".repeat(10_000), "}".repeat(10_000));
    assert_parse_matches(&objects);

    let objects_too_deep = format!("{}{}", "{\"\":".repeat(10_001), "}".repeat(10_001));
    assert_parse_matches(&objects_too_deep);

    let nested_field = format!(
        "{{\"brand_name\":{}{}}}",
        "[".repeat(9_999),
        "]".repeat(9_999)
    );
    assert_parse_matches(&nested_field);

    let nested_field_too_deep = format!(
        "{{\"brand_name\":{}{}}}",
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    assert_parse_matches(&nested_field_too_deep);
}

#[test]
fn theme_test_fixtures_match_oracle() {
    let default = Theme::default();
    assert_theme_ops_match(&default, "default");
    assert_eq!(
        oracle().call(json!({"op": "default"})),
        theme_json(&default),
        "default theme mismatch"
    );

    let mut low_contrast = default.clone();
    low_contrast.light.muted = "#b8aeb0".to_string();
    assert!(validate(&low_contrast).contains_key("light.muted"));
    assert_theme_ops_match(&low_contrast, "low contrast");

    let mut unknown_font = default.clone();
    unknown_font.font_display = "comic-sans".to_string();
    assert_theme_ops_match(&unknown_font, "unknown font");

    let mut injection = default.clone();
    injection.light.ground = "red; background-image: url(https://example.test/x".to_string();
    assert_theme_ops_match(&injection, "css injection");

    let mut email = default.clone();
    email.contact_email = "Shop <hello@example.test>".to_string();
    assert_theme_ops_match(&email, "display name email");
    email.contact_email = "hello@example.test".to_string();
    assert_theme_ops_match(&email, "plain email");

    let mut hero = default.clone();
    hero.hero_image = "javascript:alert(1)".to_string();
    assert_theme_ops_match(&hero, "javascript image");
    hero.hero_image = "https://images.example.test/hero.webp".to_string();
    assert_theme_ops_match(&hero, "remote image");
    hero.hero_image = "/images/hero-1600.webp".to_string();
    assert_theme_ops_match(&hero, "bundled image");

    let mut unicode = default.clone();
    unicode.font_display = "  SCHIBSTED  ".to_string();
    unicode.font_body = "İNTER".to_string();
    unicode.contact_email = "  HELLO@AZAHAR.EXAMPLE  ".to_string();
    unicode.light.ground = " #F6F0EB ".to_string();
    unicode.dark.accent = "#E0A0A8".to_string();
    unicode.updated_at = "2026-01-01T00:00:00Z".to_string();
    assert_theme_ops_match(&unicode, "unicode case");

    let mut case_fold = default.clone();
    case_fold.font_display = "ſchibsted".to_string();
    case_fold.font_body = "ſource_ſerif".to_string();
    case_fold.contact_email = "İſİ@example.test".to_string();
    assert_theme_ops_match(&case_fold, "simple case folding");

    let boundary = mutate(&default, |theme| {
        theme.brand_name = "a".repeat(40);
        theme.tagline = "b".repeat(120);
        theme.hero_alt = "c".repeat(160);
        theme.description = "d".repeat(400);
        theme.about_body = "e".repeat(2000);
        theme.announcement = "f".repeat(140);
        theme.hero_cta = "é".repeat(60);
        theme.statement = "é".repeat(221);
        theme.shop_title = "😀".repeat(80);
        theme.about_heading = "g".repeat(120);
        theme.contact_email = "a".repeat(64) + "@example.test";
        theme.hero_image = format!("/images/{}", "h".repeat(292));
        theme.about_image = format!("/images/{}", "é".repeat(146));
    });
    assert_theme_ops_match(&boundary, "validation boundaries");

    let over_boundary = mutate(&default, |theme| {
        theme.brand_name = "a".repeat(41);
        theme.tagline = "b".repeat(121);
        theme.description = "d".repeat(401);
        theme.about_body = "e".repeat(2001);
        theme.hero_cta = "é".repeat(61);
        theme.statement = "é".repeat(222);
        theme.shop_title = "😀".repeat(81);
        theme.hero_image = format!("/images/{}", "h".repeat(293));
        theme.about_image = format!("/images/{}", "é".repeat(147));
        theme.contact_email = "a".repeat(64) + "@example.test";
    });
    assert_theme_ops_match(&over_boundary, "validation over boundaries");

    let control_and_long = mutate(&default, |theme| {
        theme.tagline = format!("{}\u{1}", "b".repeat(200));
        theme.footer_note = "line\nbreak\ttab".to_string();
        theme.about_body = "bell\u{7}".to_string();
        theme.announcement = "del\u{7f}".to_string();
        theme.brand_name = "\u{1}".to_string();
    });
    assert_theme_ops_match(&control_and_long, "control characters");

    for email in [
        "a@b.co",
        "a@b.c",
        "a@b..com",
        "a@.com",
        "a@@b.com",
        "a b@c.com",
        ".a@b.com",
        "a@-b.com",
        "a@b-.com",
        "a@b.com.",
        "a@b.co\u{0}",
        "Shop <hello@example.test>",
        "hello@example.test",
        "héllo@example.test",
    ] {
        let theme = mutate(&default, |theme| {
            theme.contact_email = email.to_string();
        });
        assert_theme_ops_match(&theme, email);
    }

    let mut long_email = default.clone();
    long_email.contact_email = format!("{}@example.test", "a".repeat(241));
    assert_theme_ops_match(&long_email, "email at 254 bytes");
    long_email.contact_email = format!("{}@example.test", "a".repeat(242));
    assert_theme_ops_match(&long_email, "email over 254 bytes");

    assert_eq!(oracle_parse_lenient("{"), theme_json(&default));
    assert_eq!(oracle_parse_lenient("{}"), theme_json(&default));
}

#[test]
fn colour_formats_match_oracle() {
    let colours = [
        "#ffffff",
        "#FFFFFF",
        "#fff",
        "#123456",
        "#1234567",
        "#12345678",
        "123456",
        "#gggggg",
        "",
        " #ffffff",
        "#ffffff ",
        "#fffffg",
        "#000000",
        "#2c2325",
        "#f6f0eb",
        "red",
        "red; background-image: url(https://example.test/x",
        "#ABCDEF",
        "#abcdef",
        "#0a0B0c",
    ];
    let default = Theme::default();
    for colour in colours {
        let theme = mutate(&default, |theme| {
            theme.light.ground = colour.to_string();
            theme.light.surface = colour.to_string();
            theme.light.ink = colour.to_string();
            theme.light.muted = colour.to_string();
            theme.light.accent = colour.to_string();
            theme.light.accent_ink = colour.to_string();
            theme.dark.ground = colour.to_string();
        });
        assert_theme_ops_match(&theme, colour);
    }

    for foreground in colours {
        for background in colours {
            let rust = contrast_ratio(foreground, background);
            let go = oracle_contrast(foreground, background);
            assert!(
                (rust - go).abs() <= 1e-12,
                "contrast mismatch for {foreground:?} on {background:?}: {rust} vs {go}"
            );
        }
    }
}

#[test]
fn encode_edge_cases_match_oracle() {
    let default = Theme::default();
    let special_strings = [
        "",
        "plain",
        "a<b>&c",
        "a\u{2028}b\u{2029}c",
        "a\u{8}b\u{c}c",
        "a\nb\tc\r",
        "a\u{1}b\u{7f}c",
        "é",
        "😀",
        "a\"b\\c",
        "  padded  ",
    ];
    for text in special_strings {
        let theme = mutate(&default, |theme| {
            theme.brand_name = text.to_string();
            theme.tagline = text.to_string();
            theme.updated_at = text.to_string();
            theme.light.ground = text.to_string();
            theme.dark.accent_ink = text.to_string();
        });
        assert_eq!(
            encode(&theme).expect("encode"),
            oracle_encode(&theme).expect("oracle encode"),
            "encode mismatch for {text:?}"
        );
    }

    let scales = [
        0.0,
        -0.0,
        1.0,
        0.85,
        1.15,
        0.8499999999999999,
        1.1500000000000001,
        0.1,
        1.2345678901234567,
        1e-7,
        1e-6,
        1e20,
        1e21,
        1e300,
        5e-324,
        1e-400,
    ];
    for scale in scales {
        let theme = mutate(&default, |theme| theme.font_scale = scale);
        assert_eq!(
            encode(&theme).expect("encode"),
            oracle_encode(&theme).expect("oracle encode"),
            "float encoding mismatch for {scale:?}"
        );
    }

    let radii = [i64::MIN, -1, 0, 4, 12, 13, 1_000_000, i64::MAX];
    for radius in radii {
        let theme = mutate(&default, |theme| theme.radius = radius);
        assert_eq!(
            encode(&theme).expect("encode"),
            oracle_encode(&theme).expect("oracle encode"),
            "int encoding mismatch for {radius}"
        );
    }
}

fn any_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<char>(), 0..60).prop_map(|chars| chars.into_iter().collect())
}

fn colour() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[0-9a-f]{6}".prop_map(|hex| format!("#{hex}")),
        1 => "[0-9A-F]{6}".prop_map(|hex| format!("#{hex}")),
        1 => any_text(),
        1 => prop::sample::select(vec![
            "#fff".to_string(),
            "#00000000".to_string(),
            "#".to_string(),
            String::new(),
            "#GGGGGG".to_string(),
            "#12345".to_string(),
            " red ".to_string(),
            "#f6f0eb".to_string(),
            "#2c2325".to_string(),
            "#ffffff ".to_string(),
        ]),
    ]
}

fn palette() -> impl Strategy<Value = Palette> {
    (colour(), colour(), colour(), colour(), colour(), colour()).prop_map(
        |(ground, surface, ink, muted, accent, accent_ink)| Palette {
            ground,
            surface,
            ink,
            muted,
            accent,
            accent_ink,
        },
    )
}

fn field_text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any_text(),
        1 => prop::sample::select(vec![
            "schibsted".to_string(),
            "source_serif".to_string(),
            "fraunces".to_string(),
            "system".to_string(),
            "inter".to_string(),
            "comic-sans".to_string(),
            "hello@azahar.example".to_string(),
            "Shop <hello@example.test>".to_string(),
            "/images/hero-1600.webp".to_string(),
            "https://images.example.test/hero.webp".to_string(),
            "/images/../hero.webp".to_string(),
            "/images/hero%20one.webp".to_string(),
            " padded ".to_string(),
            "line\nbreak".to_string(),
            "tab\there".to_string(),
            String::new(),
        ]),
    ]
}

fn theme_strategy() -> impl Strategy<Value = Theme> {
    let strings = proptest::collection::vec(field_text(), 24..25);
    let scales = prop_oneof![
        Just(0.0),
        Just(-0.0),
        Just(1.0),
        Just(0.85),
        Just(1.15),
        Just(0.8499),
        Just(1.1501),
        Just(1e-7),
        Just(1e21),
        Just(1e300),
        any::<f64>().prop_filter("finite float", |value| value.is_finite()),
    ];
    let radii = prop_oneof![
        Just(4i64),
        Just(-1i64),
        Just(0i64),
        Just(12i64),
        Just(13i64),
        any::<i64>(),
    ];
    (strings, any::<bool>(), scales, radii, palette(), palette()).prop_map(
        |(mut strings, concerns_enabled, font_scale, radius, light, dark)| {
            let mut next = strings.drain(..);
            Theme {
                brand_name: next.next().unwrap_or_default(),
                tagline: next.next().unwrap_or_default(),
                description: next.next().unwrap_or_default(),
                hero_heading: next.next().unwrap_or_default(),
                hero_body: next.next().unwrap_or_default(),
                hero_alt: next.next().unwrap_or_default(),
                hero_cta: next.next().unwrap_or_default(),
                hero_image: next.next().unwrap_or_default(),
                statement: next.next().unwrap_or_default(),
                shelf_heading: next.next().unwrap_or_default(),
                shop_title: next.next().unwrap_or_default(),
                house_label: next.next().unwrap_or_default(),
                about_heading: next.next().unwrap_or_default(),
                about_body: next.next().unwrap_or_default(),
                about_image: next.next().unwrap_or_default(),
                about_alt: next.next().unwrap_or_default(),
                closing_heading: next.next().unwrap_or_default(),
                footer_note: next.next().unwrap_or_default(),
                footer_credit: next.next().unwrap_or_default(),
                contact_email: next.next().unwrap_or_default(),
                announcement: next.next().unwrap_or_default(),
                concerns_enabled,
                font_display: next.next().unwrap_or_default(),
                font_body: next.next().unwrap_or_default(),
                font_scale,
                radius,
                light,
                dark,
                updated_at: next.next().unwrap_or_default(),
            }
        },
    )
}

fn json_fragment() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        " ",
        "\n",
        "\t",
        "\r",
        "\"brand_name\"",
        "\"BRAND_NAME\"",
        "\"light\"",
        "\"ground\"",
        "\"radius\"",
        "\"font_scale\"",
        "\"concerns_enabled\"",
        "\"css\"",
        "\"x\"",
        "\"\"",
        "\"a",
        "\"#fff\"",
        "\"#ffffff\"",
        "\"\\uD800\"",
        "\"\\uD83D\\uDE00\"",
        "\"\\q\"",
        "\\",
        "null",
        "true",
        "false",
        "tru",
        "nul",
        "0",
        "1",
        "-1",
        "01",
        "1.",
        "1e",
        "1e+",
        "-",
        "1.5",
        "1e2",
        "1e-400",
        "1e400",
        "é",
        "😀",
        "\u{feff}",
        "\u{00a0}",
        "{\"brand_name\":\"x\"}",
        "{\"light\":{\"ground\":\"#fff\"}}",
        "{\"radius\":4}",
        "{\"a\":1}",
    ])
    .prop_map(str::to_string)
}

fn number_literal() -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        "[0-9]{1,25}",
        prop::option::of("[0-9]{1,25}"),
        prop::option::of((
            prop::option::of(prop::sample::select(vec!["+", "-"])),
            "[0-9]{1,6}",
        )),
    )
        .prop_map(|(negative, integer, fraction, exponent)| {
            let mut number = String::new();
            if negative {
                number.push('-');
            }
            number.push_str(&integer);
            if let Some(fraction) = fraction {
                number.push('.');
                number.push_str(&fraction);
            }
            if let Some((sign, digits)) = exponent {
                number.push('e');
                if let Some(sign) = sign {
                    number.push_str(sign);
                }
                number.push_str(&digits);
            }
            number
        })
}

fn raw_json() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(json_fragment(), 0..10),
        any::<bool>(),
    )
        .prop_map(|(parts, trailing)| {
            let mut raw = parts.concat();
            if trailing {
                raw.push_str("{\"brand_name\":\"y\"}");
            }
            raw
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 320,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_raw_documents_match_oracle(raw in raw_json()) {
        assert_parse_matches(&raw);
    }

    #[test]
    fn generated_numbers_match_oracle(number in number_literal()) {
        for field in ["font_scale", "radius"] {
            let raw = format!("{{\"{field}\":{number}}}");
            assert_parse_matches(&raw);
        }
    }

    #[test]
    fn generated_themes_match_oracle(theme in theme_strategy()) {
        assert_theme_ops_match(&theme, "generated theme");

        let raw = encode(&theme).expect("encode generated theme");
        assert_parse_matches(&raw);
    }

    #[test]
    fn generated_colour_pairs_match_oracle((foreground, background) in (colour(), colour())) {
        let rust = contrast_ratio(&foreground, &background);
        let go = oracle_contrast(&foreground, &background);
        prop_assert!(
            (rust - go).abs() <= 1e-12,
            "contrast mismatch for {:?} on {:?}: {} vs {}",
            foreground,
            background,
            rust,
            go
        );
    }
}

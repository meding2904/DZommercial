//! The image-captcha store used by the storefront checkout.
//!
//! Port of `backend/internal/httpapi/captcha.go`: the same five-character
//! alphabet, the same two-second minimum age, the ten-minute TTL, the 5 000
//! entry ceiling with oldest-first eviction, IP binding, one-time use and the
//! same PNG shape. The image is decorative noise plus a built-in 5x7 glyph
//! table; no external font is needed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::Rng as _;
use subtle::ConstantTimeEq;

/// Number of characters in a challenge code.
pub const CAPTCHA_CODE_LENGTH: usize = 5;
/// The code alphabet: no `I`, `O`, `0` or `1`.
pub const CAPTCHA_ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// A form filled faster than this was not filled by a person.
pub const CAPTCHA_MIN_AGE: Duration = Duration::from_secs(2);
/// Challenges live for ten minutes.
pub const CAPTCHA_TTL: Duration = Duration::from_secs(10 * 60);
/// The store never holds more than this many live challenges.
pub const CAPTCHA_MAX_ENTRIES: usize = 5000;
/// The Go service sweeps expired entries every five minutes.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const CAPTCHA_TOKEN_MAX_LENGTH: usize = 128;
const WIDTH: usize = 220;
const HEIGHT: usize = 64;
const SCALE: usize = 4;

pub type CaptchaClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[derive(Debug, Clone)]
struct Entry {
    code: String,
    ip: String,
    issued: DateTime<Utc>,
    expires: DateTime<Utc>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<String, Entry>,
}

/// Errors issuing a challenge. They are infrastructure failures, so the
/// handler renders the shared 500.
#[derive(Debug, thiserror::Error)]
pub enum CaptchaError {
    #[error("generate captcha code: {0}")]
    Code(getrandom::Error),
    #[error("generate captcha token: {0}")]
    Token(getrandom::Error),
    #[error("encode captcha image")]
    Image,
}

/// One issued challenge. The HTTP response only carries `token`, `image` and
/// `ttl`; `code` exists for tests and for the handler that renders the answer
/// into the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCaptcha {
    pub token: String,
    pub image: String,
    pub code: String,
    pub ttl: Duration,
}

/// Why a challenge answer was refused. `Ok` is the empty success case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaCheck {
    Ok,
    Missing,
    Expired,
    Fast,
    Wrong,
}

impl CaptchaCheck {
    /// The Go store's reason strings, used for logging and tests.
    #[must_use]
    pub fn as_reason(self) -> &'static str {
        match self {
            Self::Ok => "",
            Self::Missing => "missing",
            Self::Expired => "expired",
            Self::Fast => "fast",
            Self::Wrong => "wrong",
        }
    }
}

/// A thread-safe challenge store. Cloning shares the same entries.
#[derive(Clone)]
pub struct CaptchaStore {
    inner: Arc<Mutex<Inner>>,
    ttl: Duration,
    min_age: Duration,
    max_entries: usize,
    clock: CaptchaClock,
    cleanup_started: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    test_seam: bool,
}

impl CaptchaStore {
    /// A production store: system clock, ten-minute TTL, 5 000 entries, and
    /// the background sweep started when a runtime is available.
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self::with_clock(ttl, Arc::new(Utc::now))
    }

    /// A store with an injected clock, for tests that control expiry and the
    /// minimum age.
    #[must_use]
    pub fn with_clock(ttl: Duration, clock: CaptchaClock) -> Self {
        let store = Self::build(ttl, CAPTCHA_MIN_AGE, CAPTCHA_MAX_ENTRIES, clock);
        store.start_cleanup();
        store
    }

    /// A store with every policy value exposed, for the store tests. It does
    /// not start the cleanup task by itself; the checkout route builder starts
    /// it when the store is wired in.
    #[doc(hidden)]
    #[must_use]
    pub fn for_tests(
        ttl: Duration,
        min_age: Duration,
        max_entries: usize,
        clock: CaptchaClock,
    ) -> Self {
        Self::build(ttl, min_age, max_entries, clock)
    }

    fn build(ttl: Duration, min_age: Duration, max_entries: usize, clock: CaptchaClock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            ttl,
            min_age,
            max_entries,
            clock,
            cleanup_started: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            test_seam: true,
        }
    }

    /// Drops the test seam, so [`issue`](Self::issue) always generates a random
    /// code even when `AZAHAR_TEST_CAPTCHA_CODE` is set. Production builds the
    /// store this way: the variable pins the code, and a pinned code is a
    /// captcha anyone can answer.
    #[must_use]
    pub fn without_test_seam(mut self) -> Self {
        self.test_seam = false;
        self
    }

    /// The current instant according to the store clock.
    #[must_use]
    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    /// Starts the five-minute sweep when called inside a Tokio runtime. The
    /// Go constructor always starts its goroutine; an inert store is only
    /// possible outside a runtime, which production never is.
    pub fn start_cleanup(&self) {
        if self.cleanup_started.load(Ordering::SeqCst) {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.cleanup_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let store = self.clone();
        handle.spawn(async move {
            let mut ticker = tokio::time::interval(CLEANUP_INTERVAL);
            ticker.tick().await; // the first interval tick is immediate
            loop {
                ticker.tick().await;
                if store.closed.load(Ordering::SeqCst) {
                    return;
                }
                store.sweep(store.now());
            }
        });
    }

    /// Stops the background sweep. Issuing and verifying still work; this is
    /// the shutdown half of the Go `close` method.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Issues one challenge for a client address.
    ///
    /// # Errors
    ///
    /// Returns [`CaptchaError`] if the OS random generator fails or the PNG
    /// cannot be encoded.
    pub fn issue(&self, ip: &str) -> Result<IssuedCaptcha, CaptchaError> {
        // Test/oracle seam: both servers can issue a known code so the live
        // checkout success path is diffable. Unset in normal operation, and
        // ignored entirely once the store drops the seam in production.
        let pinned = if self.test_seam {
            match std::env::var("AZAHAR_TEST_CAPTCHA_CODE") {
                Ok(value)
                    if value.len() == CAPTCHA_CODE_LENGTH
                        && value
                            .chars()
                            .all(|character| CAPTCHA_ALPHABET.contains(character)) =>
                {
                    Some(value)
                }
                _ => None,
            }
        } else {
            None
        };
        let code = match pinned {
            Some(value) => value,
            None => random_code().map_err(CaptchaError::Code)?,
        };
        let token = random_token().map_err(CaptchaError::Token)?;
        let image = captcha_image(&code).ok_or(CaptchaError::Image)?;
        let now = self.now();

        let mut inner = self.lock();
        self.sweep_locked(&mut inner, now);
        if inner.entries.len() >= self.max_entries {
            evict_oldest_locked(&mut inner);
        }
        inner.entries.insert(
            token.clone(),
            Entry {
                code: code.clone(),
                ip: ip.to_string(),
                issued: now,
                expires: now + chrono_duration(self.ttl),
            },
        );
        Ok(IssuedCaptcha {
            token,
            image,
            code,
            ttl: self.ttl,
        })
    }

    /// Checks one answer and consumes the challenge, exactly like Go's
    /// `verify`. A wrong answer, an expired token and an IP mismatch all burn
    /// the token; a too-fast answer leaves it usable.
    #[must_use]
    pub fn verify(&self, token: &str, answer: &str, ip: &str) -> CaptchaCheck {
        let token = token.trim();
        let answer = answer.trim().to_uppercase();
        if token.is_empty() || token.len() > CAPTCHA_TOKEN_MAX_LENGTH {
            return CaptchaCheck::Missing;
        }

        let now = self.now();
        let mut inner = self.lock();
        let Some(entry) = inner.entries.get(token) else {
            return CaptchaCheck::Expired;
        };
        if now > entry.expires {
            inner.entries.remove(token);
            return CaptchaCheck::Expired;
        }
        if !entry.ip.is_empty() && !ip.is_empty() && entry.ip != ip {
            inner.entries.remove(token);
            return CaptchaCheck::Expired;
        }
        if now.signed_duration_since(entry.issued) < chrono_duration(self.min_age) {
            return CaptchaCheck::Fast;
        }
        if !bool::from(entry.code.as_bytes().ct_eq(answer.as_bytes())) {
            inner.entries.remove(token);
            return CaptchaCheck::Wrong;
        }

        inner.entries.remove(token);
        CaptchaCheck::Ok
    }

    /// The code behind a token. Only used by tests to answer the image in an
    /// HTTP-level challenge flow.
    #[doc(hidden)]
    pub fn code_for(&self, token: &str) -> Option<String> {
        self.lock()
            .entries
            .get(token)
            .map(|entry| entry.code.clone())
    }

    /// Number of live entries, for tests.
    #[doc(hidden)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the store is empty, for tests.
    #[doc(hidden)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops expired entries. The background task calls this every five
    /// minutes; issue also sweeps, matching the Go lock order.
    pub fn sweep(&self, now: DateTime<Utc>) {
        let mut inner = self.lock();
        self.sweep_locked(&mut inner, now);
    }

    fn sweep_locked(&self, inner: &mut Inner, now: DateTime<Utc>) {
        inner.entries.retain(|_, entry| now <= entry.expires);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Go's `evictOldestLocked`: the entry with the earliest issue instant.
fn evict_oldest_locked(inner: &mut Inner) {
    let oldest = inner
        .entries
        .iter()
        .min_by_key(|(_, entry)| entry.issued)
        .map(|(token, _)| token.clone());
    if let Some(token) = oldest {
        inner.entries.remove(&token);
    }
}

fn chrono_duration(duration: Duration) -> chrono::Duration {
    chrono::Duration::from_std(duration).unwrap_or_else(|_| chrono::Duration::zero())
}

/// Five characters from the 32-symbol alphabet. The alphabet is a power of
/// two, so five random bits per character are uniform without modulo bias.
fn random_code() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; CAPTCHA_CODE_LENGTH];
    getrandom::fill(&mut bytes)?;
    let alphabet = CAPTCHA_ALPHABET.as_bytes();
    Ok(bytes
        .iter()
        .map(|byte| char::from(alphabet[usize::from(byte & 31)]))
        .collect())
}

/// 24 random bytes as unpadded base64url, Go's `randomCaptchaToken`.
fn random_token() -> Result<String, getrandom::Error> {
    use base64::Engine as _;
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// `captchaErrorMessage` in the Go handler.
#[must_use]
pub fn captcha_error_message(reason: CaptchaCheck) -> &'static str {
    match reason {
        CaptchaCheck::Fast => "Wait a moment, then enter the code.",
        CaptchaCheck::Expired | CaptchaCheck::Missing => {
            "The code expired or was already used. Request a new one."
        }
        CaptchaCheck::Wrong => "The code is wrong. Check the characters and try again.",
        CaptchaCheck::Ok => "",
    }
}

/// Renders a code into the same 220x64 PNG the Go service produced and wraps
/// it in a `data:` URL. Returns `None` if PNG encoding fails.
fn captcha_image(code: &str) -> Option<String> {
    use base64::Engine as _;

    let background = [244u8, 236, 229, 255];
    let ink = [44u8, 35, 37, 255];
    let noise: [[u8; 4]; 4] = [
        [216, 180, 184, 255],
        [185, 196, 184, 255],
        [216, 194, 168, 255],
        [201, 189, 187, 255],
    ];

    let mut pixels = vec![0u8; WIDTH * HEIGHT * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&background);
    }

    let mut rng = rand::rng();
    for _ in 0..6 {
        let color = noise[rng.random_range(0..noise.len())];
        draw_line(
            &mut pixels,
            rng.random_range(0..WIDTH) as i32,
            rng.random_range(0..HEIGHT) as i32,
            rng.random_range(0..WIDTH) as i32,
            rng.random_range(0..HEIGHT) as i32,
            color,
        );
    }
    for _ in 0..70 {
        let x = rng.random_range(0..WIDTH);
        let y = rng.random_range(0..HEIGHT);
        let color = noise[rng.random_range(0..noise.len())];
        set_pixel(&mut pixels, x as i32, y as i32, color);
    }

    let mut x = 24i32;
    for character in code.chars() {
        let Some(glyph) = captcha_glyph(character) else {
            continue;
        };
        x += 2 + rng.random_range(0..4);
        let y = 18 + rng.random_range(0..5) - 2;
        draw_glyph(&mut pixels, glyph, x, y, ink);
        x += 5 * SCALE as i32 + 8;
    }

    for _ in 0..3 {
        let color = noise[rng.random_range(0..noise.len())];
        draw_line(
            &mut pixels,
            rng.random_range(0..WIDTH) as i32,
            rng.random_range(0..HEIGHT) as i32,
            rng.random_range(0..WIDTH) as i32,
            rng.random_range(0..HEIGHT) as i32,
            color,
        );
    }

    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, WIDTH as u32, HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels).ok()?;
    }
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

fn set_pixel(pixels: &mut [u8], x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x as usize >= WIDTH || y as usize >= HEIGHT {
        return;
    }
    let offset = (y as usize * WIDTH + x as usize) * 4;
    pixels[offset..offset + 4].copy_from_slice(&color);
}

/// Go's `drawCaptchaGlyph`: every `1` becomes a `scale` square.
fn draw_glyph(pixels: &mut [u8], glyph: [&str; 7], x: i32, y: i32, ink: [u8; 4]) {
    for (row, line) in glyph.iter().enumerate() {
        for (column, value) in line.chars().enumerate() {
            if value != '1' {
                continue;
            }
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    set_pixel(
                        pixels,
                        x + column as i32 * SCALE as i32 + dx as i32,
                        y + row as i32 * SCALE as i32 + dy as i32,
                        ink,
                    );
                }
            }
        }
    }
}

/// Go's `drawCaptchaLine`: one pixel per step of the longer axis.
fn draw_line(pixels: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, ink: [u8; 4]) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = dx.max(dy);
    if steps == 0 {
        set_pixel(pixels, x0, y0, ink);
        return;
    }
    for step in 0..=steps {
        let x = x0 + (x1 - x0) * step / steps;
        let y = y0 + (y1 - y0) * step / steps;
        set_pixel(pixels, x, y, ink);
    }
}

fn captcha_glyph(character: char) -> Option<[&'static str; 7]> {
    let glyph = match character {
        'A' => [
            "01110", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'B' => [
            "11110", "10001", "10001", "11110", "10001", "10001", "11110",
        ],
        'C' => [
            "01111", "10000", "10000", "10000", "10000", "10000", "01111",
        ],
        'D' => [
            "11110", "10001", "10001", "10001", "10001", "10001", "11110",
        ],
        'E' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "11111",
        ],
        'F' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "10000",
        ],
        'G' => [
            "01111", "10000", "10000", "10111", "10001", "10001", "01111",
        ],
        'H' => [
            "10001", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'J' => [
            "00111", "00010", "00010", "00010", "10010", "10010", "01100",
        ],
        'K' => [
            "10001", "10010", "10100", "11000", "10100", "10010", "10001",
        ],
        'L' => [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
        'M' => [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
        'N' => [
            "10001", "11001", "10101", "10011", "10001", "10001", "10001",
        ],
        'P' => [
            "11110", "10001", "10001", "11110", "10000", "10000", "10000",
        ],
        'Q' => [
            "01110", "10001", "10001", "10001", "10101", "10010", "01101",
        ],
        'R' => [
            "11110", "10001", "10001", "11110", "10100", "10010", "10001",
        ],
        'S' => [
            "01111", "10000", "10000", "01110", "00001", "00001", "11110",
        ],
        'T' => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
        'U' => [
            "10001", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'V' => [
            "10001", "10001", "10001", "10001", "10001", "01010", "00100",
        ],
        'W' => [
            "10001", "10001", "10001", "10101", "10101", "11011", "10001",
        ],
        'X' => [
            "10001", "10001", "01010", "00100", "01010", "10001", "10001",
        ],
        'Y' => [
            "10001", "10001", "01010", "00100", "00100", "00100", "00100",
        ],
        'Z' => [
            "11111", "00001", "00010", "00100", "01000", "10000", "11111",
        ],
        '2' => [
            "01110", "10001", "00001", "00010", "00100", "01000", "11111",
        ],
        '3' => [
            "11110", "00001", "00001", "01110", "00001", "00001", "11110",
        ],
        '4' => [
            "00010", "00110", "01010", "10010", "11111", "00010", "00010",
        ],
        '5' => [
            "11111", "10000", "10000", "11110", "00001", "00001", "11110",
        ],
        '6' => [
            "01110", "10000", "10000", "11110", "10001", "10001", "01110",
        ],
        '7' => [
            "11111", "00001", "00010", "00100", "01000", "01000", "01000",
        ],
        '8' => [
            "01110", "10001", "10001", "01110", "10001", "10001", "01110",
        ],
        '9' => [
            "01110", "10001", "10001", "01111", "00001", "00001", "01110",
        ],
        _ => return None,
    };
    Some(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    struct TestClock(Arc<AtomicI64>);

    impl TestClock {
        fn new() -> Self {
            Self(Arc::new(AtomicI64::new(1_800_000_000)))
        }

        fn clock(&self) -> CaptchaClock {
            let seconds = self.0.clone();
            Arc::new(move || DateTime::from_timestamp(seconds.load(Ordering::SeqCst), 0).unwrap())
        }

        fn advance(&self, seconds: i64) {
            self.0.fetch_add(seconds, Ordering::SeqCst);
        }
    }

    fn store() -> (CaptchaStore, TestClock) {
        let clock = TestClock::new();
        let store = CaptchaStore::for_tests(
            CAPTCHA_TTL,
            CAPTCHA_MIN_AGE,
            CAPTCHA_MAX_ENTRIES,
            clock.clock(),
        );
        (store, clock)
    }

    #[test]
    fn codes_use_the_go_alphabet_and_shape() {
        for _ in 0..50 {
            let code = random_code().expect("code");
            assert_eq!(code.len(), 5);
            assert!(
                code.chars()
                    .all(|character| CAPTCHA_ALPHABET.contains(character))
            );
        }
        assert_eq!(CAPTCHA_ALPHABET.len(), 32);
    }

    #[test]
    fn tokens_are_32_character_base64url() {
        let token = random_token().expect("token");
        assert_eq!(token.len(), 32);
        assert!(!token.contains('='));
        assert!(!token.contains('+'));
        assert!(!token.contains('/'));
    }

    #[test]
    fn image_is_a_png_data_url() {
        let image = captcha_image("AB23C").expect("image");
        let encoded = image
            .strip_prefix("data:image/png;base64,")
            .expect("data url");
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("png info");
        assert_eq!(reader.info().width, 220);
        assert_eq!(reader.info().height, 64);
    }

    #[test]
    fn a_challenge_verifies_once_after_the_minimum_age() {
        let (store, clock) = store();
        let issued = store.issue("203.0.113.7").expect("issue");
        assert_eq!(store.len(), 1);

        assert_eq!(
            store.verify(&issued.token, &issued.code, "203.0.113.7"),
            CaptchaCheck::Fast
        );
        clock.advance(3);
        assert_eq!(
            store.verify(&issued.token, &issued.code, "203.0.113.7"),
            CaptchaCheck::Ok
        );
        assert!(store.is_empty());
        assert_eq!(
            store.verify(&issued.token, &issued.code, "203.0.113.7"),
            CaptchaCheck::Expired
        );
    }

    #[test]
    fn answers_are_trimmed_and_upper_cased() {
        let (store, clock) = store();
        let issued = store.issue("").expect("issue");
        clock.advance(3);
        assert_eq!(
            store.verify(
                &format!("  {}  ", issued.token),
                &format!(" {} ", issued.code.to_lowercase()),
                ""
            ),
            CaptchaCheck::Ok
        );
    }

    #[test]
    fn a_wrong_answer_burns_the_token() {
        let (store, clock) = store();
        let issued = store.issue("").expect("issue");
        clock.advance(3);
        let wrong = if issued.code.starts_with('A') {
            "BBBBB"
        } else {
            "AAAAA"
        };
        assert_eq!(store.verify(&issued.token, wrong, ""), CaptchaCheck::Wrong);
        assert_eq!(
            store.verify(&issued.token, &issued.code, ""),
            CaptchaCheck::Expired
        );
    }

    #[test]
    fn expired_tokens_are_refused_and_removed() {
        let (store, clock) = store();
        let issued = store.issue("").expect("issue");
        clock.advance(601);
        assert_eq!(
            store.verify(&issued.token, &issued.code, ""),
            CaptchaCheck::Expired
        );
        assert!(store.is_empty());
    }

    #[test]
    fn a_missing_or_oversized_token_is_missing() {
        let (store, _clock) = store();
        assert_eq!(store.verify("", "ABCDE", ""), CaptchaCheck::Missing);
        assert_eq!(
            store.verify(&"a".repeat(129), "ABCDE", ""),
            CaptchaCheck::Missing
        );
    }

    #[test]
    fn a_different_address_burns_the_token() {
        let (store, clock) = store();
        let issued = store.issue("198.51.100.1").expect("issue");
        clock.advance(3);
        assert_eq!(
            store.verify(&issued.token, &issued.code, "198.51.100.2"),
            CaptchaCheck::Expired
        );
        assert!(store.is_empty());
    }

    #[test]
    fn the_oldest_entry_is_evicted_at_the_ceiling() {
        let clock = TestClock::new();
        let store = CaptchaStore::for_tests(CAPTCHA_TTL, CAPTCHA_MIN_AGE, 3, clock.clock());
        let first = store.issue("").expect("first");
        clock.advance(1);
        let second = store.issue("").expect("second");
        clock.advance(1);
        let third = store.issue("").expect("third");
        clock.advance(1);
        let fourth = store.issue("").expect("fourth");
        assert_eq!(store.len(), 3);
        assert!(store.code_for(&first.token).is_none(), "oldest evicted");
        for token in [&second.token, &third.token, &fourth.token] {
            assert!(store.code_for(token).is_some());
        }
    }

    #[test]
    fn sweep_drops_only_expired_entries() {
        let (store, clock) = store();
        let issued = store.issue("").expect("issue");
        clock.advance(599);
        store.sweep(store.now());
        assert_eq!(store.len(), 1);
        clock.advance(2);
        store.sweep(store.now());
        assert!(store.code_for(&issued.token).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn error_messages_match_the_go_handler() {
        assert_eq!(
            captcha_error_message(CaptchaCheck::Fast),
            "Wait a moment, then enter the code."
        );
        assert_eq!(
            captcha_error_message(CaptchaCheck::Expired),
            "The code expired or was already used. Request a new one."
        );
        assert_eq!(
            captcha_error_message(CaptchaCheck::Missing),
            "The code expired or was already used. Request a new one."
        );
        assert_eq!(
            captcha_error_message(CaptchaCheck::Wrong),
            "The code is wrong. Check the characters and try again."
        );
    }
}

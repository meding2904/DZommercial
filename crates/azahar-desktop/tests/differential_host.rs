//! Differential coverage for the Tauri host's pure and lifecycle helpers.
//!
//! The Go Wails host is the oracle. `desktop/host_oracle_test.go` (build tag
//! `oracle`) calls `profileDirectory` and `api.NewClient` directly, copies the
//! pure arithmetic of `screenPositionAllowed`/`clampWindowSize` (the originals
//! read a live Wails runtime out of the `context.Context` created by
//! `wails.Run`, so they cannot run headlessly), and copies the exact
//! `OpenExternal` URL condition.
//!
//! This test also parses `src/bridge.js` as text. There is no Go side for that
//! case: Wails generates `window.go.main.App` bindings at build time and ships
//! no JavaScript shim, so the eight-method contract is checked statically.
//!
//! Cases are fixed plus deterministic pseudo-random (xorshift) combinations,
//! so a failure is reproducible without storing a corpus.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use azahar_desktop::{
    ProfileError, WAILS_METHODS, clamp_window_size, profile_directory_from,
    screen_position_allowed, validate_external_url,
};
use serde::{Deserialize, Serialize};

const MIN_DIFFERENTIAL_CASES: usize = 700;
const GO_SEED: u64 = 0x5EED_0A2A_1234_5678;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OracleInput {
    profile: Vec<ProfileCase>,
    clamp: Vec<ClampCase>,
    position: Vec<PositionCase>,
    urls: Vec<UrlCase>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileCase {
    id: String,
    #[serde(rename = "override")]
    override_dir: Option<String>,
    home: Option<String>,
    xdg: Option<String>,
    cwd: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClampCase {
    id: String,
    width: i64,
    height: i64,
    screen_width: i64,
    screen_height: i64,
}

#[derive(Serialize)]
struct PositionCase {
    id: String,
    x: i64,
    y: i64,
    screens: Vec<ScreenCase>,
    #[serde(rename = "screenError")]
    screen_error: bool,
}

#[derive(Serialize)]
struct ScreenCase {
    width: i64,
    height: i64,
    current: bool,
    primary: bool,
}

#[derive(Serialize)]
struct UrlCase {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct OracleResults {
    profile: Vec<ProfileResult>,
    clamp: Vec<ClampResult>,
    position: Vec<PositionResult>,
    urls: Vec<UrlResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileResult {
    id: String,
    ok: bool,
    path: String,
    mode: u32,
    exists: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    client_error: Option<String>,
}

#[derive(Deserialize)]
struct ClampResult {
    id: String,
    width: i64,
    height: i64,
}

#[derive(Deserialize)]
struct PositionResult {
    id: String,
    allowed: bool,
}

#[derive(Deserialize)]
struct UrlResult {
    id: String,
    accepted: bool,
    #[serde(default)]
    normalized: Option<String>,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        // xorshift64*: tiny, deterministic and dependency-free.
        let mut value = self.0;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.0 = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[(self.next() % values.len() as u64) as usize]
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

fn mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

fn set_mode(path: &Path, value: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(value)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let _ = (path, value);
    }
}

fn config_root(home: Option<&str>, xdg: Option<&str>) -> Option<PathBuf> {
    match xdg.filter(|value| !value.is_empty()) {
        Some(value) => {
            // `dirs::config_dir` ignores a relative XDG_CONFIG_HOME while Go's
            // os.UserConfigDir uses it as-is. The generator only emits
            // absolute temp paths, so both resolve to the same root.
            debug_assert!(Path::new(value).is_absolute(), "relative XDG case");
            Some(PathBuf::from(value))
        }
        None => home
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(value).join(".config")),
    }
}

fn make_dirs(dirs: &[&Path]) {
    for dir in dirs {
        fs::create_dir_all(dir).unwrap();
    }
}

fn profile_cases(root: &Path) -> Vec<ProfileCase> {
    let mut cases = Vec::new();
    let base = root.join("profile-cases");
    fs::create_dir_all(&base).unwrap();

    let case_dirs = |name: &str| {
        let case = base.join(name);
        let cwd = case.join("cwd");
        let home = case.join("home");
        let xdg = case.join("xdg");
        make_dirs(&[&cwd, &home, &xdg]);
        (case, cwd, home, xdg)
    };

    let push = |cases: &mut Vec<ProfileCase>,
                id: &str,
                override_dir: Option<PathBuf>,
                home: Option<PathBuf>,
                xdg: Option<PathBuf>,
                cwd: &Path| {
        cases.push(ProfileCase {
            id: id.to_owned(),
            override_dir: override_dir.map(|path| path.to_string_lossy().into_owned()),
            home: home.map(|path| path.to_string_lossy().into_owned()),
            xdg: xdg.map(|path| path.to_string_lossy().into_owned()),
            cwd: cwd.to_string_lossy().into_owned(),
        });
    };

    let (case, cwd, home, xdg) = case_dirs("absolute-existing");
    push(
        &mut cases,
        "abs-existing",
        Some(case.join("profiles/existing")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (case, cwd, home, xdg) = case_dirs("absolute-nested");
    push(
        &mut cases,
        "abs-nested",
        Some(case.join("profiles/deep/nested/profile")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (case, cwd, home, xdg) = case_dirs("absolute-space");
    push(
        &mut cases,
        "abs-space",
        Some(case.join("profiles/profile dir")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (case, cwd, home, xdg) = case_dirs("absolute-unicode");
    push(
        &mut cases,
        "abs-unicode",
        Some(case.join("profiles/profil-é")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("relative");
    push(
        &mut cases,
        "relative",
        Some(PathBuf::from("relative-profile")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("relative-nested");
    push(
        &mut cases,
        "relative-nested",
        Some(PathBuf::from("relative/nested/profile")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("relative-dot");
    push(
        &mut cases,
        "relative-dot",
        Some(PathBuf::from("./dotted-profile")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("relative-dot-name");
    push(
        &mut cases,
        "relative-dot-name",
        Some(PathBuf::from("...")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("empty-override-xdg");
    push(
        &mut cases,
        "empty-override-xdg",
        Some(PathBuf::from("")),
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("null-override-xdg");
    push(
        &mut cases,
        "null-override-xdg",
        None,
        Some(home),
        Some(xdg),
        &cwd,
    );

    let (_case, cwd, home, _xdg) = case_dirs("empty-override-home");
    push(
        &mut cases,
        "empty-override-home",
        Some(PathBuf::from("")),
        Some(home),
        Some(PathBuf::from("")),
        &cwd,
    );

    let (_case, cwd, home, _xdg) = case_dirs("null-override-home");
    push(
        &mut cases,
        "null-override-home",
        None,
        Some(home),
        None,
        &cwd,
    );

    let (_case, cwd, _home, _xdg) = case_dirs("no-config");
    push(&mut cases, "no-config", None, None, None, &cwd);

    let (case, cwd, _home, _xdg) = case_dirs("override-wins");
    push(
        &mut cases,
        "override-wins-no-config",
        Some(case.join("override-profile")),
        None,
        None,
        &cwd,
    );

    let (_case, cwd, _home, _xdg) = case_dirs("relative-wins");
    push(
        &mut cases,
        "relative-wins-no-config",
        Some(PathBuf::from("relative-profile")),
        None,
        None,
        &cwd,
    );

    let (_case, cwd, home, xdg) = case_dirs("tighten-xdg");
    let xdg_profile = xdg.join("azahar-admin");
    fs::create_dir_all(&xdg_profile).unwrap();
    set_mode(&xdg_profile, 0o755);
    push(&mut cases, "tighten-xdg", None, Some(home), Some(xdg), &cwd);

    let (_case, cwd, home, _xdg) = case_dirs("tighten-home");
    let home_profile = home.join(".config/azahar-admin");
    fs::create_dir_all(&home_profile).unwrap();
    set_mode(&home_profile, 0o755);
    push(&mut cases, "tighten-home", None, Some(home), None, &cwd);

    cases
}

fn push_clamp(
    cases: &mut Vec<ClampCase>,
    width: i64,
    height: i64,
    screen_width: i64,
    screen_height: i64,
) {
    let id = format!("clamp-{}", cases.len());
    cases.push(ClampCase {
        id,
        width,
        height,
        screen_width,
        screen_height,
    });
}

fn clamp_cases() -> Vec<ClampCase> {
    let widths = [
        -1_i64, 0, 1, 719, 720, 899, 900, 1079, 1080, 1379, 1380, 1919, 1920, 2560, 3840, 1_000_000,
    ];
    let heights = [
        -1_i64, 0, 1, 719, 720, 899, 900, 1079, 1080, 2159, 2160, 1_000_000,
    ];
    let screen_widths = [0_i64, -1, 1, 1080, 1366, 1920, 2560, 3840];
    let screen_heights = [0_i64, -1, 1, 720, 900, 1080, 1440, 2160];

    let mut cases = Vec::new();

    for width in widths {
        for screen_width in screen_widths {
            push_clamp(&mut cases, width, 900, screen_width, 1080);
        }
    }
    for height in heights {
        for screen_height in screen_heights {
            push_clamp(&mut cases, 1380, height, 1920, screen_height);
        }
    }
    for screen_width in [0_i64, -1, 1] {
        for screen_height in [0_i64, -1, 1] {
            push_clamp(&mut cases, 1380, 900, screen_width, screen_height);
        }
    }
    for (width, height, screen_width, screen_height) in [
        (1380, 900, 1920, 1080),
        (2400, 1400, 1920, 1080),
        (2400, 900, 1920, 1080),
        (1380, 1400, 1920, 1080),
        (0, 900, 1920, 1080),
        (1380, 0, 1920, 1080),
        (-5, -5, -5, -5),
        (1, 1, 1, 1),
        (1080, 720, 1080, 720),
        (1079, 719, 1080, 720),
    ] {
        push_clamp(&mut cases, width, height, screen_width, screen_height);
    }

    let mut rng = Rng::new(GO_SEED);
    while cases.len() < 500 {
        let width = *rng.pick(&widths);
        let height = *rng.pick(&heights);
        let screen_width = *rng.pick(&screen_widths);
        let screen_height = *rng.pick(&screen_heights);
        push_clamp(&mut cases, width, height, screen_width, screen_height);
    }
    cases
}

fn position_layouts() -> Vec<Vec<ScreenCase>> {
    fn screen(width: i64, height: i64, current: bool, primary: bool) -> ScreenCase {
        ScreenCase {
            width,
            height,
            current,
            primary,
        }
    }
    vec![
        vec![],
        vec![screen(1920, 1080, true, false)],
        vec![screen(1920, 1080, false, true)],
        vec![
            screen(1600, 900, true, false),
            screen(3840, 2160, false, true),
        ],
        vec![
            screen(3840, 2160, false, true),
            screen(1920, 1080, true, false),
        ],
        vec![screen(2560, 1440, false, true)],
        vec![screen(0, 0, true, false)],
        vec![screen(-1, 1080, true, false)],
        vec![screen(1920, -1, true, false)],
        vec![screen(1920, 1080, true, true)],
        vec![
            screen(1920, 1080, false, false),
            screen(2560, 1440, false, true),
        ],
        vec![
            screen(1920, 1080, true, false),
            screen(1600, 900, false, false),
            screen(3840, 2160, false, true),
        ],
        vec![
            screen(800, 600, false, true),
            screen(1920, 1080, true, false),
        ],
        vec![screen(100, 100, false, true)],
        vec![screen(1920, 1080, true, false), screen(0, 0, false, true)],
        vec![screen(0, 0, false, true), screen(1920, 1080, true, false)],
    ]
}

fn push_position(
    cases: &mut Vec<PositionCase>,
    x: i64,
    y: i64,
    screens: &[ScreenCase],
    screen_error: bool,
) {
    let id = format!("position-{}", cases.len());
    cases.push(PositionCase {
        id,
        x,
        y,
        screens: screens
            .iter()
            .map(|screen| ScreenCase {
                width: screen.width,
                height: screen.height,
                current: screen.current,
                primary: screen.primary,
            })
            .collect(),
        screen_error,
    });
}

fn position_cases() -> Vec<PositionCase> {
    let coordinates = [
        -1_i64, 0, 1, 100, 719, 720, 899, 900, 1079, 1080, 1600, 1919, 1920, 1921, 2560, 3840,
        100_000,
    ];
    let layouts = position_layouts();
    let mut cases = Vec::new();

    for layout in &layouts {
        for (x, y) in [
            (0, 0),
            (1920, 1080),
            (1921, 1080),
            (1920, 1081),
            (-1, 0),
            (0, -1),
        ] {
            push_position(&mut cases, x, y, layout, false);
        }
    }
    // The ScreenGetAll error path allows any non-negative position.
    for layout in &layouts {
        push_position(&mut cases, 10, 10, layout, true);
        push_position(&mut cases, -10, 10, layout, true);
    }

    let mut rng = Rng::new(GO_SEED ^ 0xA5A5_A5A5_A5A5_A5A5);
    while cases.len() < 480 {
        let layout = rng.pick(&layouts);
        let x = *rng.pick(&coordinates);
        let y = *rng.pick(&coordinates);
        let screen_error = rng.below(8) == 0;
        push_position(&mut cases, x, y, layout, screen_error);
    }
    cases
}

fn curated_urls() -> Vec<&'static str> {
    vec![
        "",
        "mailto:admin@example.com",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ftp://example.com/file",
        "http:example.com",
        "//example.com",
        "https://",
        "https://:443",
        "https:///example.com",
        "http:////example.com",
        "https://user:pass@example.com",
        "https://user@example.com",
        "https://@example.com",
        "https://example.com@evil.example",
        "https://user%40example.com",
        "https://%65xample.com/",
        "https://exa%6dple.com/",
        "https://example.com/%zz",
        "https://example.com/#%zz",
        "https://example.com/%2",
        "https://example.com/%2g",
        "https://example.com/\u{1}",
        "https://example.com/\u{7f}",
        "https://example.com/a\tb",
        "https://example.com/a\nb",
        "https://example.com\\evil",
        "https://example.com\\@evil.example",
        "https://exa mple.com/",
        "https://example.com:abc/",
        "https://example.com:-1/",
        "https://[::1/",
        "https://[]/",
        "https://[::1]x/",
        "https://.example.com/",
        "https://example..com/",
        "https://-example.com/",
        "https://example-.com/",
        " https://example.com",
        "http://example.com",
        "https://example.com",
        "https://example.com/",
        "https://example.com/path?q=1#frag",
        "https://example.com:443/",
        "https://example.com:8080/admin?x=1",
        "https://127.0.0.1:8080/",
        "https://[::1]/admin",
        "https://[::1]:8080/",
        "https://[::ffff:127.0.0.1]/",
        "https://localhost/",
        "https://EXAMPLE.com/Path",
        "HTTPS://EXAMPLE.COM/Path",
        "HtTp://example.com/",
        "https://éxample.com/",
        "https://例子.测试/",
        "https://xn--xample-9ua.com/",
        "https://example.com/%20",
        "https://example.com/%2e%2e/",
        "https://example.com/%00",
        "https://example.com/a%2fb",
        "https://example.com/path?q=%zz",
        "https://example.com/path?q=%2e%2e",
        "https://example.com/path#fragment",
        "https://example.com/path#frag%20ment",
        "https://example.com:0/",
        "https://example.com:65535/",
        "https://example.com:",
        "https://example.com:/path",
        "https://example.com?x=1",
        "https://example.com#x",
        "https://example.com/a b",
        "https://example.com/a?b=c d",
        "https://example.com/a#b c",
        "https://example.com./",
        "https://sub.domain.example.co.uk/",
        "https://a-b.example/",
        "https://0x7f000001/",
        "https://example.com/path;params?q#f",
    ]
}

fn url_cases() -> Vec<UrlCase> {
    let schemes = ["http", "https", "HTTP", "Https", "ftp", "http+unix", ""];
    let hosts = [
        "example.com",
        "EXAMPLE.com",
        "localhost",
        "127.0.0.1",
        "192.168.1.10",
        "[::1]",
        "[::ffff:127.0.0.1]",
        "éxample.com",
        "例子.测试",
        "xn--xample-9ua.com",
        "example.com.",
        "sub.example.co.uk",
        "a-b.example",
        "api.local",
        "example.com@evil.example",
        "user:pass@example.com",
        "",
        "exa_mple.com",
    ];
    let ports = ["", ":80", ":443", ":8080", ":0", ":65535", ":abc"];
    let users: [Option<&str>; 4] = [None, Some("user"), Some("user:pass"), Some("")];
    let paths = [
        "",
        "/",
        "/admin",
        "/a/b",
        "/%20",
        "/%2e%2e",
        "/%zz",
        "/a b",
        "/a\\b",
        "/p?q=1",
        "/p?q=%zz",
        "/p#frag",
        "/p#%zz",
        "/p?q=1#frag",
        "/a%2fb",
        "/p%2",
        "/a\u{1}b",
        "/a\nb",
    ];

    let mut cases = Vec::new();
    for url in curated_urls() {
        cases.push(UrlCase {
            id: format!("url-{}", cases.len()),
            url: url.to_owned(),
        });
    }

    let mut rng = Rng::new(GO_SEED ^ 0x0BAD_C0DE_0BAD_C0DE);
    while cases.len() < 440 {
        let scheme = rng.pick(&schemes);
        let host = rng.pick(&hosts);
        let port = rng.pick(&ports);
        let user = rng.pick(&users);
        let path = rng.pick(&paths);
        let mut url = String::new();
        url.push_str(scheme);
        url.push_str("://");
        if let Some(user) = user {
            url.push_str(user);
            url.push('@');
        }
        url.push_str(host);
        url.push_str(port);
        url.push_str(path);
        cases.push(UrlCase {
            id: format!("url-{}", cases.len()),
            url,
        });
    }
    cases
}

fn run_go_oracle(input: &OracleInput, desktop: &Path, work: &Path) -> OracleResults {
    match Command::new("go").arg("version").output() {
        Ok(output) if output.status.success() => {}
        _ => panic!("the Go toolchain is required for the host differential test"),
    }

    let input_path = work.join("host_oracle_input.json");
    let output_path = work.join("host_oracle_output.json");
    fs::write(
        &input_path,
        serde_json::to_vec(input).expect("encode oracle input"),
    )
    .expect("write oracle input");

    let output = Command::new("go")
        .args([
            "test",
            "-tags",
            "oracle",
            "-run",
            "TestOracleHost",
            "-v",
            "-count=1",
            "./...",
        ])
        .current_dir(desktop)
        .env("AZAHAR_HOST_ORACLE_INPUT", &input_path)
        .env("AZAHAR_HOST_ORACLE_OUTPUT", &output_path)
        .env("AZAHAR_SESSION_BACKEND", "file")
        .env("AZAHAR_DISABLE_KEYRING", "1")
        .env_remove("AZAHAR_PROFILE_DIR")
        .env_remove("AZAHAR_API_URL")
        .env_remove("AZAHAR_ORIGIN")
        .output()
        .expect("run go test");

    assert!(
        output.status.success(),
        "go test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let data = fs::read(&output_path).unwrap_or_else(|error| {
        panic!(
            "the Go oracle did not write {output_path:?}: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    serde_json::from_slice(&data).expect("decode oracle results")
}

fn check_profile(cases: &[ProfileCase], results: &[ProfileResult]) -> usize {
    assert_eq!(cases.len(), results.len(), "profile result count");
    let original_dir = std::env::current_dir().expect("current dir");
    for (case, oracle) in cases.iter().zip(results) {
        assert_eq!(case.id, oracle.id);
        std::env::set_current_dir(&case.cwd).expect("chdir to profile case cwd");
        let override_dir = case.override_dir.as_deref().map(std::ffi::OsStr::new);
        let root = config_root(case.home.as_deref(), case.xdg.as_deref());
        let rust: Result<PathBuf, ProfileError> =
            profile_directory_from(override_dir, root.as_deref());

        if oracle.ok {
            let path = rust.unwrap_or_else(|error| {
                panic!(
                    "{}: oracle succeeded with {:?} but rust failed: {error}",
                    case.id, oracle.path
                )
            });
            assert!(
                oracle.client_error.is_none(),
                "{}: go client failed: {:?}",
                case.id,
                oracle.client_error
            );
            assert_eq!(
                path.to_string_lossy(),
                oracle.path,
                "{}: profile path mismatch",
                case.id
            );
            assert!(
                path.is_dir(),
                "{}: rust profile is not a directory",
                case.id
            );
            assert!(oracle.exists, "{}: go profile does not exist", case.id);
            #[cfg(unix)]
            {
                assert_eq!(
                    mode(&path),
                    oracle.mode,
                    "{}: profile mode mismatch (rust {:o}, go {:o})",
                    case.id,
                    mode(&path),
                    oracle.mode
                );
                assert_eq!(
                    oracle.mode, 0o700,
                    "{}: oracle profile is not 0700",
                    case.id
                );
            }
        } else {
            assert!(
                rust.is_err(),
                "{}: oracle failed ({}) but rust returned {rust:?}",
                case.id,
                oracle.error.as_deref().unwrap_or("no error")
            );
        }
    }
    std::env::set_current_dir(original_dir).expect("restore current dir");
    cases.len()
}

fn check_clamp(cases: &[ClampCase], results: &[ClampResult]) -> usize {
    assert_eq!(cases.len(), results.len(), "clamp result count");
    for (case, oracle) in cases.iter().zip(results) {
        assert_eq!(case.id, oracle.id);
        let (width, height) = clamp_window_size(
            case.width,
            case.height,
            case.screen_width,
            case.screen_height,
        );
        assert_eq!(
            (width, height),
            (oracle.width, oracle.height),
            "{}: clamp({}, {}, {}, {})",
            case.id,
            case.width,
            case.height,
            case.screen_width,
            case.screen_height
        );
    }
    cases.len()
}

fn check_position(cases: &[PositionCase], results: &[PositionResult]) -> usize {
    assert_eq!(cases.len(), results.len(), "position result count");
    for (case, oracle) in cases.iter().zip(results) {
        assert_eq!(case.id, oracle.id);
        let current = case
            .screens
            .iter()
            .find(|screen| screen.current)
            .map(|screen| azahar_desktop::MonitorGeometry {
                width: screen.width,
                height: screen.height,
            });
        let primary = case
            .screens
            .iter()
            .find(|screen| screen.primary)
            .map(|screen| azahar_desktop::MonitorGeometry {
                width: screen.width,
                height: screen.height,
            });
        let (current, primary) = if case.screen_error {
            // Go returns true from screenPositionAllowed when ScreenGetAll
            // fails; the Rust helper models "monitor data unavailable" as
            // current/primary = None.
            (None, None)
        } else {
            (current, primary)
        };
        assert_eq!(
            screen_position_allowed(case.x, case.y, current, primary),
            oracle.allowed,
            "{}: position ({}, {}) with {:?} (screen error: {})",
            case.id,
            case.x,
            case.y,
            case.screens
                .iter()
                .map(|screen| (screen.width, screen.height, screen.current, screen.primary))
                .collect::<Vec<_>>(),
            case.screen_error
        );
    }
    cases.len()
}

fn check_urls(cases: &[UrlCase], results: &[UrlResult]) -> usize {
    assert_eq!(cases.len(), results.len(), "url result count");
    for (case, oracle) in cases.iter().zip(results) {
        assert_eq!(case.id, oracle.id);
        let rust = validate_external_url(&case.url);
        assert_eq!(
            rust.is_ok(),
            oracle.accepted,
            "{}: URL {:?} accepted rust={} go={} (go normalised: {:?})",
            case.id,
            case.url,
            rust.is_ok(),
            oracle.accepted,
            oracle.normalized
        );
        if let Ok(target) = rust {
            assert!(
                !target.is_empty(),
                "{}: accepted URL normalised to empty",
                case.id
            );
        }
    }
    cases.len()
}

struct ExpectedBridge {
    name: &'static str,
    command: &'static str,
    params: &'static [&'static str],
    keys: &'static [&'static str],
}

const EXPECTED_BRIDGE: [ExpectedBridge; 8] = [
    ExpectedBridge {
        name: "Info",
        command: "info",
        params: &[],
        keys: &[],
    },
    ExpectedBridge {
        name: "Health",
        command: "health",
        params: &[],
        keys: &[],
    },
    ExpectedBridge {
        name: "Request",
        command: "request",
        params: &["method", "path", "body"],
        keys: &["method", "path", "body"],
    },
    ExpectedBridge {
        name: "Image",
        command: "image",
        params: &["path"],
        keys: &["path"],
    },
    ExpectedBridge {
        name: "UpdateSettings",
        command: "update_settings",
        params: &["apiUrl", "origin", "allowNetwork"],
        keys: &["apiUrl", "origin", "allowNetwork"],
    },
    ExpectedBridge {
        name: "ClearSession",
        command: "clear_session",
        params: &[],
        keys: &[],
    },
    ExpectedBridge {
        name: "FirewallGuide",
        command: "firewall_guide",
        params: &[],
        keys: &[],
    },
    ExpectedBridge {
        name: "OpenExternal",
        command: "open_external",
        params: &["url"],
        keys: &["url"],
    },
];

fn object_keys(object: &str) -> Vec<String> {
    let bytes = object.as_bytes();
    let mut keys = Vec::new();
    let mut index = 0;
    let mut depth = 0_i32;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            value if depth == 0 && (value.is_ascii_alphabetic() || value == b'_') => {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let mut look = index;
                while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                    look += 1;
                }
                if bytes.get(look) == Some(&b':') {
                    keys.push(object[start..index].to_owned());
                }
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    keys
}

fn bridge_contract() -> usize {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bridge.js");
    let script = fs::read_to_string(&path).expect("read bridge.js");

    let app_start = script.find("const app = {").expect("bridge.js app object");
    let app_end = app_start
        + script[app_start..]
            .find("\n\t};")
            .expect("bridge.js app object end");
    let block = &script[app_start..app_end];

    let mut methods: Vec<(String, String, Vec<String>, Vec<String>)> = Vec::new();
    let mut cursor = block;
    while let Some(position) = cursor.find(": function") {
        let head = &cursor[..position];
        let name = head
            .rsplit(|value: char| !(value.is_ascii_alphanumeric() || value == '_'))
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        assert!(!name.is_empty(), "bridge.js method without a name");

        let tail = &cursor[position..];
        let params_open = tail.find('(').expect("bridge.js parameter list") + 1;
        let params_close = params_open
            + tail[params_open..]
                .find(')')
                .expect("bridge.js parameter list end");
        let params: Vec<String> = tail[params_open..params_close]
            .split(',')
            .map(str::trim)
            .filter(|param| !param.is_empty())
            .map(str::to_owned)
            .collect();

        let call = tail.find("call('").expect("bridge.js command call");
        let after_command = &tail[call + "call('".len()..];
        let command_end = after_command
            .find('\'')
            .expect("bridge.js command name end");
        let command = after_command[..command_end].to_owned();
        let args = &after_command[command_end..];
        let object_start = args.find('{').expect("bridge.js argument object") + 1;
        let mut depth = 1_usize;
        let mut object_end = args.len();
        for (offset, value) in args[object_start..].char_indices() {
            match value {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        object_end = object_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert_eq!(depth, 0, "bridge.js argument object is not closed");
        let keys = object_keys(&args[object_start..object_end]);

        methods.push((name, command, params, keys));
        cursor = &tail[call..];
    }

    assert_eq!(
        methods.len(),
        EXPECTED_BRIDGE.len(),
        "bridge.js exposes {} methods, expected {}",
        methods.len(),
        EXPECTED_BRIDGE.len()
    );
    for (index, ((name, command, params, keys), expected)) in
        methods.iter().zip(EXPECTED_BRIDGE.iter()).enumerate()
    {
        assert_eq!(name, expected.name, "bridge method #{index} name");
        assert_eq!(
            command, expected.command,
            "bridge method {name} command mapping"
        );
        assert_eq!(
            params.as_slice(),
            expected.params,
            "bridge method {name} positional parameters"
        );
        assert_eq!(
            keys.as_slice(),
            expected.keys,
            "bridge method {name} camelCase keys"
        );
    }

    let pairs: Vec<(&str, &str)> = methods
        .iter()
        .map(|(name, command, _, _)| (name.as_str(), command.as_str()))
        .collect();
    assert_eq!(
        pairs.as_slice(),
        WAILS_METHODS.as_slice(),
        "bridge.js and WAILS_METHODS disagree"
    );
    assert!(
        script.contains("Object.freeze(app);"),
        "bridge.js must freeze the app object"
    );
    assert!(
        script.contains("window.go = { main: { App: app } };"),
        "bridge.js must install window.go.main.App"
    );
    assert!(
        script.contains("window.__TAURI__.core"),
        "bridge.js must use the Tauri global API"
    );

    EXPECTED_BRIDGE.len()
}

const HEADLESS_NOTES: [&str; 5] = [
    "main.go screenPositionAllowed/clampWindowSize call wailsRuntime.ScreenGetAll/WindowGetSize/WindowSetSize, which read a Wails runtime out of the context created by wails.Run; context.Background() cannot run them headlessly",
    "the oracle copies those pure expressions verbatim from main.go:151-178 and main.go:198-222 and injects monitor sizes",
    "OpenExternal validation is the exact main.go:257-259 condition; the app-context check and BrowserOpenURL are outside this differential",
    "profileDirectory is called directly, followed by api.NewClient to match NewApp and the final 0700 state",
    "there is no Go bridge side: Wails generates window.go.main.App at build time, so bridge.js is checked statically against the eight-method contract",
];

const URL_EXCLUSION_NOTE: &str = "URL parity compares accept/reject only (WHATWG vs net/url normalisation differs). The corpus excludes inputs where Go's net/url is more permissive than the WHATWG parser (IPv6 zone IDs, numeric ports above 65535, dotted hosts ending in a number, and hosts with characters such as `]` or `,` outside brackets); Rust fails closed on those and they remain unverified";

#[test]
fn host_helpers_match_the_go_oracle() {
    let work = tempfile::tempdir().expect("temp dir");
    let root = work.path().join("oracle-root");
    fs::create_dir_all(&root).unwrap();

    let profile = profile_cases(&root);
    let clamp = clamp_cases();
    let position = position_cases();
    let urls = url_cases();
    let input = OracleInput {
        profile,
        clamp,
        position,
        urls,
    };

    let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop");
    let results = run_go_oracle(&input, &desktop, work.path());

    let profile_count = check_profile(&input.profile, &results.profile);
    let clamp_count = check_clamp(&input.clamp, &results.clamp);
    let position_count = check_position(&input.position, &results.position);
    let url_count = check_urls(&input.urls, &results.urls);
    let bridge_count = bridge_contract();

    let total = profile_count + clamp_count + position_count + url_count;
    let accepted = results.urls.iter().filter(|result| result.accepted).count();
    println!("  profile directory (env/path/mode): {profile_count}");
    println!("  clampWindowSize: {clamp_count}");
    println!("  screenPositionAllowed: {position_count}");
    println!(
        "  OpenExternal URL validation: {url_count} ({accepted} accepted, {} rejected)",
        url_count - accepted
    );
    println!("  bridge.js static contract: {bridge_count} methods");
    for note in HEADLESS_NOTES {
        println!("  note: {note}");
    }
    println!("  note: {URL_EXCLUSION_NOTE}");
    println!("differential host comparisons: {total}");
    assert!(
        total >= MIN_DIFFERENTIAL_CASES,
        "host differential ran only {total} comparisons, expected at least {MIN_DIFFERENTIAL_CASES}"
    );
}

//! Differential tests against `backend/cmd/oracle-plugins`.
//!
//! Registry scenarios are generated from a seeded RNG, sent to the Go oracle
//! and replayed against the Rust registry. Settings merge/mask/complete and the
//! encrypted storage path are compared against the same process, which shares
//! the libSQL database named by `AZAHAR_TEST_DATABASE_URL`.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use azahar_db::open;
use azahar_plugins::{
    Category, Env, Manifest, Migration, OptionField, Plugin, ProductType, Registry, Route,
    SettingField, SettingOption, StorefrontPage, UNREADABLE_MESSAGE,
};
use azahar_secrets::SecretBox;
use azahar_store::{Store, format_time};
use chrono::{DateTime, Utc};
use libsql::params;
use serde::Deserialize;
use serde_json::{Value, json};

const STANDARD_SCENARIOS: u64 = 250;
const CYCLE_SCENARIOS: u64 = 20;
const MERGE_CASES: usize = 80;
const MASK_CASES: usize = 40;
const COMPLETE_CASES: usize = 40;
const ENCRYPT_CASES: usize = 30;

const ID_POOL: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "zeta",
    "eta",
    "theta",
    "oils",
    "pcparts",
    "pcbuilder",
    "base",
    "middle",
    "top",
    "a_b",
    "a-b",
    "x1",
    "z9",
    "core",
    "shop",
];
const CATEGORY_POOL: &[&str] = &["face", "body", "ritual", "cpus", "gpus", "memory", "shared"];
const PAGE_POOL: &[&str] = &[
    "/build",
    "/team",
    "/ritual",
    "/parts",
    "/shared-page",
    "/oils",
];
const SETTING_KEYS: &[&str] = &["api_key", "model", "enabled", "count", "mode", "extra"];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn dsn() -> String {
    std::env::var("AZAHAR_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8880".to_string())
}

/// Runs the Go `strconv.ParseFloat` probe next to this test. The probe lives
/// in the crate because `backend/` stays read-only for this review.
fn go_float_acceptance(numbers: &[String]) -> Vec<bool> {
    let probe = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/go-float-probe/main.go");
    let mut child = Command::new("go")
        .arg("run")
        .arg(&probe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|error| panic!("start Go float probe {}: {error}", probe.display()));
    {
        let stdin = child.stdin.as_mut().expect("probe stdin");
        serde_json::to_writer(&mut *stdin, &json!({"numbers": numbers}))
            .expect("encode probe request");
        stdin.write_all(b"\n").expect("write probe request");
        stdin.flush().expect("flush probe request");
    }
    let output = child.wait_with_output().expect("wait for Go float probe");
    assert!(
        output.status.success(),
        "Go float probe failed with {}",
        output.status
    );
    let response: Value = serde_json::from_slice(&output.stdout).expect("decode probe response");
    response["accepted"]
        .as_array()
        .expect("accepted array")
        .iter()
        .map(|value| value.as_bool().expect("acceptance flag"))
        .collect()
}

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn(dsn: &str) -> Self {
        let backend = repository_root().join("backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-plugins"])
            .current_dir(&backend)
            .env("AZAHAR_TEST_DATABASE_URL", dsn)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| panic!("start Go oracle in {}: {error}", backend.display()));
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn batch(&mut self, requests: &[Value]) -> Vec<Value> {
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            {
                let stdin = self.stdin.as_mut().expect("oracle stdin open");
                serde_json::to_writer(&mut *stdin, request).expect("encode oracle request");
                stdin.write_all(b"\n").expect("write oracle request");
                stdin.flush().expect("flush oracle request");
            }
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .expect("read oracle response");
            assert!(read > 0, "Go oracle exited before answering");
            responses.push(serde_json::from_str(&line).expect("decode oracle response"));
        }
        responses
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[derive(Debug, Deserialize)]
struct Scenario {
    plugins: Vec<PluginSpec>,
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct PluginSpec {
    manifest: Manifest,
    #[serde(default)]
    routes: Vec<Route>,
    #[serde(default)]
    migrations: Vec<Migration>,
}

#[derive(Debug, Deserialize)]
struct Step {
    op: String,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    states: HashMap<String, bool>,
}

struct FakePlugin {
    manifest: Manifest,
    routes: Vec<Route>,
    migrations: Vec<Migration>,
}

impl Plugin for FakePlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    fn routes(&self) -> Vec<Route> {
        self.routes.clone()
    }

    fn migrations(&self) -> Vec<Migration> {
        self.migrations.clone()
    }
}

fn run_scenario(scenario: &Scenario) -> Value {
    let mut registry = Registry::new();
    let mut results = Vec::with_capacity(scenario.steps.len());
    for step in &scenario.steps {
        let result = match step.op.as_str() {
            "register" => match scenario.plugins.get(step.index) {
                Some(spec) => {
                    let plugin = FakePlugin {
                        manifest: spec.manifest.clone(),
                        routes: spec.routes.clone(),
                        migrations: spec.migrations.clone(),
                    };
                    match registry.register(Arc::new(plugin)) {
                        Ok(()) => json!({"ok": true}),
                        Err(error) => json!({"error": error.to_string()}),
                    }
                }
                None => json!({"error": "plugin index out of range"}),
            },
            "validate" => match registry.validate_dependencies() {
                Ok(()) => json!({"ok": true}),
                Err(error) => json!({"error": error.to_string()}),
            },
            "set_states" => {
                registry.set_states(&step.states);
                json!({"ok": true})
            }
            "set_enabled" => json!({"result": registry.set_enabled(&step.id, step.enabled)}),
            "enabled" => json!({"result": registry.enabled(&step.id)}),
            "effective_enabled" => json!({"result": registry.effective_enabled(&step.id)}),
            "dependents" => json!({"ids": registry.dependents(&step.id)}),
            "enabled_dependents" => json!({"ids": registry.enabled_dependents(&step.id)}),
            "blocked_by" => json!({"ids": registry.blocked_by(&step.id)}),
            "manifests" => json!({"manifests": registry.manifests()}),
            "migrations" => json!({"migrations": registry.migrations()}),
            "product_type" => json!({"product_type": registry.product_type(&step.id)}),
            "all" => {
                let ids: Vec<String> = registry
                    .all()
                    .iter()
                    .map(|plugin| plugin.manifest().id)
                    .collect();
                json!({"ids": ids})
            }
            "get" => json!({"found": registry.get(&step.id).is_some()}),
            other => json!({"error": format!("unknown step {other}")}),
        };
        results.push(result);
    }
    json!({"steps": results})
}

/// Go's `BlockedBy` returns a nil slice for an unknown plugin, which marshals
/// as null; Rust returns an empty `Vec`. Assert that this is the only shape
/// difference before normalising it. The HTTP DTO uses `omitempty`, so the two
/// are indistinguishable upstream.
fn normalise_ids(rust: &mut Value, oracle: &mut Value) {
    let Some(rust_steps) = rust.get_mut("steps").and_then(Value::as_array_mut) else {
        return;
    };
    let Some(oracle_steps) = oracle.get_mut("steps").and_then(Value::as_array_mut) else {
        return;
    };
    for (rust_step, oracle_step) in rust_steps.iter_mut().zip(oracle_steps) {
        if oracle_step.get("ids") == Some(&Value::Null) {
            assert_eq!(
                rust_step.get("ids"),
                Some(&json!([])),
                "the only permitted ids difference is nil versus []"
            );
            if let Some(object) = oracle_step.as_object_mut() {
                object.insert("ids".to_string(), json!([]));
            }
        }
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.0 = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a>(&mut self, values: &'a [&'a str]) -> &'a str {
        values[self.below(values.len())]
    }
}

fn base_manifest(id: &str) -> Manifest {
    Manifest {
        id: id.to_string(),
        name: format!("Plugin {id}"),
        version: "1.0.0".to_string(),
        ..Manifest::default()
    }
}

fn random_manifest(rng: &mut Rng, id: &str) -> (Manifest, Vec<Route>, Vec<Migration>) {
    let mut manifest = base_manifest(id);
    if rng.chance(40) {
        manifest.author = "Azahar".to_string();
    }
    if rng.chance(40) {
        manifest.description = "Generated differential fixture.".to_string();
    }
    if rng.chance(35) {
        manifest.admin_path = if rng.chance(25) {
            "/plugins/not-admin".to_string()
        } else {
            format!("/admin/plugins/{id}")
        };
    }
    if rng.chance(20) {
        manifest.name = if rng.chance(50) {
            String::new()
        } else {
            "é".repeat(81)
        };
    }
    if rng.chance(15) {
        manifest.version = if rng.chance(50) {
            "x".repeat(33)
        } else {
            String::new()
        };
    }
    if rng.chance(4) {
        manifest.dependencies = vec![id.to_string()];
    }
    if rng.chance(4) {
        manifest.dependencies = vec!["Bad Dependency".to_string()];
    }

    if rng.chance(50) {
        let field_count = rng.below(3) + 1;
        let mut fields = Vec::new();
        for _ in 0..field_count {
            let mut field = OptionField {
                key: rng
                    .pick(&["size", "colour", "notes", "tags", "socket_watts"])
                    .to_string(),
                label: "Field".to_string(),
                field_type: rng
                    .pick(&["text", "select", "textarea", "list"])
                    .to_string(),
                required: rng.chance(40),
                ..OptionField::default()
            };
            if rng.chance(8) {
                field.label = String::new();
            }
            if rng.chance(8) {
                field.field_type = "color".to_string();
            }
            if rng.chance(15) {
                field.key = "Bad Key".to_string();
            }
            if field.field_type == "select" {
                let count = if rng.chance(12) { 13 } else { rng.below(3) };
                field.suggestions = (0..count).map(|index| format!("s{index}")).collect();
            }
            fields.push(field);
        }
        manifest.product_type = Some(ProductType {
            label: if rng.chance(8) {
                String::new()
            } else {
                "Sample product".to_string()
            },
            summary: if rng.chance(50) {
                "Generated.".to_string()
            } else {
                String::new()
            },
            fields,
        });
    }

    if rng.chance(45) {
        let count = rng.below(3) + 1;
        for _ in 0..count {
            let slug = rng.pick(CATEGORY_POOL);
            manifest.categories.push(Category {
                slug: slug.to_string(),
                name: if rng.chance(8) {
                    String::new()
                } else {
                    format!("Category {slug}")
                },
                description: if rng.chance(5) {
                    "x".repeat(301)
                } else {
                    "Generated category.".to_string()
                },
                sort_order: rng.below(5) as i32,
            });
        }
        if rng.chance(8) {
            manifest.categories[0].slug = "Bad Slug".to_string();
        }
    }

    if rng.chance(35) {
        let count = rng.below(3) + 1;
        for _ in 0..count {
            let path = rng.pick(PAGE_POOL);
            manifest.storefront_pages.push(StorefrontPage {
                path: path.to_string(),
                label: "Page".to_string(),
                sort_order: rng.below(4) as i32,
            });
        }
        if rng.chance(10) {
            manifest.storefront_pages[0].path = "/admin/page".to_string();
        }
        if rng.chance(6) {
            manifest.storefront_pages[0].path = "/shop".to_string();
        }
    }

    if rng.chance(55) {
        let count = rng.below(4) + 1;
        for index in 0..count {
            let mut field = SettingField {
                key: SETTING_KEYS[index % SETTING_KEYS.len()].to_string(),
                label: "Setting".to_string(),
                field_type: rng
                    .pick(&["text", "secret", "number", "boolean", "select"])
                    .to_string(),
                required: rng.chance(40),
                max_length: match rng.below(10) {
                    0 => -1,
                    1 => 4097,
                    2 => 10,
                    _ => 0,
                },
                ..SettingField::default()
            };
            if rng.chance(6) {
                field.key = "Bad Key".to_string();
            }
            if field.field_type == "select" {
                if rng.chance(10) {
                    field.options = Vec::new();
                } else {
                    field.options = vec![
                        SettingOption {
                            value: "fast".to_string(),
                            label: "Fast".to_string(),
                        },
                        SettingOption {
                            value: "slow".to_string(),
                            label: "Slow".to_string(),
                        },
                    ];
                }
            }
            manifest.settings.push(field);
        }
    }

    let mut routes: Vec<Route> = Vec::new();
    if rng.chance(45) {
        for _ in 0..(rng.below(3) + 1) {
            if rng.chance(8) && !routes.is_empty() && rng.chance(50) {
                routes.push(routes[0].clone());
                continue;
            }
            routes.push(Route {
                method: rng
                    .pick(&["GET", "get", " get ", "POST", "PATCH", "DELETE", "HEAD", ""])
                    .to_string(),
                pattern: rng
                    .pick(&[
                        "/items",
                        "/orders/{id}",
                        "/a b",
                        "/a//b",
                        "/../x",
                        "/admin/x",
                        "relative",
                    ])
                    .to_string(),
                admin: rng.chance(50),
            });
        }
    }

    let mut migrations = Vec::new();
    if rng.chance(20) {
        for index in 0..(rng.below(2) + 1) {
            migrations.push(Migration {
                version: format!("{index:03}"),
                sql: "CREATE TABLE generated (id TEXT);".to_string(),
            });
        }
    }

    (manifest, routes, migrations)
}

fn push_observation_steps(steps: &mut Vec<Value>, ids: &[String]) {
    for id in ids {
        steps.push(json!({"op": "enabled", "id": id}));
        steps.push(json!({"op": "effective_enabled", "id": id}));
        steps.push(json!({"op": "dependents", "id": id}));
        steps.push(json!({"op": "enabled_dependents", "id": id}));
        steps.push(json!({"op": "blocked_by", "id": id}));
    }
}

fn standard_scenario(seed: u64) -> Value {
    let mut rng = Rng::new(seed);
    let plugins_in_scenario = rng.below(5) + 1;
    let mut used = HashSet::new();
    let mut manifests: Vec<(Manifest, Vec<Route>)> = Vec::new();
    let mut routes: Vec<Vec<Route>> = Vec::new();
    let mut migrations = Vec::new();

    for index in 0..plugins_in_scenario {
        let id = if index > 0 && rng.chance(10) {
            manifests[rng.below(index)].0.id.clone()
        } else {
            let mut candidate = rng.pick(ID_POOL).to_string();
            while !used.insert(candidate.clone()) {
                candidate = rng.pick(ID_POOL).to_string();
            }
            candidate
        };
        let (mut manifest, plugin_routes, plugin_migrations) = random_manifest(&mut rng, &id);
        if rng.chance(25) {
            manifest.dependencies = (0..rng.below(index.min(2) + 1))
                .map(|earlier| manifests[earlier].0.id.clone())
                .collect();
        }
        manifests.push((manifest, plugin_routes.clone()));
        routes.push(plugin_routes);
        migrations.push(plugin_migrations);
    }

    // Probe the fixture so dependencies are only assigned when every plugin
    // registers. That keeps ValidateDependencies errors deterministic.
    let mut probe = Registry::new();
    let all_registered = manifests.iter().enumerate().all(|(index, (manifest, _))| {
        let plugin = FakePlugin {
            manifest: manifest.clone(),
            routes: routes[index].clone(),
            migrations: migrations[index].clone(),
        };
        probe.register(Arc::new(plugin)).is_ok()
    });

    if all_registered {
        if rng.chance(55) {
            for index in 0..manifests.len() {
                if index == 0 {
                    continue;
                }
                let count = rng.below(2) + 1;
                let mut dependencies = Vec::new();
                for _ in 0..count {
                    let earlier = rng.below(index);
                    let dependency = manifests[earlier].0.id.clone();
                    if !dependencies.contains(&dependency) {
                        dependencies.push(dependency);
                    }
                }
                manifests[index].0.dependencies = dependencies;
            }
        }
        if rng.chance(10) {
            let index = rng.below(manifests.len());
            manifests[index].0.dependencies.push("missing".to_string());
        }
    }

    let ids: Vec<String> = manifests
        .iter()
        .map(|(manifest, _)| manifest.id.clone())
        .collect();

    let plugins: Vec<Value> = manifests
        .iter()
        .enumerate()
        .map(|(index, (manifest, _))| {
            json!({
                "manifest": manifest,
                "routes": routes[index]
                    .iter()
                    .map(|route| json!({
                        "method": route.method,
                        "pattern": route.pattern,
                        "admin": route.admin,
                    }))
                    .collect::<Vec<_>>(),
                "migrations": migrations[index],
            })
        })
        .collect();

    let mut steps = Vec::new();
    for index in 0..plugins.len() {
        steps.push(json!({"op": "register", "index": index}));
        if rng.chance(6) {
            steps.push(json!({"op": "register", "index": index}));
        }
    }
    steps.push(json!({"op": "validate"}));
    if rng.chance(60) {
        let mut states = HashMap::new();
        for id in &ids {
            if rng.chance(70) {
                states.insert(id.clone(), rng.chance(50));
            }
        }
        steps.push(json!({"op": "set_states", "states": states}));
    }
    push_observation_steps(&mut steps, &ids);
    if rng.chance(70) {
        let id = ids[rng.below(ids.len())].clone();
        steps.push(json!({"op": "set_enabled", "id": id, "enabled": rng.chance(50)}));
        push_observation_steps(&mut steps, &ids);
    }
    steps.push(json!({"op": "all"}));
    steps.push(json!({"op": "manifests"}));
    steps.push(json!({"op": "migrations"}));
    steps.push(json!({"op": "product_type", "id": ids[0]}));
    steps.push(json!({"op": "product_type", "id": "missing"}));
    steps.push(json!({"op": "get", "id": ids[0]}));
    steps.push(json!({"op": "get", "id": "missing"}));

    json!({"op": "registry", "plugins": plugins, "steps": steps})
}

fn cycle_scenario(seed: u64) -> Value {
    let mut rng = Rng::new(seed);
    let first = rng.pick(ID_POOL).to_string();
    let mut second = rng.pick(ID_POOL).to_string();
    while second == first {
        second = rng.pick(ID_POOL).to_string();
    }
    let mut manifests = vec![base_manifest(&first), base_manifest(&second)];
    manifests[0].dependencies = vec![second.clone()];
    manifests[1].dependencies = vec![first.clone()];
    if rng.chance(50) {
        let third = format!("downstream{}", rng.below(100));
        let mut third_manifest = base_manifest(&third);
        third_manifest.dependencies = vec![first.clone()];
        manifests.push(third_manifest);
    }

    let plugins: Vec<Value> = manifests
        .iter()
        .map(|manifest| json!({"manifest": manifest, "routes": [], "migrations": []}))
        .collect();
    let mut steps = Vec::new();
    for index in 0..plugins.len() {
        steps.push(json!({"op": "register", "index": index}));
    }
    steps.push(json!({"op": "validate"}));
    steps.push(json!({"op": "manifests"}));
    json!({"op": "registry", "plugins": plugins, "steps": steps})
}

fn edge_scenarios() -> Vec<Value> {
    let mut scenarios = Vec::new();
    let ids = vec![
        "a\"b".to_string(),
        "a\nb".to_string(),
        "a\\b".to_string(),
        "aéb".to_string(),
        "a\u{85}b".to_string(),
        "a\u{a0}b".to_string(),
        "a\u{7f}b".to_string(),
        "a".to_string(),
        "x".repeat(33),
    ];
    for id in ids {
        let manifest = base_manifest(&id);
        scenarios.push(json!({
            "op": "registry",
            "plugins": [{"manifest": manifest, "routes": [], "migrations": []}],
            "steps": [
                {"op": "register", "index": 0},
                {"op": "validate"},
                {"op": "manifests"},
            ],
        }));
    }

    let mut route_manifest = base_manifest("routes");
    route_manifest.name = "Routes".to_string();
    scenarios.push(json!({
        "op": "registry",
        "plugins": [{
            "manifest": route_manifest,
            "routes": [
                {"method": "GET", "pattern": "/quote\"/path", "admin": false},
                {"method": "HEAD", "pattern": "/tab\tpath", "admin": true},
            ],
            "migrations": [],
        }],
        "steps": [
            {"op": "register", "index": 0},
            {"op": "manifests"},
        ],
    }));

    scenarios.push(json!({
        "op": "registry",
        "plugins": [{
            "manifest": {
                "id": "emptytype",
                "name": "Empty type",
                "version": "1.0.0",
                "description": "",
                "product_type": {"label": "Empty"},
            },
            "routes": [],
            "migrations": [],
        }],
        "steps": [
            {"op": "register", "index": 0},
            {"op": "manifests"},
        ],
    }));

    let mut only = base_manifest("onlyone");
    only.name = "Only one".to_string();
    scenarios.push(json!({
        "op": "registry",
        "plugins": [{"manifest": only, "routes": [], "migrations": []}],
        "steps": [
            {"op": "register", "index": 0},
            {"op": "blocked_by", "id": "missing"},
            {"op": "dependents", "id": "missing"},
            {"op": "enabled", "id": "missing"},
        ],
    }));

    let mut migration_manifest = base_manifest("migrator");
    migration_manifest.name = "Migrator".to_string();
    scenarios.push(json!({
        "op": "registry",
        "plugins": [{
            "manifest": migration_manifest,
            "routes": [],
            "migrations": [
                {"Version": "001", "SQL": "CREATE TABLE x (id TEXT);"},
                {"Version": "002", "SQL": "CREATE TABLE y (id TEXT);"},
            ],
        }],
        "steps": [
            {"op": "register", "index": 0},
            {"op": "migrations"},
        ],
    }));
    scenarios
}

fn random_setting_manifest(rng: &mut Rng) -> Manifest {
    let count = rng.below(5) + 1;
    let mut settings = Vec::new();
    for key in SETTING_KEYS.iter().take(count) {
        let field_type = rng
            .pick(&["text", "secret", "number", "boolean", "select", "color"])
            .to_string();
        let mut field = SettingField {
            key: (*key).to_string(),
            label: "Setting".to_string(),
            field_type: field_type.clone(),
            required: rng.chance(50),
            max_length: match rng.below(8) {
                0 => 5,
                1 => 300,
                2 => 4096,
                _ => 0,
            },
            default: if rng.chance(30) {
                "default".to_string()
            } else {
                String::new()
            },
            ..SettingField::default()
        };
        if field_type == "select" {
            field.options = vec![
                SettingOption {
                    value: "fast".to_string(),
                    label: "Fast".to_string(),
                },
                SettingOption {
                    value: "slow".to_string(),
                    label: "Slow".to_string(),
                },
            ];
        }
        settings.push(field);
    }
    Manifest {
        settings,
        ..Manifest::default()
    }
}

fn random_setting_value(rng: &mut Rng, field_type: &str) -> String {
    match field_type {
        "boolean" => rng.pick(&["true", "false", "TRUE", "1", ""]).to_string(),
        "number" => rng
            .pick(&["1", "-2.5", "NaN", "abc", "1e400", "0x1p2", "", " 7 "])
            .to_string(),
        "select" => rng.pick(&["fast", "slow", "medium", ""]).to_string(),
        "secret" => {
            if rng.chance(20) {
                String::new()
            } else if rng.chance(10) {
                "a".repeat(600)
            } else {
                rng.pick(&["sk-new", "  padded  ", "x"]).to_string()
            }
        }
        _ => {
            if rng.chance(10) {
                "a".repeat(320)
            } else {
                rng.pick(&["", "value", "  padded  ", "x"]).to_string()
            }
        }
    }
}

fn number_cases() -> Vec<String> {
    let mut values: Vec<String> = [
        // Review cases and parser boundaries.
        "+NaN",
        "-NaN",
        "NaN",
        "nan",
        "NAN",
        "Inf",
        "+Inf",
        "-Inf",
        "inf",
        "Infinity",
        "infinity",
        "+Infinity",
        "-infinity",
        "+inf",
        "-inf",
        "Infinityx",
        "nana",
        "0x_1p0",
        "0x_1.8p0",
        "0x1_p0",
        "0x1_p",
        "0x1p_0",
        "0x1p0_0",
        "0x1p+_0",
        "0x1p-_0",
        "0x1p-1024",
        "0x1p-1074",
        "0x1p-1075",
        "0x1p-2000",
        "0x0.0000000000001p1024",
        "0x1.fffffffffffff8p1023",
        "0x1.fffffffffffffp1023",
        "0x1p1024",
        "0x2p1023",
        "0x0p99999",
        "0x0p-99999",
        "0x.8p1",
        "0x8.p1",
        "0x.p1",
        "0x",
        "0x1",
        "0x1.",
        "1_000",
        "1_0.5",
        "1__0",
        "1_._5",
        "1._5",
        "1_.5",
        "_1",
        "1_",
        "1e1_0",
        "1e_10",
        "1e10_",
        ".5",
        "1.",
        ".",
        "+.5",
        "-.5",
        "+.",
        "0x1p+3",
        "0x1p-3",
        "0X1P3",
        "1e400",
        "1e-400",
        "1e308",
        "1e309",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "",
        "+",
        "-",
        "1,5",
        "1e",
        "e3",
    ]
    .iter()
    .map(|value| (*value).to_string())
    .collect();

    for sign in ["", "+", "-"] {
        for mantissa in [
            "0",
            "1",
            "0.0",
            "1.5",
            ".5",
            "1.",
            "01",
            "1_000",
            "1_0.5",
            "0.0001",
            "12345678901234567890",
            "1.7976931348623157",
            "1.7976931348623158",
            "1.797693134862315807",
            "1.7976931348623159",
            "0.9999999999999999",
            "0.99999999999999999",
        ] {
            for exponent in [
                "", "e0", "e1", "e-1", "e+30", "e308", "e309", "e400", "e-400", "e99999",
                "e-99999", "e1_0",
            ] {
                values.push(format!("{sign}{mantissa}{exponent}"));
            }
        }
    }

    for sign in ["", "+", "-"] {
        for mantissa in [
            "0x0",
            "0x1",
            "0x_1",
            "0x1.8",
            "0x.8",
            "0x8.",
            "0x1_0",
            "0x1.0_1",
            "0x1.fffffffffffff",
            "0x1.fffffffffffff0",
            "0x1.fffffffffffff1",
            "0x1.fffffffffffff7",
            "0x1.fffffffffffff8",
            "0x1.fffffffffffff9",
            "0x1.fffffffffffffff",
            "0x1.0000000000001",
            "0x1.00000000000008",
            "0x1.00000000000018",
            "0x1.fffffffffffff0001",
            "0x1.0000000000000001",
            "0x0.0000000000001",
            "0x1fffffffffffff",
            "0x1fffffffffffff0",
            "0x.00000000000001",
        ] {
            for exponent in [
                "", "p0", "p1", "P-1", "p1023", "p1024", "p-1023", "p-1024", "p-1074", "p-1075",
                "p-2000", "p99999", "p-99999", "p0_0", "p+3", "p-3",
            ] {
                values.push(format!("{sign}{mantissa}{exponent}"));
            }
        }
    }

    let mut rng = Rng::new(0x000F_10A7);
    let alphabet = b"0123456789abcdefxXpPeE_+-.nNiIfFtTyY";
    while values.len() < 3000 {
        let length = rng.below(28);
        let mut text = String::with_capacity(length);
        for _ in 0..length {
            text.push(char::from(alphabet[rng.below(alphabet.len())]));
        }
        values.push(text);
    }
    values.sort();
    values.dedup();
    values
}

fn random_values(rng: &mut Rng, manifest: &Manifest) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for field in &manifest.settings {
        if rng.chance(72) {
            values.insert(
                field.key.clone(),
                random_setting_value(rng, &field.field_type),
            );
        }
    }
    if rng.chance(25) {
        values.insert("unknown".to_string(), "x".to_string());
    }
    values
}

fn pair_map(values: &[(&str, &str)]) -> HashMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[tokio::test]
async fn differential_registry_and_settings() {
    let dsn = dsn();
    let mut oracle = Oracle::spawn(&dsn);

    // Registry scenarios.
    let mut cases: Vec<(Value, bool)> = Vec::new();
    for seed in 0..STANDARD_SCENARIOS {
        cases.push((standard_scenario(seed), false));
    }
    for seed in 0..CYCLE_SCENARIOS {
        cases.push((cycle_scenario(seed), true));
    }
    let edge_cases = edge_scenarios();
    let edge_count = edge_cases.len();
    for scenario in edge_cases {
        cases.push((scenario, false));
    }
    assert!(
        cases.len() >= 200,
        "expected at least 200 registry scenarios, got {}",
        cases.len()
    );
    println!(
        "differential: {} registry scenarios ({STANDARD_SCENARIOS} standard, {CYCLE_SCENARIOS} cycle, {edge_count} edge), {MERGE_CASES} merge, {MASK_CASES} mask, {COMPLETE_CASES} complete, {ENCRYPT_CASES} encryption, 4 storage cases",
        cases.len()
    );

    let requests: Vec<Value> = cases.iter().map(|(request, _)| request.clone()).collect();
    let responses = oracle.batch(&requests);
    for (index, ((request, cycle), oracle_value)) in cases.iter().zip(responses).enumerate() {
        let scenario: Scenario =
            serde_json::from_value(request.clone()).expect("decode scenario fixture");
        let mut rust_value = run_scenario(&scenario);
        let mut oracle_value = oracle_value;
        normalise_ids(&mut rust_value, &mut oracle_value);
        if *cycle {
            compare_cycle_results(index, &rust_value, &oracle_value);
        } else {
            assert_eq!(
                rust_value, oracle_value,
                "registry scenario {index} differs\nfixture: {request}"
            );
        }
    }

    // Merge, mask and completeness.
    let mut merge_requests = Vec::new();
    let mut merge_fixtures = Vec::new();
    let mut rng = Rng::new(0x5EED);
    for _ in 0..MERGE_CASES {
        let manifest = random_setting_manifest(&mut rng);
        let existing = random_values(&mut rng, &manifest);
        let incoming = random_values(&mut rng, &manifest);
        merge_requests.push(json!({
            "op": "merge",
            "manifest": manifest,
            "existing": existing,
            "incoming": incoming,
        }));
        merge_fixtures.push((manifest, existing, incoming));
    }
    let merge_responses = oracle.batch(&merge_requests);
    for (index, (response, (manifest, existing, incoming))) in
        merge_responses.iter().zip(&merge_fixtures).enumerate()
    {
        let (merged, fields) = azahar_plugins::merge_settings(manifest, existing, incoming);
        let expected = json!({"merged": merged, "fields": fields});
        assert_eq!(
            &expected, response,
            "merge case {index} differs\nfixture: {manifest:?} {existing:?} {incoming:?}"
        );
    }

    let mut mask_requests = Vec::new();
    let mut mask_fixtures = Vec::new();
    for _ in 0..MASK_CASES {
        let manifest = random_setting_manifest(&mut rng);
        let values = random_values(&mut rng, &manifest);
        mask_requests.push(json!({"op": "mask", "manifest": manifest, "values": values}));
        mask_fixtures.push((manifest, values));
    }
    let mask_responses = oracle.batch(&mask_requests);
    for (index, (response, (manifest, values))) in
        mask_responses.iter().zip(&mask_fixtures).enumerate()
    {
        let (masked, secrets_set) = azahar_plugins::mask_settings(manifest, values);
        let expected = json!({"masked": masked, "secrets_set": secrets_set});
        assert_eq!(&expected, response, "mask case {index} differs");
    }

    let mut complete_requests = Vec::new();
    let mut complete_fixtures = Vec::new();
    for _ in 0..COMPLETE_CASES {
        let manifest = random_setting_manifest(&mut rng);
        let values = random_values(&mut rng, &manifest);
        complete_requests.push(json!({
            "op": "complete",
            "manifest": manifest,
            "values": values,
        }));
        complete_fixtures.push((manifest, values));
    }
    let complete_responses = oracle.batch(&complete_requests);
    for (index, (response, (manifest, values))) in complete_responses
        .iter()
        .zip(&complete_fixtures)
        .enumerate()
    {
        let complete = azahar_plugins::settings_complete(manifest, values);
        assert_eq!(
            &json!({"complete": complete}),
            response,
            "complete case {index} differs"
        );
    }

    // Numeric acceptance parity with strconv.ParseFloat.
    let numbers = number_cases();
    assert!(
        numbers.len() >= 2000,
        "expected at least 2000 numeric probes, got {}",
        numbers.len()
    );
    let accepted = go_float_acceptance(&numbers);
    assert_eq!(accepted.len(), numbers.len());
    let number_manifest = Manifest {
        settings: vec![SettingField {
            key: "count".to_string(),
            label: "Count".to_string(),
            field_type: "number".to_string(),
            ..SettingField::default()
        }],
        ..Manifest::default()
    };
    for (value, expected) in numbers.iter().zip(accepted) {
        let (_, fields) = azahar_plugins::merge_settings(
            &number_manifest,
            &HashMap::new(),
            &HashMap::from([("count".to_string(), value.clone())]),
        );
        assert_eq!(
            fields.is_empty(),
            expected,
            "number acceptance mismatch for {value:?}"
        );
    }
    println!("numeric acceptance: {} strings", numbers.len());

    // Encryption compatibility. A database handle is only needed to build an
    // Env; the settings read/write checks below use the same store.
    let database = open(&dsn)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "settings differential needs libSQL at {dsn}: {error}\nrun `make dev-db` or set AZAHAR_TEST_DATABASE_URL"
            )
        });
    let connection = database.connect().expect("database connection");
    azahar_db::migrate(&connection)
        .await
        .unwrap_or_else(|error| panic!("migrate {dsn}: {error}"));
    let store = Arc::new(Store::new(database).expect("store"));

    let keys = ["differential-key-a", "differential-key-b"];
    let mut encode_requests = Vec::new();
    let mut encode_fixtures = Vec::new();
    for index in 0..ENCRYPT_CASES {
        let key = keys[index % keys.len()];
        let mut values = HashMap::new();
        values.insert("api_key".to_string(), format!("sk-{index}"));
        values.insert("unicode".to_string(), format!("café {} ☕", index));
        if index % 3 == 0 {
            values.insert("empty".to_string(), String::new());
        }
        encode_requests.push(json!({"op": "encode", "secret_key": key, "values": values}));
        encode_fixtures.push((key, values));
    }
    let encode_responses = oracle.batch(&encode_requests);
    for (index, (response, (key, values))) in
        encode_responses.iter().zip(&encode_fixtures).enumerate()
    {
        let encrypted = response["encrypted"]
            .as_str()
            .unwrap_or_else(|| panic!("encode case {index} failed: {response}"));
        let plain = SecretBox::new(key)
            .decrypt(encrypted)
            .unwrap_or_else(|error| panic!("decrypt Go ciphertext {index}: {error}"));
        let decoded: HashMap<String, String> =
            serde_json::from_slice(&plain).expect("decode Go plaintext");
        assert_eq!(decoded, *values, "encode case {index} differs");
    }

    let env = Env::new(Arc::clone(&store), keys[0]);
    let mut decode_requests = Vec::new();
    let mut decode_fixtures = Vec::new();
    for (key, values) in &encode_fixtures {
        let encrypted = Env::new(Arc::clone(&store), *key)
            .encode_settings(values)
            .expect("encrypt settings for Go");
        decode_requests.push(json!({
            "op": "decode",
            "secret_key": key,
            "encrypted": encrypted,
        }));
        decode_fixtures.push(values.clone());
    }
    let decode_responses = oracle.batch(&decode_requests);
    for (index, (response, values)) in decode_responses.iter().zip(&decode_fixtures).enumerate() {
        let error = response.get("error").and_then(Value::as_str);
        assert_eq!(error, None, "decode case {index} failed: {response}");
        let decoded: HashMap<String, String> =
            serde_json::from_value(response["values"].clone()).expect("decode values");
        assert_eq!(&decoded, values, "decode case {index} differs");
    }

    // Storage path: rows written by either side are readable by the other.
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let suffix = format!("{unique}-{}", std::process::id());
    let go_written = format!("zz-diff-go-{suffix}");
    let rust_written = format!("zz-diff-rust-{suffix}");
    let wrong_key_row = format!("zz-diff-wrong-{suffix}");
    let missing_row = format!("zz-diff-missing-{suffix}");

    let go_values = pair_map(&[
        ("api_key", "sk-go"),
        ("model", "gpt-4o-mini"),
        ("enabled", "true"),
    ]);
    let now_text = "2026-05-06T07:08:09Z";
    let save = json!({
        "op": "save",
        "secret_key": keys[0],
        "plugin_id": go_written,
        "values": go_values,
        "now": now_text,
    });
    let response = oracle.batch(&[save]).remove(0);
    assert_eq!(response, json!({"ok": true}), "Go save failed: {response}");

    let (loaded, updated_at) = env
        .settings_with_time(&go_written)
        .await
        .expect("load Go-written settings");
    assert_eq!(loaded, go_values);
    assert_eq!(
        updated_at,
        Some(
            DateTime::parse_from_rfc3339(now_text)
                .expect("timestamp")
                .with_timezone(&Utc)
        )
    );

    let rust_values = pair_map(&[
        ("api_key", "sk-rust"),
        ("model", "gpt-4.1-mini"),
        ("enabled", "false"),
    ]);
    let rust_now = DateTime::parse_from_rfc3339("2026-06-07T08:09:10Z")
        .expect("timestamp")
        .with_timezone(&Utc);
    let payload = env
        .encode_settings(&rust_values)
        .expect("encrypt Rust settings");
    store
        .save_plugin_settings(store.conn(), &rust_written, &payload, rust_now)
        .await
        .expect("save Rust-written settings");
    let load = json!({
        "op": "load",
        "secret_key": keys[0],
        "plugin_id": rust_written,
    });
    let response = oracle.batch(&[load]).remove(0);
    assert_eq!(response["error"], Value::Null, "Go load failed: {response}");
    let decoded: HashMap<String, String> =
        serde_json::from_value(response["values"].clone()).expect("decode Go-loaded values");
    assert_eq!(decoded, rust_values);
    assert_eq!(response["updated_at"], json!(format_time(&rust_now)));

    // A different key surfaces the shared unreadable message on both sides.
    let save = json!({
        "op": "save",
        "secret_key": keys[0],
        "plugin_id": wrong_key_row,
        "values": go_values,
        "now": now_text,
    });
    assert_eq!(
        oracle.batch(&[save]).remove(0),
        json!({"ok": true}),
        "save for wrong-key case"
    );
    let other = Env::new(Arc::clone(&store), keys[1]);
    assert_eq!(
        other
            .settings(&wrong_key_row)
            .await
            .expect_err("wrong key")
            .to_string(),
        UNREADABLE_MESSAGE
    );
    let load = json!({
        "op": "load",
        "secret_key": keys[1],
        "plugin_id": wrong_key_row,
    });
    assert_eq!(
        oracle.batch(&[load]).remove(0),
        json!({"error": UNREADABLE_MESSAGE}),
        "Go wrong-key load"
    );

    // A missing row is an empty map and a zero timestamp.
    assert!(
        env.settings(&missing_row)
            .await
            .expect("missing")
            .is_empty()
    );
    let load = json!({
        "op": "load",
        "secret_key": keys[0],
        "plugin_id": missing_row,
    });
    assert_eq!(
        oracle.batch(&[load]).remove(0),
        json!({"values": {}, "updated_at": ""}),
        "Go missing-row load"
    );

    for id in [&go_written, &rust_written, &wrong_key_row] {
        store
            .conn()
            .execute(
                "DELETE FROM plugin_settings WHERE plugin_id = ?",
                params![id.as_str()],
            )
            .await
            .expect("clean up settings rows");
    }
}

fn compare_cycle_results(index: usize, rust: &Value, oracle: &Value) {
    let rust_steps = rust["steps"].as_array().expect("rust steps");
    let oracle_steps = oracle["steps"].as_array().expect("oracle steps");
    assert_eq!(rust_steps.len(), oracle_steps.len());
    for (step, (rust_step, oracle_step)) in rust_steps.iter().zip(oracle_steps).enumerate() {
        match oracle_step.get("error").and_then(Value::as_str) {
            Some(message) if message.starts_with("plugin dependency cycle includes ") => {
                let rust_message = rust_step["error"].as_str().unwrap_or_else(|| {
                    panic!("scenario {index} step {step}: Rust accepted a cycle")
                });
                assert!(
                    rust_message.starts_with("plugin dependency cycle includes "),
                    "scenario {index} step {step}: expected a cycle error, got {rust_message}"
                );
            }
            _ => assert_eq!(
                rust_step, oracle_step,
                "scenario {index} step {step} differs"
            ),
        }
    }
}

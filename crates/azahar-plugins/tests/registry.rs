//! Registry rules: manifest validation, dependency bookkeeping and ordering.

use std::collections::HashMap;
use std::sync::Arc;

use azahar_plugins::{
    Category, Manifest, Migration, OptionField, Plugin, ProductType, Registry, RegistryError,
    Route, SettingField, SettingOption, StorefrontPage,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct FakePlugin {
    manifest: Manifest,
    routes: Vec<Route>,
    migrations: Vec<Migration>,
    validate_settings: Option<HashMap<String, String>>,
}

impl FakePlugin {
    fn new(id: &str) -> Self {
        Self {
            manifest: Manifest {
                id: id.to_string(),
                name: id.to_string(),
                version: "1.0.0".to_string(),
                ..Manifest::default()
            },
            routes: Vec::new(),
            migrations: Vec::new(),
            validate_settings: None,
        }
    }

    fn with_dependencies(id: &str, dependencies: &[&str]) -> Self {
        let mut plugin = Self::new(id);
        plugin.manifest.dependencies = dependencies.iter().map(|value| value.to_string()).collect();
        plugin
    }
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

    fn as_settings_validator(&self) -> Option<&dyn azahar_plugins::SettingsValidator> {
        if self.validate_settings.is_some() {
            Some(self)
        } else {
            None
        }
    }
}

impl azahar_plugins::SettingsValidator for FakePlugin {
    fn validate_settings(&self, _settings: &HashMap<String, String>) -> HashMap<String, String> {
        self.validate_settings.clone().unwrap_or_default()
    }
}

fn boxed(plugin: FakePlugin) -> Arc<dyn Plugin> {
    Arc::new(plugin)
}

fn registered(plugins: &[FakePlugin]) -> Registry {
    let mut registry = Registry::new();
    for plugin in plugins {
        registry
            .register(boxed(plugin.clone()))
            .expect("register fixture plugin");
    }
    registry
}

fn error(result: Result<(), RegistryError>) -> String {
    result.expect_err("registration must fail").to_string()
}

fn valid_manifest(id: &str) -> Manifest {
    Manifest {
        id: id.to_string(),
        name: "Sample".to_string(),
        version: "1.0.0".to_string(),
        ..Manifest::default()
    }
}

fn register_manifest(manifest: Manifest) -> Result<(), RegistryError> {
    Registry::new().register(boxed(FakePlugin {
        manifest,
        ..FakePlugin::new("unused")
    }))
}

#[test]
fn registers_and_looks_up() {
    let mut plugin = FakePlugin::new("sample");
    plugin.manifest.name = "Sample".to_string();
    plugin.manifest.product_type = Some(ProductType {
        label: "Sample product".to_string(),
        fields: vec![OptionField {
            key: "size".to_string(),
            label: "Size".to_string(),
            field_type: "select".to_string(),
            required: true,
            ..OptionField::default()
        }],
        ..ProductType::default()
    });
    plugin.manifest.settings = vec![SettingField {
        key: "api_key".to_string(),
        label: "API key".to_string(),
        field_type: "secret".to_string(),
        required: true,
        ..SettingField::default()
    }];
    plugin.routes = vec![
        Route {
            method: "GET".to_string(),
            pattern: "/items".to_string(),
            admin: false,
        },
        Route {
            method: "POST".to_string(),
            pattern: "/items".to_string(),
            admin: true,
        },
    ];

    let mut registry = Registry::new();
    registry.register(boxed(plugin)).expect("register");

    assert!(registry.get("sample").is_some());
    assert!(registry.get("missing").is_none());
    assert!(registry.product_type("sample").is_some());
    assert_eq!(registry.all().len(), 1);
    assert_eq!(registry.manifests().len(), 1);
    assert!(registry.enabled("sample"));
    assert!(registry.effective_enabled("sample"));
}

#[test]
fn rejects_invalid_ids() {
    for id in [
        "",
        "a",
        "1a",
        "Aaaa",
        "-abc",
        "_abc",
        "a b",
        &"a".repeat(33),
        "éé",
    ] {
        let message = error(register_manifest(valid_manifest(id)));
        assert!(
            message.starts_with(&format!("register plugin \"{id}\": id must be")),
            "{message}"
        );
    }
    assert!(register_manifest(valid_manifest("ab")).is_ok());
    assert!(register_manifest(valid_manifest(&"a".repeat(32))).is_ok());
    assert!(register_manifest(valid_manifest("a-b_c9")).is_ok());
}

#[test]
fn quotes_invalid_ids_like_go() {
    let message = error(register_manifest(valid_manifest("bad\"id")));
    assert_eq!(
        message,
        "register plugin \"bad\\\"id\": id must be 2 to 32 lowercase letters, numbers, dashes or underscores"
    );
}

#[test]
fn rejects_duplicate_id() {
    let mut registry = Registry::new();
    registry
        .register(boxed(FakePlugin::new("sample")))
        .expect("first");
    let message = error(registry.register(boxed(FakePlugin::new("sample"))));
    assert_eq!(message, "register plugin \"sample\": already registered");
}

#[test]
fn name_bounds() {
    let mut manifest = valid_manifest("sample");
    manifest.name = "   ".to_string();
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": name must be 1 to 80 characters"
    );
    manifest.name = "é".repeat(81);
    assert!(error(register_manifest(manifest)).ends_with("name must be 1 to 80 characters"));
    let mut accepted = valid_manifest("sample");
    accepted.name = "é".repeat(80);
    assert!(register_manifest(accepted).is_ok());
}

#[test]
fn version_bounds_are_bytes() {
    let mut manifest = valid_manifest("sample");
    manifest.version = " ".to_string();
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": version must be 1 to 32 characters"
    );
    manifest.version = "é".repeat(17);
    assert!(error(register_manifest(manifest)).ends_with("version must be 1 to 32 characters"));
    let mut accepted = valid_manifest("sample");
    accepted.version = "é".repeat(16);
    assert!(register_manifest(accepted).is_ok());
}

#[test]
fn admin_path_prefix() {
    let mut manifest = valid_manifest("sample");
    manifest.admin_path = "/plugins/sample".to_string();
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": admin_path must start with /admin/"
    );
    manifest.admin_path = "/admin".to_string();
    assert!(register_manifest(manifest).is_err());
    let mut accepted = valid_manifest("sample");
    accepted.admin_path = "/admin/plugins/sample".to_string();
    assert!(register_manifest(accepted).is_ok());
}

#[test]
fn dependency_declarations() {
    let mut manifest = valid_manifest("sample");
    manifest.dependencies = vec!["Bad Id".to_string()];
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": dependency \"Bad Id\" must be a plugin id"
    );
    manifest.dependencies = vec!["sample".to_string()];
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": a plugin cannot depend on itself"
    );
    manifest.dependencies = vec!["base".to_string(), "base".to_string()];
    assert_eq!(
        error(register_manifest(manifest.clone())),
        "register plugin \"sample\": dependency \"base\" is repeated"
    );
    manifest.dependencies = vec![" base ".to_string()];
    assert!(register_manifest(manifest).is_ok());
}

#[test]
fn validate_dependencies_rejects_unknown() {
    let registry = registered(&[FakePlugin::with_dependencies("top", &["missing"])]);
    assert_eq!(
        registry
            .validate_dependencies()
            .expect_err("unknown dependency")
            .to_string(),
        "plugin \"top\" depends on unknown plugin \"missing\""
    );
}

#[test]
fn validate_dependencies_uses_untrimmed_declarations() {
    // Register trims a dependency for its own checks but stores the original
    // value, so validation later looks up the padded id. Same quirk as Go.
    let registry = registered(&[
        FakePlugin::new("base"),
        FakePlugin::with_dependencies("top", &[" base "]),
    ]);
    assert_eq!(
        registry
            .validate_dependencies()
            .expect_err("padded dependency")
            .to_string(),
        "plugin \"top\" depends on unknown plugin \" base \""
    );
}

#[test]
fn validate_dependencies_accepts_a_dag_and_rejects_cycles() {
    let registry = registered(&[
        FakePlugin::new("base"),
        FakePlugin::with_dependencies("middle", &["base"]),
        FakePlugin::with_dependencies("top", &["middle"]),
    ]);
    registry.validate_dependencies().expect("valid graph");

    let registry = registered(&[
        FakePlugin::with_dependencies("alpha", &["beta"]),
        FakePlugin::with_dependencies("beta", &["alpha"]),
    ]);
    let message = registry
        .validate_dependencies()
        .expect_err("cycle")
        .to_string();
    assert!(
        message == "plugin dependency cycle includes \"alpha\""
            || message == "plugin dependency cycle includes \"beta\"",
        "{message}"
    );

    let registry = registered(&[
        FakePlugin::with_dependencies("alpha", &["beta"]),
        FakePlugin::with_dependencies("beta", &["gamma"]),
        FakePlugin::with_dependencies("gamma", &["alpha"]),
    ]);
    let message = registry
        .validate_dependencies()
        .expect_err("three-node cycle")
        .to_string();
    assert!(
        message.starts_with("plugin dependency cycle includes \""),
        "{message}"
    );
    assert!(
        ["alpha", "beta", "gamma"]
            .iter()
            .any(|id| message.contains(&format!("\"{id}\""))),
        "{message}"
    );
}

#[test]
fn product_type_fields() {
    let with_type = |product_type: ProductType| {
        let mut manifest = valid_manifest("sample");
        manifest.product_type = Some(product_type);
        register_manifest(manifest)
    };
    let field = |key: &str, label: &str, field_type: &str| OptionField {
        key: key.to_string(),
        label: label.to_string(),
        field_type: field_type.to_string(),
        ..OptionField::default()
    };

    assert_eq!(
        error(with_type(ProductType {
            label: " ".to_string(),
            ..ProductType::default()
        })),
        "register plugin \"sample\": product type label must be 1 to 60 characters"
    );
    assert!(
        error(with_type(ProductType {
            label: "x".repeat(61),
            ..ProductType::default()
        }))
        .ends_with("product type label must be 1 to 60 characters")
    );

    assert!(
        error(with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![field("Storage Size", "Size", "text")],
            ..ProductType::default()
        }))
        .ends_with(
            "option field \"Storage Size\" must be lowercase letters, numbers and underscores"
        )
    );

    assert!(
        error(with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![
                field("size", "Size", "text"),
                field("size", "Again", "text")
            ],
            ..ProductType::default()
        }))
        .ends_with("option field \"size\" is repeated")
    );

    assert!(
        error(with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![field("size", " ", "text")],
            ..ProductType::default()
        }))
        .ends_with("option field \"size\" needs a label of 1 to 60 characters")
    );

    assert!(
        error(with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![field("size", "Size", "color")],
            ..ProductType::default()
        }))
        .ends_with("option field \"size\" has an unsupported type \"color\"")
    );

    let mut many = field("size", "Size", "select");
    many.suggestions = (0..13).map(|index| index.to_string()).collect();
    assert!(
        error(with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![many],
            ..ProductType::default()
        }))
        .ends_with("option field \"size\" has more than 12 suggestions")
    );

    assert!(
        with_type(ProductType {
            label: "Sample".to_string(),
            fields: vec![
                field("size", "Size", "text"),
                field("finish", "Finish", "select"),
                field("notes", "Notes", "textarea"),
                field("tags", "Tags", "list"),
            ],
            ..ProductType::default()
        })
        .is_ok()
    );
}

#[test]
fn category_rules() {
    let with_categories = |categories: Vec<Category>| {
        let mut manifest = valid_manifest("sample");
        manifest.categories = categories;
        register_manifest(manifest)
    };
    let category = |slug: &str, name: &str, description: &str| Category {
        slug: slug.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        sort_order: 0,
    };

    assert!(
        error(with_categories(vec![category("Bad Slug", "Name", "")])).ends_with(
            "category \"Bad Slug\" must be lowercase letters, numbers, dashes or underscores"
        )
    );
    assert!(
        error(with_categories(vec![
            category("face", "Face", ""),
            category("face", "Face again", ""),
        ]))
        .ends_with("category \"face\" is repeated")
    );
    assert!(
        error(with_categories(vec![category("face", " ", "")]))
            .ends_with("category \"face\" needs a name of 1 to 80 characters")
    );
    assert!(
        error(with_categories(vec![category(
            "face",
            "Face",
            &"x".repeat(301)
        )]))
        .ends_with("category \"face\" has a description over 300 characters")
    );

    assert!(with_categories(vec![category("face", &"é".repeat(80), &"x".repeat(300))]).is_ok());
}

#[test]
fn storefront_page_rules() {
    let with_pages = |pages: Vec<StorefrontPage>| {
        let mut manifest = valid_manifest("sample");
        manifest.storefront_pages = pages;
        register_manifest(manifest)
    };
    let page = |path: &str, label: &str| StorefrontPage {
        path: path.to_string(),
        label: label.to_string(),
        sort_order: 0,
    };

    for path in [
        "relative",
        "/admin/panel",
        "/images/../x",
        "/images//x",
        "/with space",
        "/with\ttab",
        "/back\\slash",
        "/per%20cent",
        "/query?x=1",
        "/hash#x",
    ] {
        let message = error(with_pages(vec![page(path, "Page")]));
        assert!(
            message.ends_with(&format!(
                "storefront page path \"{}\" must be a clean public path",
                path.replace('\\', "\\\\").replace('\t', "\\t")
            )),
            "{path}: {message}"
        );
    }
    assert!(
        error(with_pages(vec![page(
            &format!("/{}", "x".repeat(200)),
            "Page"
        )]))
        .ends_with("must be a clean public path")
    );

    // "/admin" never reaches the reserved list: the prefix check rejects it
    // as an unclean path first, exactly like Go.
    assert!(
        error(with_pages(vec![page("/admin", "Page")]))
            .ends_with("storefront page path \"/admin\" must be a clean public path")
    );
    for path in [
        "/",
        "/shop",
        "/about",
        "/contact",
        "/cart",
        "/checkout",
        "/account",
        "/order",
        "/product",
    ] {
        assert!(
            error(with_pages(vec![page(path, "Page")])).ends_with(&format!(
                "storefront page path \"{path}\" is reserved by the core"
            ))
        );
    }

    assert!(
        error(with_pages(vec![
            page("/build", "Build"),
            page("/build", "Again")
        ]))
        .ends_with("storefront page path \"/build\" is repeated")
    );
    assert!(
        error(with_pages(vec![page("/build", " ")]))
            .ends_with("storefront page \"/build\" needs a label of 1 to 60 characters")
    );

    assert!(
        with_pages(vec![page(
            &format!("/{}", "x".repeat(199)),
            &"é".repeat(60)
        )])
        .is_ok()
    );
}

#[test]
fn categories_and_pages_are_owned_by_one_plugin() {
    let mut first = FakePlugin::new("first");
    first.manifest.categories = vec![Category {
        slug: "shared".to_string(),
        name: "Shared".to_string(),
        ..Category::default()
    }];
    first.manifest.storefront_pages = vec![StorefrontPage {
        path: "/shared".to_string(),
        label: "Shared".to_string(),
        ..StorefrontPage::default()
    }];

    let mut second = FakePlugin::new("second");
    second.manifest.categories = vec![Category {
        slug: "shared".to_string(),
        name: "Shared".to_string(),
        ..Category::default()
    }];
    let mut registry = Registry::new();
    registry.register(boxed(first)).expect("first");
    assert_eq!(
        error(registry.register(boxed(second))),
        "register plugin \"second\": category \"shared\" is already owned by \"first\""
    );

    let mut third = FakePlugin::new("third");
    third.manifest.storefront_pages = vec![StorefrontPage {
        path: "/shared".to_string(),
        label: "Shared".to_string(),
        ..StorefrontPage::default()
    }];
    assert_eq!(
        error(registry.register(boxed(third))),
        "register plugin \"third\": storefront page \"/shared\" is already owned by \"first\""
    );
}

#[test]
fn settings_field_rules() {
    let with_settings = |settings: Vec<SettingField>| {
        let mut manifest = valid_manifest("sample");
        manifest.settings = settings;
        register_manifest(manifest)
    };
    let setting = |key: &str, label: &str, field_type: &str| SettingField {
        key: key.to_string(),
        label: label.to_string(),
        field_type: field_type.to_string(),
        ..SettingField::default()
    };

    assert!(
        error(with_settings(vec![setting("API Key", "Key", "text")]))
            .ends_with("setting \"API Key\" must be lowercase letters, numbers and underscores")
    );
    assert!(
        error(with_settings(vec![
            setting("api_key", "Key", "text"),
            setting("api_key", "Again", "text"),
        ]))
        .ends_with("setting \"api_key\" is repeated")
    );
    assert!(
        error(with_settings(vec![setting("api_key", " ", "text")]))
            .ends_with("setting \"api_key\" needs a label of 1 to 80 characters")
    );
    assert!(
        error(with_settings(vec![setting("api_key", "Key", "color")]))
            .ends_with("setting \"api_key\" has an unsupported type \"color\"")
    );
    assert!(
        error(with_settings(vec![setting("mode", "Mode", "select")]))
            .ends_with("setting \"mode\" needs at least one option")
    );

    let mut negative = setting("api_key", "Key", "text");
    negative.max_length = -1;
    assert!(
        error(with_settings(vec![negative]))
            .ends_with("setting \"api_key\" has an invalid max length")
    );
    let mut oversized = setting("api_key", "Key", "text");
    oversized.max_length = 4097;
    assert!(
        error(with_settings(vec![oversized]))
            .ends_with("setting \"api_key\" has an invalid max length")
    );

    let mut select = setting("mode", "Mode", "select");
    select.options = vec![SettingOption {
        value: "a".to_string(),
        label: "A".to_string(),
    }];
    assert!(
        with_settings(vec![
            select,
            setting("api_key", "Key", "secret"),
            setting("count", "Count", "number"),
            setting("enabled", "Enabled", "boolean"),
        ])
        .is_ok()
    );
    assert!(with_settings(vec![setting(&"a".repeat(32), &"é".repeat(80), "text")]).is_ok());
}

#[test]
fn route_rules() {
    let with_routes = |routes: Vec<Route>| {
        let mut plugin = FakePlugin::new("sample");
        plugin.routes = routes;
        Registry::new().register(boxed(plugin))
    };
    let route = |method: &str, pattern: &str| Route {
        method: method.to_string(),
        pattern: pattern.to_string(),
        admin: false,
    };

    assert_eq!(
        error(with_routes(vec![route("HEAD", "/items")])),
        "register plugin \"sample\": route HEAD \"/items\" has an unsupported method"
    );
    assert!(with_routes(vec![route("get ", "/items")]).is_ok());

    for pattern in [
        "items",
        "/items/../x",
        "/items//x",
        "/with space",
        "/with\ttab",
    ] {
        let message = error(with_routes(vec![route("GET", pattern)]));
        assert!(
            message.contains("must be a clean absolute path"),
            "{pattern}: {message}"
        );
    }

    for admin in [true, false] {
        let mut plugin = FakePlugin::new("sample");
        plugin.routes = vec![Route {
            method: "GET".to_string(),
            pattern: "/admin/items".to_string(),
            admin,
        }];
        assert_eq!(
            error(Registry::new().register(boxed(plugin))),
            "register plugin \"sample\": route \"/admin/items\" must not include the /admin prefix; use Admin"
        );
    }

    assert_eq!(
        error(with_routes(vec![
            route("GET", "/items"),
            route("get", "/items"),
        ])),
        "register plugin \"sample\": duplicate route GET /items"
    );
}

#[test]
fn enable_state_and_dependencies() {
    let registry = &mut registered(&[
        FakePlugin::new("base"),
        FakePlugin::with_dependencies("middle", &["base"]),
        FakePlugin::with_dependencies("top", &["middle"]),
    ]);
    registry.set_states(&HashMap::from([
        ("base".to_string(), true),
        ("middle".to_string(), true),
        ("top".to_string(), true),
    ]));

    assert_eq!(registry.dependents("base"), vec!["middle", "top"]);
    assert!(registry.effective_enabled("top"));

    assert!(registry.set_enabled("base", false));
    assert!(registry.enabled("middle"));
    assert!(!registry.effective_enabled("middle"));
    assert!(!registry.effective_enabled("top"));
    assert_eq!(registry.enabled_dependents("base"), vec!["middle", "top"]);
    assert_eq!(registry.blocked_by("top"), vec!["base"]);

    assert!(registry.set_enabled("top", false));
    assert_eq!(registry.enabled_dependents("base"), vec!["middle"]);

    registry.set_states(&HashMap::from([("base".to_string(), true)]));
    // A row-less plugin returns to the default enabled state.
    assert!(registry.enabled("top"));
    assert!(registry.effective_enabled("top"));
    assert!(registry.enabled("base"));
    assert!(registry.effective_enabled("middle"));
}

#[test]
fn set_states_defaults_missing_plugins_to_enabled() {
    let registry = &mut registered(&[FakePlugin::new("base")]);
    registry.set_states(&HashMap::new());
    assert!(registry.enabled("base"));
    assert!(registry.effective_enabled("base"));
    registry.set_states(&HashMap::from([("unknown".to_string(), false)]));
    assert!(registry.enabled("base"));
}

#[test]
fn unknown_and_core_ids() {
    let registry = &mut registered(&[FakePlugin::new("sample")]);
    assert!(registry.enabled("missing"));
    assert!(registry.effective_enabled("missing"));
    assert!(registry.enabled(""));
    assert!(registry.enabled("core"));
    assert!(registry.effective_enabled("core"));
    assert!(registry.dependents("missing").is_empty());
    assert!(registry.enabled_dependents("missing").is_empty());
    assert!(registry.blocked_by("missing").is_empty());
    assert!(!registry.set_enabled("missing", false));
    assert!(registry.product_type("").is_none());
}

#[test]
fn dependents_are_sorted_and_blocked_by_keeps_duplicates() {
    let registry = &mut registered(&[
        FakePlugin::new("base"),
        FakePlugin::with_dependencies("zeta", &["base"]),
        FakePlugin::with_dependencies("alpha", &["base"]),
    ]);
    assert_eq!(registry.dependents("base"), vec!["alpha", "zeta"]);

    let registry = &mut registered(&[
        FakePlugin::new("cfg"),
        FakePlugin::with_dependencies("aa", &["cfg"]),
        FakePlugin::with_dependencies("bb", &["cfg"]),
        FakePlugin::with_dependencies("top", &["aa", "bb"]),
    ]);
    assert!(registry.set_enabled("cfg", false));
    // Go appends the disabled dependency once per path, so a diamond reports
    // it twice and only sorts when more than one entry exists.
    assert_eq!(registry.blocked_by("top"), vec!["cfg", "cfg"]);
    assert_eq!(registry.dependents("cfg"), vec!["aa", "bb", "top"]);
}

#[test]
fn product_type_lookup_is_exact() {
    let mut plugin = FakePlugin::new("sample");
    plugin.manifest.product_type = Some(ProductType {
        label: "Sample".to_string(),
        ..ProductType::default()
    });
    let registry = registered(&[plugin]);
    assert!(registry.product_type("sample").is_some());
    assert!(registry.product_type("SAMPLE").is_none());
}

#[test]
fn migrations_keep_registration_and_declaration_order() {
    let mut first = FakePlugin::new("first");
    first.migrations = vec![
        Migration {
            version: "001".to_string(),
            sql: "CREATE TABLE a (id TEXT);".to_string(),
        },
        Migration {
            version: "002".to_string(),
            sql: "CREATE TABLE b (id TEXT);".to_string(),
        },
    ];
    let second = FakePlugin::new("second");
    let registry = registered(&[first, second]);
    let migrations = registry.migrations();
    assert_eq!(migrations.len(), 2);
    assert_eq!(migrations[0].plugin_id, "first");
    assert_eq!(migrations[0].migration.version, "001");
    assert_eq!(migrations[1].migration.version, "002");
}

#[test]
fn manifests_follow_registration_order() {
    let registry = registered(&[
        FakePlugin::new("zeta"),
        FakePlugin::new("alpha"),
        FakePlugin::new("middle"),
    ]);
    let ids: Vec<String> = registry
        .manifests()
        .into_iter()
        .map(|manifest| manifest.id)
        .collect();
    assert_eq!(ids, vec!["zeta", "alpha", "middle"]);
}

#[test]
fn enrichment_and_settings_validators_are_optional() {
    let mut plugin = FakePlugin::new("sample");
    plugin.validate_settings = Some(HashMap::from([(
        "api_key".to_string(),
        "Bad key.".to_string(),
    )]));
    let registry = registered(&[plugin]);
    let stored = registry.get("sample").expect("plugin");
    let validator = stored
        .as_settings_validator()
        .expect("validator is exposed");
    assert_eq!(
        validator.validate_settings(&HashMap::new())["api_key"],
        "Bad key."
    );
    assert!(stored.as_product_enricher().is_none());
    assert!(registry.get("missing").is_none());
}

#[test]
fn empty_product_type_fields_serialise_as_null() {
    // Go's nil slice marshals as null; an empty non-nil one would be []. A Rust
    // Vec cannot carry that distinction, so null is the only shape emitted.
    let manifest = Manifest {
        id: "sample".to_string(),
        name: "Sample".to_string(),
        version: "1.0.0".to_string(),
        product_type: Some(ProductType {
            label: "Empty".to_string(),
            ..ProductType::default()
        }),
        ..Manifest::default()
    };
    let value = serde_json::to_value(&manifest).expect("serialize manifest");
    assert_eq!(value["product_type"]["fields"], Value::Null);
    assert_eq!(
        serde_json::from_value::<Manifest>(json!({
            "id": "sample",
            "name": "Sample",
            "version": "1.0.0",
            "product_type": {"label": "Empty", "fields": null},
        }))
        .expect("deserialize null fields")
        .product_type
        .expect("product type")
        .fields,
        Vec::new()
    );
}

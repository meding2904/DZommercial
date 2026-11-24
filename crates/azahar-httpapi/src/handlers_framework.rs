//! Admin plugin framework and storefront theme routes.
//!
//! Ported from `handlers_framework.go`. The public manifest route already lives
//! in [`crate::handlers_catalog`] and is not duplicated here.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use azahar_ids::new as new_id;
use azahar_plugins::{
    Env, Manifest, Plugin, SettingsError, mask_settings, merge_settings, settings_complete,
};
use azahar_store::{AuditEntry, StoreError, format_time};
use azahar_storefront::{Palette, Theme};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

use crate::dto::AdminPluginDto;
use crate::extract::JsonBody;
use crate::handlers_admin::audit_detail;
use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, json_response, problem, problem_fields};
use crate::state::AppState;
use crate::validate::fields as field_map;

fn plugin_env(state: &AppState) -> Env {
    Env::new(state.store.clone(), state.cfg.plugin_secret_key.clone())
}

fn plugin_from_path(state: &AppState, raw: &str) -> Option<Arc<dyn Plugin>> {
    let id = raw.trim();
    if id.is_empty() {
        return None;
    }
    state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(id)
}

fn plugin_not_found() -> ApiProblem {
    problem(404, "not_found", "Plugin not found.")
}

fn registry_state(state: &AppState, id: &str) -> (bool, bool, Vec<String>, Vec<String>) {
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (
        registry.enabled(id),
        registry.effective_enabled(id),
        registry.dependents(id),
        registry.blocked_by(id),
    )
}

fn plugin_names(state: &AppState, ids: &[String]) -> String {
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut names: Vec<String> = ids
        .iter()
        .map(|id| {
            registry
                .get(id)
                .map(|plugin| plugin.manifest().name)
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    names.sort();
    names.join(", ")
}

async fn admin_plugin_dto(state: &AppState, plugin: &Arc<dyn Plugin>) -> AdminPluginDto {
    let manifest = plugin.manifest();
    let (enabled, effective_enabled, dependents, blocked_by) = registry_state(state, &manifest.id);
    let mut dto = AdminPluginDto {
        manifest: manifest.clone(),
        configured: false,
        settings_state: "ready".to_string(),
        updated_at: None,
        enabled,
        effective_enabled,
        dependents,
        blocked_by,
    };
    match plugin_env(state).settings_with_time(&manifest.id).await {
        Ok((values, updated_at)) => {
            dto.configured = settings_complete(&manifest, &values);
            if let Some(updated_at) = updated_at {
                dto.updated_at = Some(format_time(&updated_at));
            }
        }
        Err(_) => dto.settings_state = "unreadable".to_string(),
    }
    dto
}

/// `GET /api/admin/plugins`.
pub async fn admin_plugins(State(state): State<AppState>) -> Response {
    let plugins = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .all();
    let mut result = Vec::with_capacity(plugins.len());
    for plugin in &plugins {
        result.push(admin_plugin_dto(&state, plugin).await);
    }
    json_response(200, &json!({"plugins": result}))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginStatePayload {
    pub enabled: Option<bool>,
    pub confirm_dependents: Option<bool>,
}

/// `PUT /api/admin/plugins/{pluginID}/state`.
pub async fn plugin_state(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    CurrentUser(current): CurrentUser,
    body: Result<JsonBody<PluginStatePayload>, ApiProblem>,
) -> Result<Response, ApiProblem> {
    // The Go handler resolves the plugin before it decodes the body.
    let Some(plugin) = plugin_from_path(&state, &plugin_id) else {
        return Err(plugin_not_found());
    };
    let payload = body?.payload;
    let Some(enabled) = payload.enabled else {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            field_map(&[("enabled", "Choose whether the plugin is enabled.")]),
        ));
    };
    let manifest = plugin.manifest();

    if enabled {
        let (_, _, _, blocked) = registry_state(&state, &manifest.id);
        if !blocked.is_empty() {
            let names = plugin_names(&state, &blocked);
            return Err(problem_fields(
                409,
                "plugin_dependency",
                &format!("Enable {names} before enabling {}.", manifest.name),
                field_map(&[("dependencies", names.as_str())]),
            ));
        }
    } else {
        let dependents = state
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enabled_dependents(&manifest.id);
        if !dependents.is_empty() && !payload.confirm_dependents.unwrap_or(false) {
            let names = plugin_names(&state, &dependents);
            return Err(problem_fields(
                409,
                "plugin_dependency_warning",
                &format!(
                    "These enabled plugins depend on {}: {names}. They will stop working until it is enabled again.",
                    manifest.name
                ),
                field_map(&[("dependents", names.as_str())]),
            ));
        }
    }

    let now = Utc::now();
    let (enabled_dependents, dependencies) = {
        let registry = state
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            registry.enabled_dependents(&manifest.id),
            registry.blocked_by(&manifest.id),
        )
    };
    let detail = audit_detail(&json!({
        "enabled": enabled,
        "dependencies": dependencies,
        "dependents": enabled_dependents,
    }));
    let action = if enabled {
        "plugin.enabled"
    } else {
        "plugin.disabled"
    };
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let saved = state
        .store
        .set_plugin_state(tx.conn(), &manifest.id, enabled, now)
        .await;
    let audited = match saved {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: action.to_string(),
                        entity_type: "plugin".to_string(),
                        entity_id: manifest.id.clone(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(internal_error());
        }
    }
    state
        .plugins
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .set_enabled(&manifest.id, enabled);
    Ok(json_response(
        200,
        &json!({"plugin": admin_plugin_dto(&state, &plugin).await}),
    ))
}

/// `GET /api/admin/plugins/{pluginID}/settings`.
pub async fn get_plugin_settings(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
) -> Result<Response, ApiProblem> {
    let Some(plugin) = plugin_from_path(&state, &plugin_id) else {
        return Err(plugin_not_found());
    };
    let manifest = plugin.manifest();
    let (values, updated_at) = match plugin_env(&state).settings_with_time(&manifest.id).await {
        Ok(value) => value,
        Err(SettingsError::Unreadable) => {
            return Err(problem(
                409,
                "plugin_settings_unreadable",
                "These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.",
            ));
        }
        Err(_) => return Err(internal_error()),
    };
    let (masked, secrets_set) = mask_settings(&manifest, &values);
    let masked: BTreeMap<String, String> = masked.into_iter().collect();
    let mut response = json!({
        "plugin_id": manifest.id,
        "settings": masked,
        "secrets_set": secrets_set,
        "configured": settings_complete(&manifest, &values),
    });
    if let Some(updated_at) = updated_at {
        response["updated_at"] = json!(format_time(&updated_at));
    }
    Ok(json_response(200, &response))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginSettingsPayload {
    /// Go decodes a null map value into the empty string.
    pub settings: Option<HashMap<String, Option<String>>>,
}

/// `PUT /api/admin/plugins/{pluginID}/settings`.
pub async fn put_plugin_settings(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    CurrentUser(current): CurrentUser,
    body: Result<JsonBody<PluginSettingsPayload>, ApiProblem>,
) -> Result<Response, ApiProblem> {
    let Some(plugin) = plugin_from_path(&state, &plugin_id) else {
        return Err(plugin_not_found());
    };
    let body = body?;
    let manifest = plugin.manifest();
    let env = plugin_env(&state);

    let existing = match env.settings(&manifest.id).await {
        Ok(values) => values,
        Err(SettingsError::Unreadable) => HashMap::new(),
        Err(_) => return Err(internal_error()),
    };
    let incoming: HashMap<String, String> = body
        .payload
        .settings
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, value.unwrap_or_default()))
        .collect();

    let (merged, mut fields) = merge_settings(&manifest, &existing, &incoming);
    if let Some(validator) = plugin.as_settings_validator() {
        fields.extend(validator.validate_settings(&merged));
    }
    if !fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields,
        ));
    }
    if !settings_complete(&manifest, &merged) {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            field_map(&[("settings", "Fill every required setting.")]),
        ));
    }

    let sealed = env.encode_settings(&merged).map_err(|_| internal_error())?;
    let now = Utc::now();
    let changed = changed_setting_keys(&manifest, &existing, &merged);
    let detail = audit_detail(&json!({"changed": changed}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let saved = state
        .store
        .save_plugin_settings(tx.conn(), &manifest.id, &sealed, now)
        .await;
    let audited = match saved {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "plugin.settings_update".to_string(),
                        entity_type: "plugin".to_string(),
                        entity_id: manifest.id.clone(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(internal_error());
        }
    }

    let (masked, secrets_set) = mask_settings(&manifest, &merged);
    let masked: BTreeMap<String, String> = masked.into_iter().collect();
    Ok(json_response(
        200,
        &json!({
            "plugin_id": manifest.id,
            "settings": masked,
            "secrets_set": secrets_set,
            "configured": settings_complete(&manifest, &merged),
            "updated_at": format_time(&now),
        }),
    ))
}

fn changed_setting_keys(
    manifest: &Manifest,
    before: &HashMap<String, String>,
    after: &HashMap<String, String>,
) -> Vec<String> {
    let mut changed = Vec::new();
    for field in &manifest.settings {
        let old = before.get(&field.key).map(String::as_str).unwrap_or("");
        let new = after.get(&field.key).map(String::as_str).unwrap_or("");
        if old != new {
            changed.push(field.key.clone());
        }
    }
    changed.sort();
    changed
}

// ---------------------------------------------------------------------------
// Storefront theme
// ---------------------------------------------------------------------------

async fn load_theme(state: &AppState) -> Result<Theme, StoreError> {
    match state.store.storefront_settings().await {
        Ok((raw, updated_at)) => {
            let mut theme = azahar_storefront::parse_lenient(&raw);
            theme.updated_at = azahar_store::format_time_or_zero(updated_at.as_ref());
            Ok(theme)
        }
        Err(StoreError::NotFound) => Ok(Theme::default()),
        Err(error) => Err(error),
    }
}

/// `GET /api/admin/storefront/theme`.
pub async fn admin_storefront_theme(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let theme = load_theme(&state).await.map_err(|_| internal_error())?;
    let encoded = azahar_storefront::encode(&theme).map_err(|_| internal_error())?;
    let defaults = azahar_storefront::encode(&Theme::default()).map_err(|_| internal_error())?;
    // Go encodes a map here, so encoding/json sorts the two keys.
    Ok(raw_json(format!(
        "{{\"defaults\":{defaults},\"theme\":{encoded}}}\n"
    )))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PalettePayload {
    ground: Option<String>,
    surface: Option<String>,
    ink: Option<String>,
    muted: Option<String>,
    accent: Option<String>,
    accent_ink: Option<String>,
}

impl PalettePayload {
    fn into_palette(self) -> Palette {
        Palette {
            ground: self.ground.unwrap_or_default(),
            surface: self.surface.unwrap_or_default(),
            ink: self.ink.unwrap_or_default(),
            muted: self.muted.unwrap_or_default(),
            accent: self.accent.unwrap_or_default(),
            accent_ink: self.accent_ink.unwrap_or_default(),
        }
    }
}

/// One theme request decoded the way Go decodes into a zero `Theme`: absent and
/// `null` fields stay at the zero value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemePayload {
    brand_name: Option<String>,
    tagline: Option<String>,
    description: Option<String>,
    hero_heading: Option<String>,
    hero_body: Option<String>,
    hero_alt: Option<String>,
    hero_cta: Option<String>,
    hero_image: Option<String>,
    statement: Option<String>,
    shelf_heading: Option<String>,
    shop_title: Option<String>,
    house_label: Option<String>,
    about_heading: Option<String>,
    about_body: Option<String>,
    about_image: Option<String>,
    about_alt: Option<String>,
    closing_heading: Option<String>,
    footer_note: Option<String>,
    footer_credit: Option<String>,
    contact_email: Option<String>,
    announcement: Option<String>,
    concerns_enabled: Option<bool>,
    font_display: Option<String>,
    font_body: Option<String>,
    font_scale: Option<f64>,
    radius: Option<i64>,
    light: Option<PalettePayload>,
    dark: Option<PalettePayload>,
    updated_at: Option<String>,
}

impl ThemePayload {
    fn into_theme(self) -> Theme {
        Theme {
            brand_name: self.brand_name.unwrap_or_default(),
            tagline: self.tagline.unwrap_or_default(),
            description: self.description.unwrap_or_default(),
            hero_heading: self.hero_heading.unwrap_or_default(),
            hero_body: self.hero_body.unwrap_or_default(),
            hero_alt: self.hero_alt.unwrap_or_default(),
            hero_cta: self.hero_cta.unwrap_or_default(),
            hero_image: self.hero_image.unwrap_or_default(),
            statement: self.statement.unwrap_or_default(),
            shelf_heading: self.shelf_heading.unwrap_or_default(),
            shop_title: self.shop_title.unwrap_or_default(),
            house_label: self.house_label.unwrap_or_default(),
            about_heading: self.about_heading.unwrap_or_default(),
            about_body: self.about_body.unwrap_or_default(),
            about_image: self.about_image.unwrap_or_default(),
            about_alt: self.about_alt.unwrap_or_default(),
            closing_heading: self.closing_heading.unwrap_or_default(),
            footer_note: self.footer_note.unwrap_or_default(),
            footer_credit: self.footer_credit.unwrap_or_default(),
            contact_email: self.contact_email.unwrap_or_default(),
            announcement: self.announcement.unwrap_or_default(),
            concerns_enabled: self.concerns_enabled.unwrap_or_default(),
            font_display: self.font_display.unwrap_or_default(),
            font_body: self.font_body.unwrap_or_default(),
            font_scale: self.font_scale.unwrap_or_default(),
            radius: self.radius.unwrap_or_default(),
            light: self.light.unwrap_or_default().into_palette(),
            dark: self.dark.unwrap_or_default().into_palette(),
            updated_at: self.updated_at.unwrap_or_default(),
        }
    }
}

/// `PUT /api/admin/storefront/theme`.
pub async fn put_storefront_theme(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
    body: JsonBody<ThemePayload>,
) -> Result<Response, ApiProblem> {
    let theme = body.payload.into_theme();
    let mut theme = azahar_storefront::normalize(&theme);
    theme.updated_at = String::new();
    let problems = azahar_storefront::validate(&theme);
    if !problems.is_empty() {
        let fields: HashMap<String, String> = problems.into_iter().collect();
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked storefront settings.",
            fields,
        ));
    }

    let payload = azahar_storefront::encode(&theme).map_err(|_| internal_error())?;
    let now = Utc::now();
    // Go ignores a failed previous read and diffs against a zero Theme.
    let previous = load_theme(&state)
        .await
        .unwrap_or_else(|_| ThemePayload::default().into_theme());
    let detail = audit_detail(&json!({
        "changed": changed_theme_fields(&previous, &theme),
    }));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let saved = state
        .store
        .save_storefront_settings(tx.conn(), &payload, now)
        .await;
    let audited = match saved {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "storefront.theme_update".to_string(),
                        entity_type: "storefront".to_string(),
                        entity_id: "default".to_string(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(internal_error());
        }
    }
    theme.updated_at = format_time(&now);
    let encoded = azahar_storefront::encode(&theme).map_err(|_| internal_error())?;
    Ok(raw_json(format!("{{\"theme\":{encoded}}}\n")))
}

fn changed_theme_fields(before: &Theme, after: &Theme) -> Vec<String> {
    let mut changed = Vec::new();
    macro_rules! compare {
        ($field:ident, $name:literal) => {
            if before.$field != after.$field {
                changed.push($name.to_string());
            }
        };
    }
    compare!(brand_name, "brand_name");
    compare!(tagline, "tagline");
    compare!(description, "description");
    compare!(hero_heading, "hero_heading");
    compare!(hero_body, "hero_body");
    compare!(hero_alt, "hero_alt");
    compare!(hero_cta, "hero_cta");
    compare!(hero_image, "hero_image");
    compare!(statement, "statement");
    compare!(shelf_heading, "shelf_heading");
    compare!(shop_title, "shop_title");
    compare!(house_label, "house_label");
    compare!(about_heading, "about_heading");
    compare!(about_body, "about_body");
    compare!(about_image, "about_image");
    compare!(about_alt, "about_alt");
    compare!(closing_heading, "closing_heading");
    compare!(footer_note, "footer_note");
    compare!(footer_credit, "footer_credit");
    compare!(contact_email, "contact_email");
    compare!(announcement, "announcement");
    compare!(concerns_enabled, "concerns_enabled");
    compare!(font_display, "font_display");
    compare!(font_body, "font_body");
    compare!(font_scale, "font_scale");
    compare!(radius, "radius");
    for (palette_before, palette_after, prefix) in [
        (&before.light, &after.light, "light"),
        (&before.dark, &after.dark, "dark"),
    ] {
        compare_palette(palette_before, palette_after, prefix, &mut changed);
    }
    changed.sort();
    changed
}

fn compare_palette(before: &Palette, after: &Palette, prefix: &str, changed: &mut Vec<String>) {
    macro_rules! compare {
        ($field:ident, $name:literal) => {
            if before.$field != after.$field {
                changed.push(format!("{prefix}.{}", $name));
            }
        };
    }
    compare!(ground, "ground");
    compare!(surface, "surface");
    compare!(ink, "ink");
    compare!(muted, "muted");
    compare!(accent, "accent");
    compare!(accent_ink, "accent_ink");
}

fn raw_json(body: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        Body::from(body),
    )
        .into_response()
}

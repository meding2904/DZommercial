//! Public catalogue routes: categories, products and the storefront theme,
//! plus the public plugin manifests. Ported from `handlers_catalog.go`,
//! `handlers_framework.go` and `product_options.go`.

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use azahar_store::{Product, ProductFilter, StoreError};
use serde_json::json;

use crate::dto::{CategoryDto, PublicPluginDto, to_product_async, to_products};
use crate::middleware::internal_error;
use crate::respond::{ApiProblem, json_response, problem};
use crate::state::AppState;
use crate::validate::truncate_runes;

/// Reads one query parameter the way Go's `url.Values.Get` does: percent
/// decoding per `form_urlencoded`, first occurrence wins, missing is empty.
pub(crate) fn query_param(raw: Option<&str>, name: &str) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    url::form_urlencoded::parse(raw.as_bytes())
        .find(|(key, _)| key.as_ref() == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn product_not_found() -> ApiProblem {
    problem(404, "not_found", "Product not found.")
}

/// `GET /api/categories` and `?all=1`.
pub async fn categories(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let all = query_param(raw.as_deref(), "all") == "1";
    let categories = if all {
        state.store.categories().await
    } else {
        state.store.public_categories().await
    }
    .map_err(|_| internal_error())?;

    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result: Vec<CategoryDto> = categories
        .iter()
        .filter(|category| all || registry.effective_enabled(&category.plugin_id))
        .map(CategoryDto::from)
        .collect();
    Ok(json_response(200, &json!({"categories": result})))
}

/// `GET /api/products`.
pub async fn products(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let mut filter = ProductFilter {
        category: query_param(raw.as_deref(), "category").trim().to_string(),
        concern: query_param(raw.as_deref(), "concern").trim().to_string(),
        query: query_param(raw.as_deref(), "q").trim().to_string(),
        sort: query_param(raw.as_deref(), "sort"),
        ..ProductFilter::default()
    };
    filter.query = truncate_runes(&filter.query, 100);

    let products = state
        .store
        .list_products(&filter)
        .await
        .map_err(|_| internal_error())?;

    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let visible: Vec<Product> = products
        .into_iter()
        .filter(|product| registry.effective_enabled(&product.plugin_id))
        .collect();
    Ok(json_response(
        200,
        &json!({"products": to_products(&registry, &visible, false)}),
    ))
}

/// `GET /api/products/{slug}`.
pub async fn product(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, ApiProblem> {
    let product = match state.store.product_by_slug(&slug, true).await {
        Ok(product) => product,
        Err(StoreError::NotFound) => return Err(product_not_found()),
        Err(_) => return Err(internal_error()),
    };

    {
        let registry = state
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !registry.effective_enabled(&product.plugin_id) {
            return Err(product_not_found());
        }
    }

    let related = state
        .store
        .related_products(&product.category.id, &product.id, 4)
        .await
        .map_err(|_| internal_error())?;

    let related = {
        let registry = state
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let visible: Vec<Product> = related
            .into_iter()
            .filter(|item| registry.effective_enabled(&item.plugin_id))
            .collect();
        to_products(&registry, &visible, false)
    };
    let detail = to_product_async(&state, &product, false).await;
    Ok(json_response(
        200,
        &json!({"product": detail, "related": related}),
    ))
}

/// `GET /api/plugins`: public manifests in registry order.
pub async fn plugins(State(state): State<AppState>) -> Response {
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result: Vec<PublicPluginDto> = registry
        .all()
        .iter()
        .map(|plugin| {
            let manifest = plugin.manifest();
            let enabled = registry.effective_enabled(&manifest.id);
            PublicPluginDto::from_manifest(&manifest, enabled)
        })
        .collect();
    json_response(200, &json!({"plugins": result}))
}

/// `GET /api/storefront/theme`.
///
/// The theme is serialised with the storefront crate's `encode`, so the float
/// `font_scale` and string escaping match Go's `encoding/json` byte for byte;
/// the result is then wrapped in the usual `{"theme": ...}` object.
pub async fn storefront_theme(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let theme = match state.store.storefront_settings().await {
        Ok((raw, updated_at)) => {
            let mut theme = azahar_storefront::parse_lenient(&raw);
            theme.updated_at = azahar_store::format_time_or_zero(updated_at.as_ref());
            theme
        }
        Err(StoreError::NotFound) => azahar_storefront::Theme::default(),
        Err(_) => return Err(internal_error()),
    };

    let encoded = azahar_storefront::encode(&theme).map_err(|_| internal_error())?;
    let body = format!("{{\"theme\":{encoded}}}\n");
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        Body::from(body),
    )
        .into_response())
}

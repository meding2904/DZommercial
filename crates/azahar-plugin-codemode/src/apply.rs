//! Applying a validated plan, one action at a time, inside a write
//! transaction.
//!
//! Port of `backend/internal/plugins/codemode/apply.go`. Every action writes an
//! audit row carrying the plan id. A failure returns an error whose message is
//! recorded in the plan's result column; the caller rolls the transaction back.

use std::collections::HashMap;
use std::sync::RwLock;

use azahar_orderintake::HookProvider;
use azahar_plugins::Registry;
use azahar_store::{AuditEntry, Product, Store, StoreError, format_time};
use azahar_storefront::Theme;
use chrono::{DateTime, Utc};
use libsql::Connection;
use libsql::params;

use crate::json;
use crate::plan::{
    Action, ProductDraft, parse_product_patch, product_fingerprint, validate_product_draft,
};

/// A failed action. `Stale` is the sentinel the handler maps to a 409.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{0}")]
    Message(String),
    #[error("codemode plan is stale")]
    Stale,
}

impl ApplyError {
    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl From<StoreError> for ApplyError {
    fn from(error: StoreError) -> Self {
        Self::Message(error.to_string())
    }
}

/// Applies one action and returns its message and the resulting theme.
#[allow(clippy::too_many_arguments)]
pub async fn apply_action(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    theme: Theme,
    baseline: &mut HashMap<String, String>,
    now: DateTime<Utc>,
    registry: Option<&RwLock<Registry>>,
    hooks: &dyn HookProvider,
) -> Result<(String, Theme), ApplyError> {
    match action.action_type.as_str() {
        "theme.update" => {
            apply_theme_update(store, conn, actor_user_id, plan_id, action, theme, now).await
        }
        "product.update" => {
            let message =
                apply_product_update(store, conn, actor_user_id, plan_id, action, baseline, now)
                    .await?;
            Ok((message, theme))
        }
        "product.archive" => {
            let message =
                apply_product_archive(store, conn, actor_user_id, plan_id, action, baseline, now)
                    .await?;
            Ok((message, theme))
        }
        "product.create" => {
            let message =
                apply_product_create(store, conn, actor_user_id, plan_id, action, now, registry)
                    .await?;
            Ok((message, theme))
        }
        "order.status" => {
            let message = apply_order_status(
                store,
                conn,
                actor_user_id,
                plan_id,
                action,
                baseline,
                now,
                hooks,
            )
            .await?;
            Ok((message, theme))
        }
        other => Err(ApplyError::message(format!(
            "unknown action type {}",
            json::quote(other)
        ))),
    }
}

async fn apply_theme_update(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    theme: Theme,
    now: DateTime<Utc>,
) -> Result<(String, Theme), ApplyError> {
    let mut next = crate::plan::apply_theme_patch(&theme, action.patch.as_deref())
        .map_err(ApplyError::Message)?;
    let payload = azahar_storefront::encode(&next)
        .map_err(|_| ApplyError::message("the theme could not be encoded"))?;
    if store
        .save_storefront_settings(conn, &payload, now)
        .await
        .is_err()
    {
        tracing::warn!("codemode theme could not be saved");
        return Err(ApplyError::message("the theme could not be saved"));
    }
    if insert_action_audit(
        store,
        conn,
        actor_user_id,
        "codemode.theme_update",
        "storefront",
        "default",
        plan_id,
        now,
    )
    .await
    .is_err()
    {
        return Err(ApplyError::message(
            "the theme audit entry could not be saved",
        ));
    }
    next.updated_at = format_time(&now);
    Ok(("Updated the storefront theme.".to_string(), next))
}

async fn apply_product_update(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    baseline: &mut HashMap<String, String>,
    now: DateTime<Utc>,
) -> Result<String, ApplyError> {
    let patch = parse_product_patch(action.patch.as_deref()).map_err(ApplyError::Message)?;
    let mut product = match store
        .product_by_id_tx(conn, &action.product_id, false)
        .await
    {
        Ok(product) => product,
        Err(StoreError::NotFound) => {
            return Err(ApplyError::message(format!(
                "product {} no longer exists",
                json::quote(&action.product_id)
            )));
        }
        Err(error) => {
            tracing::warn!(product_id = %action.product_id, error = %error, "codemode product could not be read");
            return Err(ApplyError::message("the product could not be read"));
        }
    };
    match baseline.get(&format!("product:{}", product.id)) {
        Some(want) if product_fingerprint(&product) == *want => {}
        _ => return Err(ApplyError::Stale),
    }

    if let Some(name) = patch.name {
        product.name = name;
    }
    if let Some(subtitle) = patch.subtitle {
        product.subtitle = subtitle;
    }
    if let Some(description) = patch.description {
        product.description = description;
    }
    if let Some(price_cents) = patch.price_cents {
        product.price_cents = price_cents;
    }
    if let Some(stock) = patch.stock {
        product.stock = i32::try_from(stock)
            .map_err(|_| ApplyError::message("the product could not be updated"))?;
    }
    if let Some(active) = patch.active {
        product.active = active;
    }
    if product.sale_price_cents.is_some() && !product.sale_window().price_valid(product.price_cents)
    {
        return Err(ApplyError::message(
            "the sale price must stay below the regular price",
        ));
    }
    product.updated_at = Some(now);

    if let Err(error) = store.update_product(conn, &product).await {
        if error.is_not_found() {
            return Err(ApplyError::message(format!(
                "product {} no longer exists",
                json::quote(&action.product_id)
            )));
        }
        tracing::warn!(product_id = %action.product_id, error = %error, "codemode product could not be updated");
        return Err(ApplyError::message("the product could not be updated"));
    }
    baseline.insert(
        format!("product:{}", product.id),
        product_fingerprint(&product),
    );
    if insert_action_audit(
        store,
        conn,
        actor_user_id,
        "codemode.product_update",
        "product",
        &product.id,
        plan_id,
        now,
    )
    .await
    .is_err()
    {
        return Err(ApplyError::message(
            "the product audit entry could not be saved",
        ));
    }
    Ok(format!("Updated {}.", json::quote(&product.name)))
}

async fn apply_product_archive(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    baseline: &mut HashMap<String, String>,
    now: DateTime<Utc>,
) -> Result<String, ApplyError> {
    let mut product = match store
        .product_by_id_tx(conn, &action.product_id, false)
        .await
    {
        Ok(product) => product,
        Err(StoreError::NotFound) => {
            return Err(ApplyError::message(format!(
                "product {} no longer exists",
                json::quote(&action.product_id)
            )));
        }
        Err(error) => {
            tracing::warn!(product_id = %action.product_id, error = %error, "codemode product could not be read");
            return Err(ApplyError::message("the product could not be read"));
        }
    };
    match baseline.get(&format!("product:{}", product.id)) {
        Some(want) if product_fingerprint(&product) == *want => {}
        _ => return Err(ApplyError::Stale),
    }
    if let Err(error) = store
        .set_product_active(conn, &product.id, false, now)
        .await
    {
        if error.is_not_found() {
            return Err(ApplyError::message(format!(
                "product {} no longer exists",
                json::quote(&action.product_id)
            )));
        }
        tracing::warn!(product_id = %action.product_id, error = %error, "codemode product could not be archived");
        return Err(ApplyError::message("the product could not be archived"));
    }
    product.active = false;
    baseline.insert(
        format!("product:{}", product.id),
        product_fingerprint(&product),
    );
    if insert_action_audit(
        store,
        conn,
        actor_user_id,
        "codemode.product_archive",
        "product",
        &product.id,
        plan_id,
        now,
    )
    .await
    .is_err()
    {
        return Err(ApplyError::message(
            "the product audit entry could not be saved",
        ));
    }
    Ok(format!("Archived {}.", json::quote(&product.name)))
}

async fn apply_product_create(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    now: DateTime<Utc>,
    registry: Option<&RwLock<Registry>>,
) -> Result<String, ApplyError> {
    let Some(source) = &action.product else {
        return Err(ApplyError::message("choose the product to create"));
    };
    let mut draft: ProductDraft = source.clone();
    validate_product_draft(&mut draft, registry).map_err(ApplyError::Message)?;

    let category = match store.category_by_slug_tx(conn, &draft.category_slug).await {
        Ok(category) => category,
        Err(StoreError::NotFound) => {
            return Err(ApplyError::message(format!(
                "category {} no longer exists",
                json::quote(&draft.category_slug)
            )));
        }
        Err(error) => {
            tracing::warn!(error = %error, "codemode category could not be read");
            return Err(ApplyError::message("the category could not be read"));
        }
    };
    if category.plugin_id != draft.plugin_id {
        return Err(ApplyError::message(format!(
            "category {} belongs to another product type",
            json::quote(&draft.category_slug)
        )));
    }

    let taken = match conn
        .query(
            "SELECT COUNT(*) FROM products WHERE slug = ?",
            params![draft.slug.clone()],
        )
        .await
    {
        Ok(mut rows) => match rows.next().await {
            Ok(Some(row)) => row.get::<i64>(0).unwrap_or_default(),
            Ok(None) => 0,
            Err(error) => {
                tracing::warn!(error = %error, "codemode slug could not be checked");
                return Err(ApplyError::message("the product slug could not be checked"));
            }
        },
        Err(error) => {
            tracing::warn!(error = %error, "codemode slug could not be checked");
            return Err(ApplyError::message("the product slug could not be checked"));
        }
    };
    if taken > 0 {
        return Err(ApplyError::message(format!(
            "slug {} is already in use",
            json::quote(&draft.slug)
        )));
    }

    let product = Product {
        id: azahar_ids::new("prd"),
        slug: draft.slug.clone(),
        name: draft.name.clone(),
        description: draft.description.clone(),
        price_cents: draft.price_cents,
        currency: "DZD".to_string(),
        category,
        image: draft.image.clone(),
        stock: i32::try_from(draft.stock)
            .map_err(|_| ApplyError::message("the product could not be created"))?,
        active: true,
        plugin_id: draft.plugin_id.clone(),
        attributes: draft.attributes.clone(),
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };
    if let Err(error) = store.create_product(conn, &product).await {
        tracing::warn!(slug = %draft.slug, error = %error, "codemode product could not be created");
        return Err(ApplyError::message("the product could not be created"));
    }
    if insert_action_audit(
        store,
        conn,
        actor_user_id,
        "codemode.product_create",
        "product",
        &product.id,
        plan_id,
        now,
    )
    .await
    .is_err()
    {
        return Err(ApplyError::message(
            "the product audit entry could not be saved",
        ));
    }
    Ok(format!("Created {}.", json::quote(&product.name)))
}

#[allow(clippy::too_many_arguments)]
async fn apply_order_status(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    plan_id: &str,
    action: &Action,
    baseline: &mut HashMap<String, String>,
    now: DateTime<Utc>,
    hooks: &dyn HookProvider,
) -> Result<String, ApplyError> {
    let order = match store.order_by_id_tx(conn, &action.order_id).await {
        Ok(order) => order,
        Err(StoreError::NotFound) => {
            return Err(ApplyError::message(format!(
                "order {} no longer exists",
                json::quote(&action.order_id)
            )));
        }
        Err(error) => {
            tracing::warn!(order_id = %action.order_id, error = %error, "codemode order could not be read");
            return Err(ApplyError::message("the order could not be read"));
        }
    };
    match baseline.get(&format!("order:{}", order.id)) {
        Some(want) if order.status == *want => {}
        _ => return Err(ApplyError::Stale),
    }
    if !azahar_orderstatus::can_transition(&order.status, &action.status) {
        return Err(ApplyError::message(format!(
            "order {} cannot move from {} to {}",
            order.number, order.status, action.status
        )));
    }

    if let Err(error) = store
        .update_order_status(conn, &order.id, &order.status, &action.status, now)
        .await
    {
        if error.is_not_found() {
            return Err(ApplyError::Stale);
        }
        tracing::warn!(order_id = %order.id, error = %error, "codemode order could not be updated");
        return Err(ApplyError::message("the order could not be updated"));
    }
    baseline.insert(format!("order:{}", order.id), action.status.clone());
    if action.status == "cancelled" {
        let items = match store.order_items_tx(conn, &order.id).await {
            Ok(items) => items,
            Err(error) => {
                tracing::warn!(order_id = %order.id, error = %error, "codemode order items could not be read");
                return Err(ApplyError::message("the order items could not be read"));
            }
        };
        for item in items {
            if let Err(error) = store
                .increment_stock(conn, &item.product_id, item.quantity)
                .await
            {
                tracing::warn!(order_id = %order.id, error = %error, "codemode order stock could not be restored");
                return Err(ApplyError::message("the order stock could not be restored"));
            }
            match store.product_by_id_tx(conn, &item.product_id, false).await {
                Ok(product) => {
                    baseline.insert(
                        format!("product:{}", item.product_id),
                        product_fingerprint(&product),
                    );
                }
                Err(StoreError::NotFound) => {}
                Err(error) => {
                    tracing::warn!(product_id = %item.product_id, error = %error, "codemode restocked product could not be read");
                    return Err(ApplyError::message(
                        "the restocked product could not be read",
                    ));
                }
            }
        }
    }
    if insert_action_audit(
        store,
        conn,
        actor_user_id,
        "codemode.order_status",
        "order",
        &order.id,
        plan_id,
        now,
    )
    .await
    .is_err()
    {
        return Err(ApplyError::message(
            "the order audit entry could not be saved",
        ));
    }
    for hook in hooks.status_hooks() {
        if let Err(error) = hook
            .on_order_status_changed(conn, &order.id, &order.status, &action.status)
            .await
        {
            tracing::warn!(order_id = %order.id, error = %error, "codemode order status hook failed");
            return Err(ApplyError::message(
                "the order status could not be completed",
            ));
        }
    }
    Ok(format!(
        "Moved order {} from {} to {}.",
        order.number, order.status, action.status
    ))
}

/// Writes the audit row for one applied action. The detail is exactly Go's
/// sorted-key map encoding: `{"plan":"...","via":"codemode"}`.
#[allow(clippy::too_many_arguments)]
pub async fn insert_action_audit(
    store: &Store,
    conn: &Connection,
    actor_user_id: &str,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    plan_id: &str,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    let mut detail = json::Object::new();
    detail.string_field("plan", plan_id);
    detail.string_field("via", "codemode");
    store
        .insert_audit(
            conn,
            &AuditEntry {
                id: azahar_ids::new("aud"),
                actor_user_id: actor_user_id.to_string(),
                action: action.to_string(),
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                detail: detail.finish(),
                created_at: Some(now),
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_detail_matches_go_map_order() {
        let mut detail = json::Object::new();
        detail.string_field("plan", "cmp_1");
        detail.string_field("via", "codemode");
        assert_eq!(detail.finish(), r#"{"plan":"cmp_1","via":"codemode"}"#);
    }

    #[test]
    fn stale_and_message_errors_keep_their_text() {
        assert_eq!(ApplyError::Stale.to_string(), "codemode plan is stale");
        assert_eq!(
            ApplyError::message("the product could not be read").to_string(),
            "the product could not be read"
        );
    }
}

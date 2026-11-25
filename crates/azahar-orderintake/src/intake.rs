//! The one path that turns basket lines into a stored order and the one path
//! that moves an order's status, ported from
//! `backend/internal/orderintake/orderintake.go`.
//!
//! Every write runs inside one store transaction, so plugin hooks receive the
//! transaction's connection and the audit row joins the same commit.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use azahar_ids::new as new_id;
use azahar_orderstatus::{can_transition, known};
use azahar_plugins::Registry;
use azahar_store::products::{CORE_PLUGIN_ID, normalize_plugin_id};
use azahar_store::store::{Store, StoreError, WriteTx};
use azahar_store::{AuditEntry, Order, OrderItem};
use chrono::{DateTime, Datelike, Utc};

use crate::channel::Channel;
use crate::hooks::{CheckoutLine, HookError, HookProvider, OrderRef};
use crate::lines::Line;
use crate::options::{option_label, option_schema, selected_option_fields};
use crate::problems::{Problem, refuse, refuse_fields};

const STATUS_DELIVERED: &str = "delivered";
const STATUS_CANCELLED: &str = "cancelled";

/// One order to store. The caller has already validated its own payload fields
/// and line count; [`crate::build_lines`] has trimmed and merged the lines.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub lines: Vec<Line>,
    pub channel: Channel,
    pub customer: Customer,
    pub destination: Destination,
    /// The status the row is created with.
    pub status: String,
    /// The hash of the order token the storefront shows.
    pub token_hash: String,
    /// The account or employee behind the order.
    pub actor_user_id: String,
}

/// The person on the order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Customer {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub notes: String,
}

/// Where a delivered order goes. A counter sale leaves it zero apart from
/// `delivery_type`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Destination {
    pub wilaya_code: i32,
    pub wilaya_name: String,
    pub commune_name: String,
    pub delivery_type: String,
    pub stop_desk_id: String,
    pub stop_desk_name: String,
}

/// A stored order and its lines, ready for a response body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaceResult {
    pub order: Order,
    pub items: Vec<OrderItem>,
}

/// One status move on an existing order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub order_id: String,
    pub to: String,
    pub actor_user_id: String,
    pub actor_role: String,
}

/// An order after a status move, with its lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    pub order: Order,
    pub items: Vec<OrderItem>,
}

/// Why an order write did not happen.
#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    /// A refusal the HTTP layer renders as its problem response.
    #[error(transparent)]
    Problem(#[from] Problem),
    /// An infrastructure failure the caller reports as an internal error.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A plugin hook failed.
    #[error("{0}")]
    Hook(String),
}

impl From<HookError> for IntakeError {
    fn from(value: HookError) -> Self {
        Self::Hook(value.to_string())
    }
}

impl IntakeError {
    #[must_use]
    pub fn problem(&self) -> Option<&Problem> {
        match self {
            Self::Problem(problem) => Some(problem),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Store(StoreError::NotFound))
    }
}

/// Runs the order writes.
pub struct Intake {
    store: Arc<Store>,
    plugins: Arc<RwLock<Registry>>,
    hooks: Arc<dyn HookProvider>,
}

impl Intake {
    /// Builds an intake that writes through `store`, asks `plugins` about the
    /// product types that own products, and resolves hooks through `hooks`.
    #[must_use]
    pub fn new(
        store: Arc<Store>,
        plugins: Arc<RwLock<Registry>>,
        hooks: Arc<dyn HookProvider>,
    ) -> Self {
        Self {
            store,
            plugins,
            hooks,
        }
    }

    /// The registry guard. Poisoning can only come from a panicking writer;
    /// reads still see a complete registry, so recover instead of panicking.
    fn registry(&self) -> RwLockReadGuard<'_, Registry> {
        self.plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Stores one order: checks the catalog and the selected options in the
    /// write transaction, prices the lines, groups the plugin lines, allocates
    /// the order number, inserts the order and its items, moves stock, runs the
    /// checkout hooks, optionally marks the sale delivered, and writes the
    /// audit row.
    ///
    /// # Errors
    ///
    /// Returns [`IntakeError::Problem`] for a refusal, [`IntakeError::Store`]
    /// for a database failure and [`IntakeError::Hook`] for a plugin failure.
    pub async fn place(&self, request: Request) -> Result<PlaceResult, IntakeError> {
        let now = Utc::now();
        let mut order = Order {
            id: new_id("ord"),
            status: request.status.clone(),
            name: request.customer.name.clone(),
            email: request.customer.email.clone(),
            phone: request.customer.phone.clone(),
            address: request.customer.address.clone(),
            city: request.destination.commune_name.clone(),
            notes: request.customer.notes.clone(),
            currency: "DZD".to_string(),
            created_at: Some(now),
            updated_at: Some(now),
            wilaya_code: request.destination.wilaya_code,
            wilaya_name: request.destination.wilaya_name.clone(),
            commune_name: request.destination.commune_name.clone(),
            delivery_type: request.destination.delivery_type.clone(),
            stop_desk_id: request.destination.stop_desk_id.clone(),
            stop_desk_name: request.destination.stop_desk_name.clone(),
            token_hash: request.token_hash.clone(),
            ..Order::default()
        };
        if !request.customer.user_id.is_empty() {
            order.user_id = Some(request.customer.user_id.clone());
        }

        let tx = self.store.begin_write().await?;
        match self.place_in(&tx, &request, &mut order, now).await {
            Ok(items) => {
                tx.commit().await?;
                Ok(PlaceResult { order, items })
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    async fn place_in(
        &self,
        tx: &WriteTx<'_>,
        request: &Request,
        order: &mut Order,
        now: DateTime<Utc>,
    ) -> Result<Vec<OrderItem>, IntakeError> {
        let conn = tx.conn();
        let mut items: Vec<OrderItem> = Vec::with_capacity(request.lines.len());
        let mut plugin_lines: HashMap<String, Vec<CheckoutLine>> = HashMap::new();
        let mut subtotal: i64 = 0;

        for line in &request.lines {
            let product = match self
                .store
                .product_by_id_tx(conn, &line.product_id, true)
                .await
            {
                Ok(product) => product,
                Err(StoreError::NotFound) => {
                    return Err(refuse_fields(
                        409,
                        "conflict",
                        "Some products are no longer available.",
                        HashMap::from([(
                            "items".to_string(),
                            format!(
                                "A product in the {} is no longer available.",
                                request.channel.basket
                            ),
                        )]),
                    )
                    .into());
                }
                Err(error) => return Err(error.into()),
            };
            if !self.registry().effective_enabled(&product.plugin_id) {
                return Err(refuse_fields(
                    409,
                    "conflict",
                    "Some products are no longer available.",
                    HashMap::from([(
                        "items".to_string(),
                        format!("{} is no longer available.", product.name),
                    )]),
                )
                .into());
            }

            let (schema, hook) = {
                let registry = self.registry();
                (
                    option_schema(&registry, &product),
                    self.hooks.checkout_hook(&product.plugin_id),
                )
            };
            let fields = selected_option_fields(&schema, &product.attributes, &line.options);
            if !fields.is_empty() {
                return Err(refuse_fields(
                    409,
                    "conflict",
                    "Some product options have changed.",
                    fields,
                )
                .into());
            }

            let item_id = new_id("itm");
            let plugin_line = CheckoutLine {
                product: product.clone(),
                options: line.options.clone(),
                quantity: line.quantity,
                order_id: order.id.clone(),
                item_id: item_id.clone(),
            };
            if let Some(hook) = &hook
                && let Some(problem) = hook.validate_checkout_line(conn, &plugin_line).await
            {
                return Err(Problem {
                    status: problem.status,
                    code: problem.code,
                    message: problem.message,
                    fields: problem.fields,
                }
                .into());
            }

            if i64::from(product.stock) < line.quantity {
                return Err(refuse_fields(
                    409,
                    "conflict",
                    "Some products do not have enough stock.",
                    HashMap::from([(
                        "items".to_string(),
                        format!("Reduce the quantity for {}.", product.name),
                    )]),
                )
                .into());
            }
            let unit_price = product.effective_price_cents(now);
            if let Some(expected) = line.expected_price_cents
                && expected != unit_price
            {
                return Err(refuse_fields(
                    409,
                    "price_changed",
                    "A price changed while the bag was open. Review the bag and try again.",
                    HashMap::from([(
                        "items".to_string(),
                        format!("{} now costs a different amount.", product.name),
                    )]),
                )
                .into());
            }
            let quantity = quantity_i32(line.quantity);
            let line_total = unit_price.wrapping_mul(line.quantity);
            subtotal = subtotal.wrapping_add(line_total);
            items.push(OrderItem {
                id: item_id,
                order_id: order.id.clone(),
                product_id: product.id.clone(),
                slug: product.slug.clone(),
                name: product.name.clone(),
                image: product.image.clone(),
                unit_price_cents: unit_price,
                quantity,
                line_total_cents: line_total,
                options: line.options.clone(),
                options_label: option_label(&schema, &line.options),
                plugin_id: normalize_plugin_id(&product.plugin_id),
            });
            if normalize_plugin_id(&product.plugin_id) != CORE_PLUGIN_ID {
                plugin_lines
                    .entry(product.plugin_id.clone())
                    .or_default()
                    .push(plugin_line);
            }
        }

        order.subtotal_cents = subtotal;
        order.shipping_cents = request.channel.shipping_cents(subtotal);
        order.total_cents = subtotal.wrapping_add(order.shipping_cents);

        let number = self.store.next_order_number(conn, now.year()).await?;
        order.number = number;
        self.store.create_order(conn, order, &items).await?;

        // Go walks the request lines again, so a caller that skipped BuildLines
        // and sent the same product twice decrements once per line, not once
        // per stored item.
        for line in &request.lines {
            let moved = self
                .store
                .decrement_stock(conn, &line.product_id, quantity_i32(line.quantity))
                .await?;
            if !moved {
                return Err(refuse_fields(
                    409,
                    "conflict",
                    "Some products do not have enough stock.",
                    HashMap::from([(
                        "items".to_string(),
                        format!("Reduce the quantity in the {}.", request.channel.basket),
                    )]),
                )
                .into());
            }
        }

        for (plugin_id, hook_lines) in &plugin_lines {
            let Some(hook) = self.hooks.checkout_hook(plugin_id) else {
                continue;
            };
            hook.on_order_created(
                conn,
                &OrderRef {
                    id: order.id.clone(),
                    number: order.number.clone(),
                    status: order.status.clone(),
                },
                hook_lines,
            )
            .await?;
        }

        if request.channel.delivered {
            // A counter sale is born delivered: creation-time state, not an
            // admin lifecycle move, so it skips `can_transition`. Creating the
            // row pending first keeps UpdateOrderStatus and the status hooks
            // in their usual order inside the same transaction.
            let from = order.status.clone();
            self.store
                .update_order_status(conn, &order.id, &from, STATUS_DELIVERED, now)
                .await?;
            for hook in self.hooks.status_hooks() {
                hook.on_order_status_changed(conn, &order.id, &from, STATUS_DELIVERED)
                    .await?;
            }
            order.status = STATUS_DELIVERED.to_string();
        }

        let detail = request.channel.audit_detail(order, items.len(), now);
        self.store
            .insert_audit(
                conn,
                &AuditEntry {
                    id: new_id("aud"),
                    actor_user_id: request.actor_user_id.clone(),
                    action: request.channel.audit.action.clone(),
                    entity_type: "order".to_string(),
                    entity_id: order.id.clone(),
                    detail: encode_audit_detail(&detail),
                    created_at: Some(now),
                },
            )
            .await?;
        Ok(items)
    }

    /// Moves an order through the lifecycle. Refuses an unknown status, a
    /// missing order, a move the lifecycle does not allow and a cancellation
    /// by anyone who is not an administrator. An allowed move updates the row,
    /// restocks a cancellation, writes the audit row and runs the status hooks
    /// in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns [`IntakeError::Problem`] for a refusal, [`IntakeError::Store`]
    /// for a database failure and [`IntakeError::Hook`] for a plugin failure.
    pub async fn change_status(&self, change: Change) -> Result<Changed, IntakeError> {
        if !known(&change.to) {
            return Err(refuse_fields(
                422,
                "validation_error",
                "Check the marked fields.",
                HashMap::from([("status".to_string(), "Unknown order status.".to_string())]),
            )
            .into());
        }
        let mut order = match self.store.order_by_id(&change.order_id).await {
            Ok(order) => order,
            Err(StoreError::NotFound) => {
                return Err(refuse(404, "not_found", "Order not found.").into());
            }
            Err(error) => return Err(error.into()),
        };
        if !can_transition(&order.status, &change.to) {
            return Err(refuse_fields(
                409,
                "invalid_transition",
                "That status change is not allowed.",
                HashMap::from([(
                    "status".to_string(),
                    format!(
                        "This order cannot move from {} to {}.",
                        order.status, change.to
                    ),
                )]),
            )
            .into());
        }
        if change.to == STATUS_CANCELLED && change.actor_role != "admin" {
            return Err(refuse(
                403,
                "forbidden",
                "Only an administrator can cancel an order.",
            )
            .into());
        }

        let now = Utc::now();
        let restock = if change.to == STATUS_CANCELLED {
            self.store.order_items(&order.id).await?
        } else {
            Vec::new()
        };
        let detail =
            encode_audit_detail(&serde_json::json!({"from": &order.status, "to": &change.to}));

        let tx = self.store.begin_write().await?;
        let applied = self
            .apply_status_change(&tx, &order, &change, &restock, now, &detail)
            .await;
        match applied {
            Ok(()) => tx.commit().await?,
            Err(error) => {
                let _ = tx.rollback().await;
                if matches!(error, IntakeError::Store(StoreError::NotFound)) {
                    return Err(refuse_fields(
                        409,
                        "conflict",
                        "The order changed. Reload and try again.",
                        HashMap::from([(
                            "status".to_string(),
                            "The status changed while this page was open.".to_string(),
                        )]),
                    )
                    .into());
                }
                return Err(error);
            }
        }

        order.status = change.to.clone();
        order.updated_at = Some(now);
        let items = self.store.order_items(&order.id).await?;
        Ok(Changed { order, items })
    }

    async fn apply_status_change(
        &self,
        tx: &WriteTx<'_>,
        order: &Order,
        change: &Change,
        restock: &[OrderItem],
        now: DateTime<Utc>,
        detail: &str,
    ) -> Result<(), IntakeError> {
        let conn = tx.conn();
        self.store
            .update_order_status(conn, &order.id, &order.status, &change.to, now)
            .await?;
        for item in restock {
            self.store
                .increment_stock(conn, &item.product_id, item.quantity)
                .await?;
        }
        self.store
            .insert_audit(
                conn,
                &AuditEntry {
                    id: new_id("aud"),
                    actor_user_id: change.actor_user_id.clone(),
                    action: "order.status".to_string(),
                    entity_type: "order".to_string(),
                    entity_id: order.id.clone(),
                    detail: detail.to_string(),
                    created_at: Some(now),
                },
            )
            .await?;
        for hook in self.hooks.status_hooks() {
            hook.on_order_status_changed(conn, &order.id, &order.status, &change.to)
                .await?;
        }
        Ok(())
    }
}

/// The stored `quantity` is an `i32`; the stock check bounds every accepted
/// line to a product's `i32` stock, so this only clamps a value that can no
/// longer reach an item.
fn quantity_i32(quantity: i64) -> i32 {
    i32::try_from(quantity).unwrap_or(i32::MAX)
}

/// Go's `json.Marshal` escapes HTML-significant characters in every string,
/// including audit details. `serde_json` does not, so apply the same escapes
/// the store's own JSON helpers use.
fn encode_audit_detail(detail: &serde_json::Value) -> String {
    let encoded = detail.to_string();
    if !encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']) {
        return encoded;
    }
    encoded
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_details_use_go_html_escaping() {
        let detail = serde_json::json!({"version": "<2026> & co"});
        assert_eq!(
            encode_audit_detail(&detail),
            r#"{"version":"\u003c2026\u003e \u0026 co"}"#
        );
        assert_eq!(encode_audit_detail(&serde_json::json!({})), "{}");
    }
}

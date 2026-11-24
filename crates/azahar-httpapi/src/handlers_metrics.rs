//! The admin dashboard aggregate: `GET /api/admin/metrics`.
//!
//! Ported from `handlers_metrics.go`. The date range defaults to the last 30
//! days and only 7, 30 or 90 are accepted; both endpoints are UTC midnights and
//! the previous window is the equally long block immediately before it.

use axum::extract::{RawQuery, State};
use axum::response::Response;
use azahar_orderstatus::STATUSES;
use azahar_store::format_time_or_zero;
use azahar_store::metrics::Metrics;
use chrono::{Duration, Utc};
use serde::Serialize;

use crate::handlers_catalog::query_param;
use crate::middleware::internal_error;
use crate::respond::{ApiProblem, json_response, problem_fields};
use crate::state::AppState;
use crate::validate::fields;

#[derive(Debug, Clone, Serialize)]
struct MetricsRangeDto {
    days: i64,
    from: String,
    to: String,
}

#[derive(Debug, Clone, Serialize)]
struct LowStockDto {
    id: String,
    name: String,
    stock: i32,
    image: String,
}

#[derive(Debug, Clone, Serialize)]
struct RevenuePointDto {
    date: String,
    revenue_cents: i64,
    orders: i64,
}

#[derive(Debug, Clone, Serialize)]
struct StatusCountDto {
    status: String,
    count: i64,
}

#[derive(Debug, Clone, Serialize)]
struct TopProductDto {
    product_id: String,
    name: String,
    units: i64,
    revenue_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
struct RecentOrderDto {
    id: String,
    number: String,
    name: String,
    total_cents: i64,
    status: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct MetricsResponse {
    demo: bool,
    range: MetricsRangeDto,
    revenue_cents: i64,
    previous_revenue_cents: i64,
    delta_pct: Option<f64>,
    orders_count: i64,
    previous_orders_count: i64,
    aov_cents: i64,
    units_sold: i64,
    new_customers: i64,
    low_stock: Vec<LowStockDto>,
    revenue_series: Vec<RevenuePointDto>,
    status_split: Vec<StatusCountDto>,
    top_products: Vec<TopProductDto>,
    recent_orders: Vec<RecentOrderDto>,
}

fn parse_days(raw: Option<&str>) -> Result<i64, ApiProblem> {
    let Some(value) = raw.filter(|value| !value.is_empty()) else {
        return Ok(30);
    };
    match value.parse::<i64>() {
        Ok(days) if days == 7 || days == 30 || days == 90 => Ok(days),
        _ => Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("days", "Choose 7, 30 or 90.")]),
        )),
    }
}

/// `GET /api/admin/metrics`.
pub async fn metrics(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let days_param = raw.as_deref().map(|query| query_param(Some(query), "days"));
    let days = parse_days(days_param.as_deref())?;

    let now = Utc::now();
    let to = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc();
    let from = to - Duration::days(days - 1);
    let previous_to = from - Duration::days(1);
    let previous_from = previous_to - Duration::days(days - 1);

    let metrics = state
        .store
        .metrics(from, to, previous_from, previous_to)
        .await
        .map_err(|_| internal_error())?;

    Ok(json_response(
        200,
        &build_response(days, from, to, &metrics),
    ))
}

fn build_response(
    days: i64,
    from: chrono::DateTime<Utc>,
    to: chrono::DateTime<Utc>,
    metrics: &Metrics,
) -> MetricsResponse {
    let delta_pct = if metrics.previous_revenue_cents > 0 {
        let change = (metrics.revenue_cents - metrics.previous_revenue_cents) as f64
            / metrics.previous_revenue_cents as f64
            * 100.0;
        Some((change * 10.0).round() / 10.0)
    } else {
        None
    };
    let aov_cents = if metrics.orders_count > 0 {
        metrics.revenue_cents / metrics.orders_count
    } else {
        0
    };

    let low_stock = metrics
        .low_stock
        .iter()
        .map(|item| LowStockDto {
            id: item.id.clone(),
            name: item.name.clone(),
            stock: item.stock,
            image: item.image.clone(),
        })
        .collect();
    let revenue_series = metrics
        .revenue_series
        .iter()
        .map(|point| RevenuePointDto {
            date: point.date.clone(),
            revenue_cents: point.revenue_cents,
            orders: point.orders,
        })
        .collect();
    let status_split = STATUSES
        .iter()
        .map(|status| StatusCountDto {
            status: (*status).to_string(),
            count: metrics.status_split.get(*status).copied().unwrap_or(0),
        })
        .collect();
    let top_products = metrics
        .top_products
        .iter()
        .map(|product| TopProductDto {
            product_id: product.product_id.clone(),
            name: product.name.clone(),
            units: product.units,
            revenue_cents: product.revenue_cents,
        })
        .collect();
    let recent_orders = metrics
        .recent_orders
        .iter()
        .map(|order| RecentOrderDto {
            id: order.id.clone(),
            number: order.number.clone(),
            name: order.name.clone(),
            total_cents: order.total_cents,
            status: order.status.clone(),
            created_at: format_time_or_zero(order.created_at.as_ref()),
        })
        .collect();

    MetricsResponse {
        demo: metrics.demo,
        range: MetricsRangeDto {
            days,
            from: from.format("%Y-%m-%d").to_string(),
            to: to.format("%Y-%m-%d").to_string(),
        },
        revenue_cents: metrics.revenue_cents,
        previous_revenue_cents: metrics.previous_revenue_cents,
        delta_pct,
        orders_count: metrics.orders_count,
        previous_orders_count: metrics.previous_orders_count,
        aov_cents,
        units_sold: metrics.units_sold,
        new_customers: metrics.new_customers,
        low_stock,
        revenue_series,
        status_split,
        top_products,
        recent_orders,
    }
}

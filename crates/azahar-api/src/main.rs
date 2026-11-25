//! Azahar API server, the Rust port of `backend/cmd/api`.

mod logging;

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::Arc;

use azahar_config::Config;
use azahar_courier::{Courier, Options as CourierOptions};
use azahar_db::{PluginMigration, migrate, migrate_plugins, open};
use azahar_httpapi::{AppState, Server};
use azahar_plugins::Registry;
use azahar_seed::{Options as SeedOptions, run as seed_run};
use azahar_store::Store;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let log_writer = logging::LogWriter::new();
    tracing_subscriber::fmt()
        .json()
        .with_writer(log_writer.clone())
        .with_env_filter(
            // The session layer opens an info span for every request it sees.
            // It carries no fields of its own, so keep its warnings and errors
            // and drop the per-request span noise.
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_sessions=warn")),
        )
        .init();

    let status = match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "azahar-api stopped");
            ExitCode::FAILURE
        }
    };
    log_writer.flush();
    status
}

async fn run() -> anyhow::Result<()> {
    let cfg = Config::load().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let shutdown = CancellationToken::new();

    info!(
        addr = %cfg.api_addr,
        env = %cfg.app_env,
        "api starting"
    );

    let database = open(&cfg.dsn()).await?;
    let connection = database.connect()?;
    migrate(&connection).await?;
    let store = Arc::new(Store::new(database)?);

    let mut registry = Registry::new();
    registry.register(Arc::new(azahar_plugin_oils::new()))?;
    registry.register(Arc::new(azahar_plugin_pcparts::new()))?;
    registry.register(Arc::new(azahar_plugin_pcbuilder::new()))?;
    registry.register(Arc::new(azahar_plugin_codemode::new()))?;
    registry.register(Arc::new(azahar_plugin_phone::new()))?;
    registry.register(Arc::new(azahar_plugin_ai::new()))?;
    registry.validate_dependencies()?;

    let migrations: Vec<PluginMigration> = registry
        .migrations()
        .into_iter()
        .map(|item| PluginMigration {
            plugin_id: item.plugin_id,
            version: item.migration.version,
            sql: item.migration.sql,
        })
        .collect();
    migrate_plugins(store.conn(), migrations).await?;

    let states = store.plugin_states().await?;
    registry.set_states(&states);
    for plugin in registry.all() {
        let manifest = plugin.manifest();
        for category in &manifest.categories {
            store
                .upsert_category(
                    &manifest.id,
                    &azahar_store::Category {
                        slug: category.slug.clone(),
                        name: category.name.clone(),
                        description: category.description.clone(),
                        sort_order: category.sort_order,
                        ..Default::default()
                    },
                )
                .await?;
        }
    }
    store.reconcile_product_categories().await?;

    if cfg.seed_demo {
        let options = SeedOptions {
            admin_email: cfg.admin_email.clone(),
            admin_password: cfg.admin_password.clone(),
            customer_email: cfg.customer_email.clone(),
            customer_password: cfg.customer_password.clone(),
            employee_email: cfg.employee_email.clone(),
            employee_password: cfg.employee_password.clone(),
        };
        let summary = seed_run(&store, &options).await?;
        info!(
            categories = summary.categories,
            products = summary.products,
            users = summary.users,
            orders = summary.orders,
            "demo seed applied"
        );
    }

    azahar_plugin_phone::PhoneService::new()
        .init(store.conn(), chrono::Utc::now())
        .await;

    if cfg.is_development() {
        warn!(
            admin_email = %cfg.admin_email,
            admin_password = %cfg.admin_password,
            employee_email = %cfg.employee_email,
            employee_password = %cfg.employee_password,
            customer_email = %cfg.customer_email,
            customer_password = %cfg.customer_password,
            "development demo credentials"
        );
    }

    let (credentials, parsed) = cfg.courier_credential_values();
    if !parsed {
        warn!("COURIER_CREDENTIALS must be a JSON object; ignoring the value");
    }
    let credentials: BTreeMap<String, String> =
        credentials.unwrap_or_default().into_iter().collect();
    if cfg.courier_key != "sandbox" && credentials.is_empty() {
        warn!(
            courier = %cfg.courier_key,
            "courier credentials are empty; shipment creation will fail until they are set"
        );
    }
    if cfg.is_production() && cfg.courier_key == "sandbox" {
        warn!("the sandbox courier is active in production; no real parcels will be created");
    }

    let fallback_logger = tracing::dispatcher::get_default(|dispatch| dispatch.clone());
    let shipping: Arc<dyn Courier> = Arc::new(azahar_courier::Client::new(CourierOptions {
        base_url: cfg.courier_api_url.clone(),
        courier_key: cfg.courier_key.clone(),
        credentials,
        from_wilaya: i32::try_from(cfg.courier_from_wilaya).unwrap_or(16),
        timeout: cfg.courier_timeout,
        allow_local_sandbox: cfg.is_development(),
        on_fallback: Some(Arc::new(move |resource| {
            let _guard = tracing::dispatcher::set_default(&fallback_logger);
            warn!(
                resource,
                "courier API unavailable; using the bundled fallback"
            );
        })),
        ..CourierOptions::default()
    }));

    let session_cleanup = azahar_httpapi::LibsqlSessionStore::new(store.conn().clone());
    let cleanup_token = shutdown.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cleanup_token.cancelled() => return,
                _ = ticker.tick() => {
                    if let Err(error) = session_cleanup.delete_expired(chrono::Utc::now()).await {
                        tracing::warn!(error = %error, "expired session cleanup failed");
                    }
                }
            }
        }
    });

    let state = AppState::new(cfg.clone(), store.clone(), registry, shipping);
    let plugin_routes = axum::Router::new()
        .merge(azahar_plugin_codemode::codemode_routes())
        .merge(azahar_plugin_ai::ai_routes());
    let server = Server::bind(&cfg.api_addr, state.clone(), plugin_routes).await?;
    info!(
        addr = %server.local_addr()?,
        env = %cfg.app_env,
        "api listening"
    );

    // Kiri/libSQL closes an idle Hrana stream after ~40s. The Rust store keeps
    // one connection, so a periodic probe keeps the stream warm and resets it
    // if the database restarted underneath the process.
    let keepalive_conn = store.conn().clone();
    let keepalive_token = shutdown.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = keepalive_token.cancelled() => return,
                _ = ticker.tick() => {
                    let healthy = match keepalive_conn.query("SELECT 1", ()).await {
                        Ok(mut rows) => rows.next().await.is_ok(),
                        Err(_) => false,
                    };
                    if !healthy {
                        tracing::warn!("database stream reset after a failed keepalive");
                        keepalive_conn.reset().await;
                    }
                }
            }
        }
    });

    let audit_shutdown = CancellationToken::new();
    let audit_task = tokio::spawn(azahar_auditbatch::run(
        store.clone(),
        cfg.audit_batch_interval,
        audit_shutdown.clone(),
    ));

    let signal_token = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal_token.cancel();
    });

    let shutdown_token = shutdown.clone();
    server
        .run_with_shutdown(async move { shutdown_token.cancelled().await })
        .await?;

    info!("shutting down");
    audit_shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), audit_task).await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                error!(error = %error, "terminate signal listener failed");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

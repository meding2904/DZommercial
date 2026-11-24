//! Background audit outbox flusher, ported from
//! `backend/internal/auditbatch`.

use std::sync::Arc;
use std::time::Duration;

use azahar_store::Store;
use tokio_util::sync::CancellationToken;

pub const FLUSH_LIMIT: i64 = 200;
pub const FINAL_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Flushes queued audit rows immediately and then once per interval until the
/// token is cancelled. A final flush runs with its own timeout before
/// returning.
pub async fn run(store: Arc<Store>, interval: Duration, shutdown: CancellationToken) {
    let interval = if interval.is_zero() {
        Duration::from_secs(2)
    } else {
        interval
    };

    loop {
        if shutdown.is_cancelled() {
            break;
        }
        match store.flush_audit_batch(FLUSH_LIMIT).await {
            Ok(flushed) => {
                if flushed > 0 {
                    tracing::info!(rows = flushed, "audit batch flushed");
                }
                if flushed < FLUSH_LIMIT {
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "audit batch flush failed; queued rows kept for the next tick"
                );
                break;
            }
        }
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                let _ = tokio::time::timeout(FINAL_FLUSH_TIMEOUT, store.flush_audit_batch(FLUSH_LIMIT)).await;
                return;
            }
            _ = ticker.tick() => {
                if let Err(error) = store.flush_audit_batch(FLUSH_LIMIT).await {
                    tracing::warn!(
                        error = %error,
                        "audit batch flush failed; queued rows kept for the next tick"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use azahar_db::migrate;
    use azahar_store::AuditEntry;

    async fn store() -> Arc<Store> {
        let database = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .expect("open");
        let store = Arc::new(Store::new(database).expect("store"));
        migrate(store.conn()).await.expect("migrate");
        store
    }

    fn entry(id: &str) -> AuditEntry {
        AuditEntry {
            id: id.to_string(),
            actor_user_id: "usr_test".to_string(),
            action: "order.status".to_string(),
            entity_type: "order".to_string(),
            entity_id: "ord_test".to_string(),
            detail: "{}".to_string(),
            created_at: Some(chrono::Utc::now()),
        }
    }

    #[tokio::test]
    async fn an_already_cancelled_run_still_final_flushes() {
        let store = store().await;
        store
            .insert_audit(store.conn(), &entry("aud_cancelled"))
            .await
            .expect("insert");
        let token = CancellationToken::new();
        token.cancel();
        run(store.clone(), Duration::from_secs(3600), token).await;

        let mut rows = store
            .conn()
            .query("SELECT COUNT(*) FROM audit_log", ())
            .await
            .expect("query");
        let row = rows.next().await.expect("row").expect("some");
        let count: i64 = row.get(0).expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn flushes_queued_rows_and_stops_on_cancel() {
        let store = store().await;
        for index in 0..3 {
            store
                .insert_audit(store.conn(), &entry(&format!("aud_{index}")))
                .await
                .expect("insert");
        }
        let token = CancellationToken::new();
        let handle = tokio::spawn(run(store.clone(), Duration::from_millis(20), token.clone()));
        tokio::time::sleep(Duration::from_millis(100)).await;
        token.cancel();
        handle.await.expect("join");

        let mut rows = store
            .conn()
            .query("SELECT COUNT(*) FROM audit_log", ())
            .await
            .expect("query");
        let row = rows.next().await.expect("row").expect("some");
        let count: i64 = row.get(0).expect("count");
        assert_eq!(count, 3);

        let mut rows = store
            .conn()
            .query("SELECT COUNT(*) FROM audit_outbox", ())
            .await
            .expect("query");
        let row = rows.next().await.expect("row").expect("some");
        let remaining: i64 = row.get(0).expect("count");
        assert_eq!(remaining, 0);
    }
}

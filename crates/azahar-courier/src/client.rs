//! The resolver shared by checkout and shipment flows: fresh cache, the local
//! sandbox while the courier is offline, one adapter call, the stale cache,
//! then the bundled snapshot.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};

use crate::cache::{Cache, lock};
use crate::dzship::DzShip;
use crate::options::{Clock, FALLBACK_CACHE_TTL, OFFLINE_WINDOW, Options, Settings};
use crate::snapshot::SnapshotSource;
use crate::types::{APIError, Commune, Desk, Error, OrderRequest, Shipment, Tracking, Wilaya};

/// The seam the checkout and shipment flows use: the delivery map they offer
/// and the parcel they hand over. `Client` is the production implementation.
#[async_trait]
pub trait Courier: Send + Sync {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, Error>;
    async fn wilaya(&self, code: i32) -> Result<Wilaya, Error>;
    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, Error>;
    async fn commune(&self, wilaya_code: i32, name: &str) -> Result<String, Error>;
    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, Error>;
    async fn desk(&self, wilaya_code: i32, id: &str) -> Result<Desk, Error>;
    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, Error>;
    async fn track(&self, courier_key: &str, tracking_number: &str) -> Result<Tracking, Error>;
}

/// One adapter behind `Client`. The methods mirror the delivery map and parcel
/// verbs without carrying any cache or policy.
#[async_trait]
pub(crate) trait Source: Send + Sync {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, Error>;
    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, Error>;
    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, Error>;
    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, Error>;
    async fn track(&self, courier_key: &str, tracking_number: &str) -> Result<Tracking, Error>;
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

pub struct Client {
    remote: Arc<dyn Source>,
    snapshot: Arc<dyn Source>,
    local_sandbox: bool,
    on_fallback: Option<crate::options::FallbackCallback>,
    ttl: Duration,
    now: Clock,
    wilayas_cache: Cache<Vec<Wilaya>>,
    commune_caches: Mutex<HashMap<i32, Arc<Cache<Vec<Commune>>>>>,
    desk_caches: Mutex<HashMap<i32, Arc<Cache<Vec<Desk>>>>>,
    offline_until: Mutex<Option<DateTime<Utc>>>,
}

impl Client {
    /// `Client::new` is sync and infallible. `timeout` and `cache_ttl` of zero
    /// select the defaults (12 seconds, 24 hours).
    pub fn new(options: Options) -> Self {
        Self::build(options, Arc::new(Utc::now))
    }

    /// Test constructor: the clock drives cache expiry, the offline window and
    /// sandbox tracking timestamps.
    pub fn with_now<F>(options: Options, now: F) -> Self
    where
        F: Fn() -> DateTime<Utc> + Send + Sync + 'static,
    {
        Self::build(options, Arc::new(now))
    }

    fn build(options: Options, now: Clock) -> Self {
        let settings = Settings::from_options(&options);
        let remote = DzShip::new(&options, settings.timeout);
        let snapshot = SnapshotSource::new(now.clone());
        Self {
            remote: Arc::new(remote),
            snapshot: Arc::new(snapshot),
            local_sandbox: settings.local_sandbox,
            on_fallback: options.on_fallback.clone(),
            ttl: settings.ttl,
            now,
            wilayas_cache: Cache::default(),
            commune_caches: Mutex::new(HashMap::new()),
            desk_caches: Mutex::new(HashMap::new()),
            offline_until: Mutex::new(None),
        }
    }

    fn now(&self) -> DateTime<Utc> {
        (self.now)()
    }

    fn notify_fallback(&self, resource: &str) {
        if let Some(callback) = &self.on_fallback {
            callback(resource);
        }
    }

    fn is_offline(&self) -> bool {
        if !self.local_sandbox {
            return false;
        }
        let now = self.now();
        match *lock(&self.offline_until) {
            Some(until) => now < until,
            None => false,
        }
    }

    fn mark_offline(&self) {
        if !self.local_sandbox {
            return;
        }
        let until = advance(self.now(), OFFLINE_WINDOW);
        *lock(&self.offline_until) = Some(until);
    }

    /// A real courier key never receives the local sandbox's desks, parcels or
    /// tracking.
    fn sandbox_unavailable(&self, error: &Error) -> bool {
        self.local_sandbox && error.is_unavailable()
    }

    fn commune_cache(&self, wilaya_code: i32) -> Arc<Cache<Vec<Commune>>> {
        let mut caches = lock(&self.commune_caches);
        caches
            .entry(wilaya_code)
            .or_insert_with(|| Arc::new(Cache::default()))
            .clone()
    }

    fn desk_cache(&self, wilaya_code: i32) -> Arc<Cache<Vec<Desk>>> {
        let mut caches = lock(&self.desk_caches);
        caches
            .entry(wilaya_code)
            .or_insert_with(|| Arc::new(Cache::default()))
            .clone()
    }
}

#[async_trait]
impl Courier for Client {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, Error> {
        let fetch = self.remote.wilayas();
        resolve_map(
            self,
            &self.wilayas_cache,
            "wilayas",
            fetch,
            || self.snapshot.wilayas(),
            |_| true,
        )
        .await
    }

    async fn wilaya(&self, code: i32) -> Result<Wilaya, Error> {
        let wilayas = self.wilayas().await?;
        wilayas
            .into_iter()
            .find(|wilaya| wilaya.code == code)
            .ok_or(Error::UnknownWilaya)
    }

    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, Error> {
        let entry = self.commune_cache(wilaya_code);
        let fetch = self.remote.communes(wilaya_code);
        resolve_map(
            self,
            &entry,
            "communes",
            fetch,
            || self.snapshot.communes(wilaya_code),
            |_| true,
        )
        .await
    }

    async fn commune(&self, wilaya_code: i32, name: &str) -> Result<String, Error> {
        let communes = self.communes(wilaya_code).await?;
        let needle = name.trim();
        communes
            .into_iter()
            .find(|commune| equal_fold(&commune.name_fr, needle))
            .map(|commune| commune.name_fr)
            .ok_or(Error::UnknownCommune)
    }

    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, Error> {
        let entry = self.desk_cache(wilaya_code);
        let fetch = self.remote.desks(wilaya_code);
        resolve_map(
            self,
            &entry,
            "desks",
            fetch,
            || self.snapshot.desks(wilaya_code),
            |error| self.sandbox_unavailable(error),
        )
        .await
    }

    async fn desk(&self, wilaya_code: i32, id: &str) -> Result<Desk, Error> {
        let desks = self.desks(wilaya_code).await?;
        desks
            .into_iter()
            .find(|desk| desk.id == id)
            .ok_or(Error::UnknownDesk)
    }

    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, Error> {
        let fetch = self.remote.create_order(request.clone());
        let shipment = resolve_call(self, "orders", fetch, || {
            self.snapshot.create_order(request.clone())
        })
        .await?;
        if shipment.tracking_number.trim().is_empty() {
            return Err(Error::Api(APIError {
                status: 502,
                code: "courier_error".to_owned(),
                message: "The courier did not return a tracking number.".to_owned(),
                ..APIError::default()
            }));
        }
        Ok(shipment)
    }

    async fn track(&self, courier_key: &str, tracking_number: &str) -> Result<Tracking, Error> {
        let fetch = self.remote.track(courier_key, tracking_number);
        resolve_call(self, "track", fetch, || {
            self.snapshot.track(courier_key, tracking_number)
        })
        .await
    }
}

async fn resolve_map<'a, T, L, O>(
    client: &'a Client,
    entry: &Cache<T>,
    resource: &str,
    fetch: BoxFuture<'a, T>,
    local: L,
    local_on: O,
) -> Result<T, Error>
where
    T: Clone + Send + 'a,
    L: Fn() -> BoxFuture<'a, T> + Send,
    O: Fn(&Error) -> bool + Send,
{
    if let Some(value) = entry.get(client.now()) {
        return Ok(value);
    }
    if client.is_offline()
        && let Ok(value) = local().await
    {
        client.notify_fallback(resource);
        entry.set(value.clone(), advance(client.now(), FALLBACK_CACHE_TTL));
        return Ok(value);
    }
    match fetch.await {
        Ok(value) => {
            entry.set(value.clone(), advance(client.now(), client.ttl));
            Ok(value)
        }
        Err(error) => {
            if let Some(stale) = entry.stale() {
                return Ok(stale);
            }
            if local_on(&error)
                && let Ok(fallback) = local().await
            {
                client.mark_offline();
                client.notify_fallback(resource);
                entry.set(fallback.clone(), advance(client.now(), FALLBACK_CACHE_TTL));
                return Ok(fallback);
            }
            Err(error)
        }
    }
}

async fn resolve_call<'a, T, L>(
    client: &'a Client,
    resource: &str,
    fetch: BoxFuture<'a, T>,
    local: L,
) -> Result<T, Error>
where
    T: Send + 'a,
    L: Fn() -> BoxFuture<'a, T> + Send,
{
    if client.is_offline()
        && let Ok(value) = local().await
    {
        client.notify_fallback(resource);
        return Ok(value);
    }
    match fetch.await {
        Ok(value) => Ok(value),
        Err(error) => {
            if client.sandbox_unavailable(&error)
                && let Ok(fallback) = local().await
            {
                client.mark_offline();
                client.notify_fallback(resource);
                return Ok(fallback);
            }
            Err(error)
        }
    }
}

fn advance(now: DateTime<Utc>, duration: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX);
    now.checked_add_signed(delta)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// Go's `strings.EqualFold` compares simple case folds, so `ſ` matches `s` and
/// `ς` matches `Σ`/`σ`. Folding character by character keeps it allocation-free
/// and, unlike full case folding, does not equate `ß` with `ss`.
fn equal_fold(left: &str, right: &str) -> bool {
    left.chars()
        .map(casefold::simple_fold_char)
        .eq(right.chars().map(casefold::simple_fold_char))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::cache::lock as lock_mutex;
    use crate::options::{DEFAULT_CACHE_TTL, Options};
    use crate::types::Recipient;

    struct FakeClock {
        now: Mutex<DateTime<Utc>>,
    }

    impl FakeClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(
                    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                        .expect("fixed clock")
                        .with_timezone(&Utc),
                ),
            })
        }

        fn now(&self) -> DateTime<Utc> {
            *lock_mutex(&self.now)
        }

        fn advance(&self, duration: Duration) {
            let mut now = lock_mutex(&self.now);
            *now = advance(*now, duration);
        }
    }

    #[derive(Default)]
    struct ScriptedSource {
        wilayas_value: Mutex<Vec<Wilaya>>,
        communes_value: Mutex<Vec<Commune>>,
        desks_value: Mutex<Vec<Desk>>,
        shipment_value: Mutex<Shipment>,
        tracking_value: Mutex<Tracking>,
        error: Mutex<Option<Error>>,
        calls: AtomicUsize,
    }

    impl ScriptedSource {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn set_error(&self, error: Option<Error>) {
            *lock_mutex(&self.error) = error;
        }

        fn next_error(&self) -> Option<Error> {
            lock_mutex(&self.error).clone()
        }

        fn record_call(&self) -> Option<Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.next_error()
        }
    }

    #[async_trait]
    impl Source for ScriptedSource {
        async fn wilayas(&self) -> Result<Vec<Wilaya>, Error> {
            if let Some(error) = self.record_call() {
                return Err(error);
            }
            Ok(lock_mutex(&self.wilayas_value).clone())
        }

        async fn communes(&self, _wilaya_code: i32) -> Result<Vec<Commune>, Error> {
            if let Some(error) = self.record_call() {
                return Err(error);
            }
            Ok(lock_mutex(&self.communes_value).clone())
        }

        async fn desks(&self, _wilaya_code: i32) -> Result<Vec<Desk>, Error> {
            if let Some(error) = self.record_call() {
                return Err(error);
            }
            Ok(lock_mutex(&self.desks_value).clone())
        }

        async fn create_order(&self, _request: OrderRequest) -> Result<Shipment, Error> {
            if let Some(error) = self.record_call() {
                return Err(error);
            }
            Ok(lock_mutex(&self.shipment_value).clone())
        }

        async fn track(
            &self,
            _courier_key: &str,
            _tracking_number: &str,
        ) -> Result<Tracking, Error> {
            if let Some(error) = self.record_call() {
                return Err(error);
            }
            Ok(lock_mutex(&self.tracking_value).clone())
        }
    }

    type Notified = Arc<Mutex<Vec<String>>>;

    fn scripted_harness(
        key: &str,
        sandbox: bool,
    ) -> (Client, Arc<ScriptedSource>, Arc<FakeClock>, Notified) {
        let clock = FakeClock::new();
        let notified: Notified = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&notified);
        let options = Options {
            courier_key: key.to_owned(),
            allow_local_sandbox: sandbox,
            cache_ttl: Duration::from_secs(60 * 60),
            on_fallback: Some(Arc::new(move |resource| {
                lock_mutex(&recorder).push(resource.to_owned());
            })),
            ..Options::default()
        };
        let clock_for_client = Arc::clone(&clock);
        let mut client = Client::with_now(options, move || clock_for_client.now());
        let remote = Arc::new(ScriptedSource::default());
        client.remote = Arc::clone(&remote) as Arc<dyn Source>;
        (client, remote, clock, notified)
    }

    fn wilaya(code: i32) -> Wilaya {
        Wilaya {
            code,
            name_fr: format!("Wilaya {code}"),
            ..Wilaya::default()
        }
    }

    #[tokio::test]
    async fn fresh_wilayas_are_served_from_the_cache() {
        let (client, remote, clock, notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.wilayas_value) = vec![wilaya(16)];

        assert_eq!(client.wilayas().await.expect("prime").len(), 1);
        clock.advance(Duration::from_secs(59 * 60));
        assert_eq!(client.wilayas().await.expect("cached").len(), 1);
        assert_eq!(remote.calls(), 1);
        assert!(lock_mutex(&notified).is_empty());
    }

    #[tokio::test]
    async fn a_stale_map_beats_the_snapshot_when_the_courier_fails() {
        let (client, remote, clock, notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.wilayas_value) = vec![wilaya(16)];
        client.wilayas().await.expect("prime");

        clock.advance(Duration::from_secs(2 * 60 * 60));
        remote.set_error(Some(Error::transport("courier is gone")));

        let wilayas = client.wilayas().await.expect("stale");
        assert_eq!(wilayas, vec![wilaya(16)]);
        assert_eq!(remote.calls(), 2);
        assert!(
            lock_mutex(&notified).is_empty(),
            "stale cache must not notify"
        );
    }

    #[tokio::test]
    async fn an_expired_map_refetches_and_refreshes_its_ttl() {
        let (client, remote, clock, _notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.wilayas_value) = vec![wilaya(16)];

        client.wilayas().await.expect("prime");
        clock.advance(Duration::from_secs(61 * 60));
        *lock_mutex(&remote.wilayas_value) = vec![wilaya(31)];
        assert_eq!(client.wilayas().await.expect("refresh"), vec![wilaya(31)]);

        clock.advance(Duration::from_secs(40 * 60));
        assert_eq!(
            client.wilayas().await.expect("fresh again"),
            vec![wilaya(31)]
        );
        assert_eq!(remote.calls(), 2);
    }

    #[tokio::test]
    async fn communes_are_cached_per_wilaya() {
        let (client, remote, _clock, _notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.communes_value) = vec![Commune {
            wilaya_code: 16,
            name_fr: "Bab Ezzouar".into(),
            ..Commune::default()
        }];

        for code in [16, 31, 16] {
            assert_eq!(client.communes(code).await.expect("communes").len(), 1);
        }
        assert_eq!(remote.calls(), 2);
    }

    #[tokio::test]
    async fn a_commune_lookup_returns_the_canonical_name_case_insensitively() {
        let (client, remote, _clock, _notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.communes_value) = vec![Commune {
            wilaya_code: 4,
            name_fr: "Aïn M'lila".into(),
            ..Commune::default()
        }];

        let commune = client
            .commune(4, "  aïn m'lila ")
            .await
            .expect("case-insensitive lookup");
        assert_eq!(commune, "Aïn M'lila");
        assert_eq!(
            client.commune(4, "M'lila").await,
            Err(Error::UnknownCommune)
        );
    }

    #[test]
    fn commune_folding_matches_go_simple_case_folding() {
        assert!(equal_fold("ſ", "s"));
        assert!(equal_fold("S", "ſ"));
        assert!(equal_fold("Σ", "σ"));
        assert!(equal_fold("Σ", "ς"));
        assert!(equal_fold("ς", "σ"));
        assert!(equal_fold("Aïn M'lila", "AÏN M'LILA"));
        assert!(equal_fold("Bab Ezzouar", "BAB EZZOUAR"));
        assert!(equal_fold("باب الزوار", "باب الزوار"));
        // Full case folding would equate these; Go's EqualFold does not.
        assert!(!equal_fold("ß", "ss"));
        assert!(!equal_fold("Aïn M'lila", "Ain M'lila"));
        assert!(!equal_fold("باب الزوار", "عين مليلة"));
    }

    #[tokio::test]
    async fn sandbox_desks_fall_back_locally_and_stay_cached_for_five_minutes() {
        let (client, remote, clock, notified) = scripted_harness("sandbox", true);
        remote.set_error(Some(Error::Api(APIError {
            status: 503,
            code: "courier_error".into(),
            message: "down".into(),
            ..APIError::default()
        })));

        let desks = client.desks(16).await.expect("fallback desks");
        assert_eq!(desks.len(), 1);
        assert_eq!(desks[0].id, "SBX-16");
        assert!(!desks[0].commune_name.is_empty());
        assert_eq!(notified.lock().expect("notified").as_slice(), ["desks"]);
        assert_eq!(remote.calls(), 1);

        clock.advance(Duration::from_secs(4 * 60));
        assert_eq!(client.desks(16).await.expect("cached desks").len(), 1);
        assert_eq!(remote.calls(), 1);

        clock.advance(Duration::from_secs(2 * 60));
        assert_eq!(client.desks(16).await.expect("refetched desks").len(), 1);
        assert_eq!(remote.calls(), 2);
    }

    #[tokio::test]
    async fn real_couriers_never_get_the_local_sandbox() {
        let (client, remote, _clock, notified) = scripted_harness("yalidine", true);
        remote.set_error(Some(Error::Api(APIError {
            status: 503,
            ..APIError::default()
        })));

        assert!(client.create_order(OrderRequest::default()).await.is_err());
        assert!(client.track("yalidine", "YAL-1").await.is_err());
        assert!(client.desks(16).await.is_err());
        assert!(notified.lock().expect("notified").is_empty());
        assert_eq!(remote.calls(), 3);
    }

    #[tokio::test]
    async fn sandbox_falls_back_when_the_courier_is_offline() {
        let (client, remote, _clock, notified) = scripted_harness("sandbox", true);
        client.mark_offline();

        let wilayas = client.wilayas().await.expect("snapshot wilayas");
        assert_eq!(wilayas.len(), 58);
        let shipment = client
            .create_order(OrderRequest {
                reference: "AZ-2026-0041".into(),
                ..OrderRequest::default()
            })
            .await
            .expect("local shipment");
        assert_eq!(shipment.tracking_number, "SBX-AZ-2026-0041");
        let tracking = client
            .track("sandbox", &shipment.tracking_number)
            .await
            .expect("local tracking");
        assert_eq!(tracking.status, "in_transit");
        assert_eq!(tracking.events.len(), 3);

        assert_eq!(remote.calls(), 0);
        assert_eq!(notified.lock().expect("notified").len(), 3);
    }

    #[tokio::test]
    async fn the_offline_window_expires_and_the_network_is_tried_again() {
        let (client, remote, clock, notified) = scripted_harness("sandbox", true);
        remote.set_error(Some(Error::transport("no route")));

        client.wilayas().await.expect("fallback wilayas");
        assert_eq!(remote.calls(), 1);
        clock.advance(Duration::from_secs(60));
        client.communes(16).await.expect("offline communes");
        assert_eq!(remote.calls(), 1, "offline window skips the network");

        clock.advance(Duration::from_secs(61));
        client.communes(31).await.expect("fallback communes");
        assert_eq!(remote.calls(), 2, "the window expired");
        assert_eq!(notified.lock().expect("notified").len(), 3);
    }

    #[tokio::test]
    async fn sandbox_does_not_mask_a_courier_rejection() {
        let (client, remote, _clock, notified) = scripted_harness("sandbox", true);
        remote.set_error(Some(Error::Api(APIError {
            status: 422,
            code: "invalid_phone".into(),
            message: "Invalid phone.".into(),
            ..APIError::default()
        })));

        let error = client
            .create_order(OrderRequest::default())
            .await
            .expect_err("rejection must surface");
        match error {
            Error::Api(api) => {
                assert_eq!(api.status, 422);
                assert_eq!(api.code, "invalid_phone");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(notified.lock().expect("notified").is_empty());
    }

    #[tokio::test]
    async fn sandbox_transport_failures_create_a_local_shipment() {
        let (client, remote, _clock, notified) = scripted_harness("sandbox", true);
        remote.set_error(Some(Error::transport("connection refused")));

        let shipment = client
            .create_order(OrderRequest {
                reference: "AZ-2026-0041".into(),
                recipient: Recipient::default(),
                ..OrderRequest::default()
            })
            .await
            .expect("local shipment");
        assert_eq!(shipment.tracking_number, "SBX-AZ-2026-0041");
        assert_eq!(notified.lock().expect("notified").as_slice(), ["orders"]);
    }

    #[tokio::test]
    async fn a_shipment_without_a_tracking_number_is_rejected() {
        let (client, remote, _clock, _notified) = scripted_harness("yalidine", false);
        *lock_mutex(&remote.shipment_value) = Shipment {
            status: "created".into(),
            ..Shipment::default()
        };

        let error = client
            .create_order(OrderRequest::default())
            .await
            .expect_err("blank tracking");
        let api = error.api().expect("api error");
        assert_eq!(api.status, 502);
        assert_eq!(api.code, "courier_error");
        assert_eq!(api.message, "The courier did not return a tracking number.");
    }

    #[tokio::test]
    async fn unknown_lookups_return_the_typed_errors() {
        let (client, _remote, _clock, _notified) = scripted_harness("sandbox", true);

        assert_eq!(client.wilaya(999).await, Err(Error::UnknownWilaya));
        assert_eq!(
            client.commune(999, "Nowhere").await,
            Err(Error::UnknownCommune)
        );
        assert_eq!(client.desk(16, "NOPE").await, Err(Error::UnknownDesk));
    }

    #[test]
    fn ttl_defaults_are_applied_by_the_client() {
        let client = Client::new(Options::default());
        assert_eq!(client.ttl, DEFAULT_CACHE_TTL);
        assert!(!client.local_sandbox);
    }

    #[test]
    fn the_courier_trait_is_object_safe() {
        let shipping: Arc<dyn Courier> = Arc::new(Client::new(Options::default()));
        let _: &dyn Courier = shipping.as_ref();
    }
}

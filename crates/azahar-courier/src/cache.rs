use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A single cached value with an expiry. A poisoned lock is recovered rather
/// than propagated: cache contents are plain data, so there is no invariant to
/// lose.
pub(crate) struct Cache<T> {
    state: Mutex<State<T>>,
}

struct State<T> {
    value: Option<T>,
    expires: DateTime<Utc>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                value: None,
                expires: DateTime::<Utc>::MIN_UTC,
            }),
        }
    }
}

impl<T: Clone> Cache<T> {
    pub(crate) fn get(&self, now: DateTime<Utc>) -> Option<T> {
        let state = lock(&self.state);
        match &state.value {
            Some(value) if now < state.expires => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn stale(&self) -> Option<T> {
        lock(&self.state).value.clone()
    }

    pub(crate) fn set(&self, value: T, expires: DateTime<Utc>) {
        let mut state = lock(&self.state);
        state.value = Some(value);
        state.expires = expires;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + TimeDelta::seconds(seconds)
    }

    #[test]
    fn a_fresh_value_is_returned_until_its_expiry() {
        let cache = Cache::default();
        cache.set(vec![1, 2, 3], at(60));
        assert_eq!(cache.get(at(0)), Some(vec![1, 2, 3]));
        assert_eq!(cache.get(at(59)), Some(vec![1, 2, 3]));
        assert_eq!(cache.get(at(60)), None);
        assert_eq!(cache.stale(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn an_empty_cache_has_no_fresh_or_stale_value() {
        let cache: Cache<Vec<u8>> = Cache::default();
        assert_eq!(cache.get(at(0)), None);
        assert_eq!(cache.stale(), None);
    }
}

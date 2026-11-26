//! The HTTP client's rate limit, per domain: the generic cell rate algorithm (as governor's), `burst` requests at
//! once and one more every `period` after that.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) struct RateLimiter {
    period: Duration,
    /// how far ahead of now a key's theoretical arrival time may run: the burst less one request
    tolerance: Duration,
    /// each key's theoretical arrival time: when its next request would be on schedule
    tat: Mutex<HashMap<String, Instant>>,
}

impl RateLimiter {
    pub(crate) fn new(period: Duration, burst: u32) -> Self {
        Self {
            period,
            tolerance: period * burst.saturating_sub(1),
            tat: Mutex::new(HashMap::new()),
        }
    }

    /// Takes a request for `key` now, or says how long until one would be allowed.
    pub(crate) fn check_key(&self, key: &str) -> Result<(), Duration> {
        self.check_key_at(key, Instant::now())
    }

    fn check_key_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let mut tat = self
            .tat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = tat.get(key).map_or(now, |&t| t.max(now));
        let ahead = next - now;
        if ahead > self.tolerance {
            return Err(ahead - self.tolerance);
        }
        tat.insert(key.to_string(), next + self.period);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_millis(100);

    /// A burst goes through at once, the next request waits one period, a key's limit isn't another's, and the
    /// requests come back one per period.
    #[test]
    fn burst_then_one_per_period() {
        let limiter = RateLimiter::new(PERIOD, 3);
        let t0 = Instant::now();
        for _ in 0..3 {
            assert_eq!(limiter.check_key_at("spotify.com", t0), Ok(()));
        }
        assert_eq!(limiter.check_key_at("spotify.com", t0), Err(PERIOD));
        assert_eq!(limiter.check_key_at("scdn.co", t0), Ok(()), "another key");
        assert_eq!(
            limiter.check_key_at("spotify.com", t0 + PERIOD / 2),
            Err(PERIOD / 2)
        );
        assert_eq!(limiter.check_key_at("spotify.com", t0 + PERIOD), Ok(()));
        assert_eq!(
            limiter.check_key_at("spotify.com", t0 + PERIOD),
            Err(PERIOD)
        );
    }

    /// Quiet for long, the whole burst is there again (and no more).
    #[test]
    fn the_burst_refills() {
        let limiter = RateLimiter::new(PERIOD, 2);
        let t0 = Instant::now();
        assert!(limiter.check_key_at("k", t0).is_ok());
        assert!(limiter.check_key_at("k", t0).is_ok());
        assert!(limiter.check_key_at("k", t0).is_err());
        let later = t0 + PERIOD * 10;
        assert!(limiter.check_key_at("k", later).is_ok());
        assert!(limiter.check_key_at("k", later).is_ok());
        assert!(limiter.check_key_at("k", later).is_err());
    }
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

/// Token bucket speed limiter.
/// `bytes_per_sec`: max speed in bytes/second. If 0 or None, rate is unlimited.
#[derive(Clone)]
pub struct SpeedLimiter {
    state: Option<Arc<LimiterState>>,
}

struct LimiterState {
    rate: AtomicU64, // bytes per second
    inner: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last_update: Instant,
}

impl SpeedLimiter {
    pub fn new(bytes_per_sec: Option<u64>) -> Self {
        match bytes_per_sec {
            Some(rate) if rate > 0 => {
                let state = Arc::new(LimiterState {
                    rate: AtomicU64::new(rate),
                    inner: Mutex::new(BucketState {
                        tokens: rate as f64,
                        last_update: Instant::now(),
                    }),
                });
                Self { state: Some(state) }
            }
            _ => Self { state: None },
        }
    }

    /// Sets a new rate limit on the fly. 0 disables limit.
    pub fn set_rate(&self, bytes_per_sec: u64) {
        if let Some(state) = &self.state {
            state.rate.store(bytes_per_sec, Ordering::Relaxed);
        }
    }

    /// Consumes `bytes` from the bucket, throttling if necessary.
    pub async fn consume(&self, bytes: usize) {
        let state = match &self.state {
            Some(s) => s,
            None => return, // Unlimited
        };

        let rate = state.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }

        let mut bucket = state.inner.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_update).as_secs_f64();
        bucket.last_update = now;

        // Refill tokens based on elapsed time, capped at 1 second of burst
        let max_tokens = rate as f64;
        bucket.tokens = (bucket.tokens + elapsed * rate as f64).min(max_tokens);

        let needed = bytes as f64;
        if bucket.tokens < needed {
            let deficit = needed - bucket.tokens;
            let wait_secs = deficit / rate as f64;
            bucket.tokens = 0.0;
            bucket.last_update = now + Duration::from_secs_f64(wait_secs);
            drop(bucket);
            sleep(Duration::from_secs_f64(wait_secs)).await;
        } else {
            bucket.tokens -= needed;
        }
    }
}

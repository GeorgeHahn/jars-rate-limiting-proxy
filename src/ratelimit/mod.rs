use crate::Res;
use async_trait::async_trait;

pub mod redis;

// Future: consider implementing an in-process cache for over-limit paths & their calculated
// expiration times (evmap for the underlying store? https://github.com/jonhoo/rust-evmap)

/// Describes a generic rate limiting service
#[async_trait]
pub trait RateLimitStore {
    /// Returns true if the rate limit has not been exceeded for the current minute
    async fn under_limit(&mut self, path: &str, limit: u32) -> Res<bool>;
    async fn incr(&mut self, path: &str) -> Res<()>;
}

use super::RateLimitStore;
use crate::Res;
use async_trait::async_trait;
use redis::AsyncCommands;

#[derive(Clone)]
pub struct RedisRateLimitStore {
    connection: redis::aio::MultiplexedConnection,
}

impl RedisRateLimitStore {
    pub async fn create(uri: String) -> Res<RedisRateLimitStore> {
        let client = redis::Client::open(uri)?;
        let connection = client.get_multiplexed_tokio_connection().await?;
        Ok(RedisRateLimitStore { connection })
    }
}

/// Redis rate limiting implementation
///
/// # Theory of operation
///
/// Store two counts per resource path, one for the current minute and one for the previous minute.
/// Each key records usage for that minute. This design gives a moving average rate limit with a
/// sliding window.
#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    /// Verify that moving average of calls to `path` is under `limit`
    async fn under_limit(&mut self, path: &str, limit: u32) -> Res<bool> {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;

        // Timestamp in minutes since the epoch. (This could be confusing for someone looking
        // directly at the redis keys - it might be worth switching to a standard timestamp rounded
        // to the nearest minute or just minute % 60)
        let current_second = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let current_minute = current_second / 60;

        let previous_path = format!("{}:{}", path, current_minute - 1);
        let current_path = format!("{}:{}", path, current_minute);

        let res: Result<(Option<u32>, Option<u32>), _> =
            self.connection.get(&[previous_path, current_path]).await;

        let (previous, current) = match res {
            Ok((p, c)) => (p.unwrap_or(0), c.unwrap_or(0)),
            Err(e) => return Err(Box::new(e)),
        };

        // No need to calculate the moving average if the current minute is over the allowed rate
        if current >= limit {
            return Ok(false);
        }

        // Caculate a single minute moving average over the last two minutes
        let seconds_into_this_minute = (current_second % 60) as f32;
        let split = (60.0 - seconds_into_this_minute) / 60.0;
        let previous_split = split * (previous as f32);
        let mavg = previous_split + (current as f32);

        Ok((mavg.ceil() as u32) < limit)
    }

    /// Increments the redis key at path:timestamp_m
    async fn incr(&mut self, path: &str) -> Res<()> {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;
        let now_mins = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 60;
        let timestamped_path = format!("{}:{}", path, now_mins);

        // Increment current key and reset expiration to ~2 minutes. This refreshes the expiration
        // value on every increment, which is not necessary and is a reasonable target for
        // optimization.
        // Note: expiration is left a bit longer that the absolute minimum 2 minutes to allow for
        // reasonable operation for reasonable amounts of time skew.
        redis::pipe()
            .incr(&timestamped_path, 1 as u32)
            .expire(&timestamped_path, 130 as usize)
            .query_async(&mut self.connection)
            .await?;
        Ok(())
    }
}

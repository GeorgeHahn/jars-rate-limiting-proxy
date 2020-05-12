use jars::ratelimit::redis::RedisRateLimitStore;
use jars::settings::Settings;
use jars::RequestForwarder;
use jars::Res;

#[tokio::main]
async fn main() -> Res<()> {
    Settings::load().await;

    let path = Settings::get_redis_uri().await;
    let redis = RedisRateLimitStore::create(path).await.unwrap();

    let forwarder = RequestForwarder::create(redis);
    forwarder.run().await?;
    Ok(())
}

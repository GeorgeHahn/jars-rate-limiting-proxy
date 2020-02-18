#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

mod settings;

use async_trait::async_trait;
use futures_util::StreamExt;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{client::HttpConnector, Body, Client as HyperClient, Request, Response, Server};
use log::trace;
use redis::AsyncCommands;
use settings::Settings;
use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::RwLock;
use tokio::task;

type Res<T> = Result<T, Box<dyn Error>>;

lazy_static! {
    static ref SETTINGS: RwLock<Settings> = RwLock::new(Settings::load_and_validate());
}

#[async_trait]
trait RateLimitStore {
    // TODO: is u32 the right option here?
    async fn get(&mut self, path: &str) -> Res<u32>;
    async fn incr(&mut self, path: &str) -> Res<()>;
}

struct RedisRateLimitStore {
    connection: redis::aio::Connection,
}

impl RedisRateLimitStore {
    pub async fn create() -> Res<RedisRateLimitStore> {
        let client = redis::Client::open("redis://127.0.0.1/")?;
        let connection = client.get_async_connection().await?;
        Ok(RedisRateLimitStore { connection })
    }
}

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    /// TODO doc this
    async fn get(&mut self, path: &str) -> Res<u32> {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;

        // get path:timestamp_m - 1
        // get path:timestamp_m
        // return moving average of the two

        let now_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let now_mins = now_seconds / 60;
        let previous_path = format!("{}:{}", path, now_mins - 1);
        let current_path = format!("{}:{}", path, now_mins);

        // TODO: dispatch these concurrently
        let previous: u32 = self.connection.get(&previous_path)
            .await.map_err(|e| {
                println!("{}: {:?}", &previous_path, e);
                e
            }).unwrap_or(0);
        let current = self.connection.get(&current_path).await.unwrap_or(0);

        let seconds_into_this_minute = now_seconds % 60;
        let split = (60 - seconds_into_this_minute) as f32 / 60.0;
        let previous_split = split * (previous as f32);
        let mavg = previous_split + (current as f32);

        println!("mavg: {}, previous: {}, current: {}", mavg, previous, current);

        Ok(mavg.ceil() as u32)
    }

    /// Increments the redis key at path:timestamp_m
    async fn incr(&mut self, path: &str) -> Res<()> {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;
        let now_mins = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 60;
        let timestamped_path = format!("{}:{}", path, now_mins);
        self.connection.incr(&timestamped_path, 1 as u32).await?;
        Ok(())
    }
}

/// Add X-Forwarded headers to request
/// (Currently only implements X-Forwarded-For header)
fn set_proxy_headers(req: &mut Request<Body>, remote_addr: SocketAddr) {
    let ip = remote_addr.ip().to_string();
    let headers = req.headers_mut();
    let xff_value = match headers.get("X-Forwarded-For") {
        Some(existing_xff) => {
            match existing_xff.to_str() {
                Ok(value) => value.to_owned() + "," + &ip,
                Err(_e) => {
                    // drop invalid xff headers (TODO: this may have security implications; review)
                    ip
                }
            }
        }
        None => ip,
    };

    headers.insert(
        "X-Forwarded-For",
        hyper::header::HeaderValue::from_str(&xff_value).unwrap(),
    );

    // TODO: set XFP, XFH headers
}

async fn handle(
    client: HyperClient<HttpConnector>,
    remote_addr: SocketAddr,
    mut req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let uri = req.uri();
    let mut parts = uri.clone().into_parts();

    let mut counter_store = RedisRateLimitStore::create().await.unwrap();

    let path = req.uri().path();

    // todo: consider an in-process cache in front of counter_store to track blocks
    if counter_store.get(path).await.unwrap() < 100 {
        let path = path.to_owned();

        // This ends up doing a whole lot more parsing and validation than we need. At a minimum, we
        // have to modify the request (to add appropriate headers), but the response should be streamed
        // verbatim. Not implemented here, but I suspect this could be done almost entirely in the
        // kernel with io_uring.

        parts.scheme = Some("http".parse().unwrap());
        parts.authority = Some(SETTINGS.read().unwrap().base_path.clone());

        // TODO only replace base path
        *req.uri_mut() = http::Uri::from_parts(parts).unwrap();

        // Add proxy headers
        set_proxy_headers(&mut req, remote_addr);

        // Send request to upstream server
        let response = client.request(req);

        // Stream response back to client
        let (mut tx, body) = Body::channel();
        task::spawn(async move {
            // This is kind of the worst stream forwarder ever. Need to verify that it works.
            let mut responsebody = response.await.unwrap().into_body();

            while let Some(Ok(data)) = responsebody.next().await {
                tx.send_data(data).await.expect("fwd data");
            }
        });

        // Note: requests at the very end of a second will be counted towards the
        // next second's quota. This is considered acceptable.
        let _ = counter_store.incr(&path).await.map_err(|e| println!("Unable to incr key {}: {:?}", &path, e));
        Ok(hyper::Response::builder().body(body).unwrap())
    } else {
        // TODO: pass error
        // TODO: Set local cache to 'blocked' and set tokio timer to unblock
        // TODO: cache error

        return Ok(hyper::Response::builder()
            .status(200) // TODO get that value from hyper
            .body(hyper::Body::from(format!("Rate limited\n\n{:?}\n\n{:?}", req.uri(), &req)))
            .unwrap());
    }
}

struct RequestForwarder {
    counter_store: RedisRateLimitStore,
}

impl RequestForwarder {
    pub fn create(counter_store: RedisRateLimitStore) -> Self {
        Self { counter_store }
    }

    pub async fn run(self) -> Res<()> {
        // TODO: make this configurable
        let addr = SocketAddr::from(([127, 0, 0, 1], 80));

        // This client will be cloned for proxying below
        let client = HyperClient::builder().keep_alive(false).build_http();

        // Set max buf size to something reasonable for streaming (hyper defaults to ~400k)
        // .http1_max_buf_size()

        let make_service = make_service_fn(move |conn: &AddrStream| {
            let remote = conn.remote_addr();

            // The amount of clones to get a connection pool here is pretty bad.
            // Ugliness aside, client is a fair number of bytes large.
            // TODO: verify connection pooling helps perf & cannot be implemented more cleanly
            let c = client.clone();
            async move {
                let c = c.clone();
                Ok::<_, Infallible>(service_fn(move |body| handle(c.clone(), remote, body)))
            }
        });

        let server = Server::bind(&addr).serve(make_service);

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        Ok(())
    }
}

fn load_settings() {
    // SETTINGS is lazily initialized; this line will cause load_and_validate to be called.
    SETTINGS.read().unwrap();
}

#[tokio::main]
async fn main() -> Res<()> {
    load_settings();

    // TODO: subscribe to etcd settings updates
    //     ConfigProviderEnum::Etcd => {
    //         let config_path = static_config.get_etcd_uri();
    //         // TODO: Etcd auth
    //         // EtcdConfigProvider::create(..)
    //         let provider = EtcdConfigProvider::connect();
    //         Settings::with_provider(provider)?
    //     }
    // };

    let redis = RedisRateLimitStore::create().await?;
    let forwarder = RequestForwarder::create(redis);
    forwarder.run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::HeaderValue;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn xff_header_is_added_ipv4() {
        let mut req = Request::new(Body::empty());
        let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        set_proxy_headers(&mut req, remote);
        assert_eq!(
            req.headers().get("X-Forwarded-For"),
            Some(&HeaderValue::from_static("127.0.0.1"))
        );
    }

    #[test]
    fn xff_header_is_added_ipv6() {
        let mut req = Request::new(Body::empty());
        let remote = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080);
        set_proxy_headers(&mut req, remote);
        assert_eq!(
            req.headers().get("X-Forwarded-For"),
            Some(&HeaderValue::from_static("::1"))
        );
    }

    #[test]
    fn xff_header_can_append() {
        let mut req = Request::new(Body::empty());
        req.headers_mut()
            .insert("X-Forwarded-For", HeaderValue::from_static("127.0.1.1"));
        let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        set_proxy_headers(&mut req, remote);
        assert_eq!(
            req.headers().get("X-Forwarded-For"),
            Some(&HeaderValue::from_static("127.0.1.1,127.0.0.1"))
        );
    }
}

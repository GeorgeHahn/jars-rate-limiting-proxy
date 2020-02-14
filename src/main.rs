#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

mod settings;

use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, Client as HyperClient, client::HttpConnector};
use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::RwLock;
use settings::Settings;
use tokio::task;
use futures_util::FutureExt;
use futures_util::StreamExt;
use bytes::Bytes;

type Res<T> = Result<T, Box<dyn Error>>;

lazy_static! {
    static ref SETTINGS: RwLock<Settings> = RwLock::new(Settings::load_and_validate());
}

trait CounterStore {
    // TODO: is u32 the right option here?
    fn get(&self, path: &str) -> u32;
    fn incr(&self, path: &str);
}

#[derive(Clone)]
struct RedisCounterStore {}

impl CounterStore for RedisCounterStore {
    fn get(&self, path: &str) -> u32 {
        // get path:timestamp_s - 1
        // get path:timestamp_s
        // return moving average of the two
        0
    }

    fn incr(&self, path: &str) {
        // increment path::timestamp_s
    }
}

/// Add X-Forwarded headers to request
/// (Currently only implements X-Forwarded-For header)
fn set_xf_headers(req: &mut Request<Body>, remote_addr: SocketAddr) {
    let ip = remote_addr.ip().to_string();
    let headers = req.headers_mut();
    let xff_value = match headers.get("X-Forwarded-For") {
        Some(existing_xff) => {
            match existing_xff.to_str() {
                Ok(value) => {
                    value.to_owned() + "," + &ip
                }
                Err(_e) => {
                    // drop invalid xff headers (TODO: this may have security implications; review)
                    ip
                }
            }
        },
        None => ip,
    };

    headers.insert(
        "X-Forwarded-For",
        hyper::header::HeaderValue::from_str(&xff_value).unwrap(),
    );

    // TODO: set XFP, XFH headers
}

async fn handle(client: HyperClient<HttpConnector<>>, remote_addr: SocketAddr, mut req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let uri = req.uri();

    // println!("Uri: {:?}", &uri);
    // println!("Request: {:?}", &req);
    let mut parts = uri.clone().into_parts();

    let counter_store = RedisCounterStore {};

    // todo: local cache in front of counter_store to track blocks
    if counter_store.get(req.uri().path()) < 100 {
        // This ends up doing a whole lot more parsing and validation than we need. At a minimum, we
        // have to modify the request (to add appropriate headers), but the response should be streamed
        // verbatim. Not implemented here, but I suspect the current optimal solution would be to use
        // io_uring to stream this back along the response.

        // TODO: support https?
        parts.scheme = Some("http".parse().unwrap());

        parts.authority = Some(SETTINGS.read().unwrap().base_path.clone());

        // TODO only replace base path
        *req.uri_mut() = http::Uri::from_parts(parts).unwrap();

        // Add proxy headers
        set_xf_headers(&mut req, remote_addr);

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
        counter_store.incr("some path");
        Ok(hyper::Response::builder().body(body).unwrap())
    } else {
        // TODO: pass error
        // TODO: Set local cache to 'blocked' and set tokio timer to unblock
        // TODO: cache error

        return Ok(hyper::Response::builder()
            .status(429) // TODO get that value from hyper
            .header("X-Custom-Foo", "Bar")
            .body(hyper::Body::from(format!("{:?}\n\n{:?}", req.uri(), &req)))
            .unwrap());
    }
}

struct RequestForwarder {
    counter_store: RedisCounterStore,
}

impl RequestForwarder {
    pub fn create(counter_store: RedisCounterStore) -> Self {
        Self { counter_store }
    }

    pub async fn run(self) -> Res<()> {
        // TODO: make this configurable
        let addr = SocketAddr::from(([127, 0, 0, 1], 80));

        // This client will be cloned for proxying below
        let client = HyperClient::builder().keep_alive(false).build_http();

        // Set max buf size to something reasonable for streaming (hyper defaults to ~400k)
        // .http1_max_buf_size()

        let make_service =
            make_service_fn(move |conn: &AddrStream| {
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

    let forwarder = RequestForwarder::create(RedisCounterStore {});
    forwarder.run().await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, IpAddr, Ipv6Addr};
    use headers::HeaderValue;

    #[test]
    fn xff_header_is_added_ipv4() {
        let mut req = Request::new(Body::empty());
        let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        set_xf_headers(&mut req, remote);
        assert_eq!(req.headers().get("X-Forwarded-For"), Some(&HeaderValue::from_static("127.0.0.1")));
    }

    #[test]
    fn xff_header_is_added_ipv6() {
        let mut req = Request::new(Body::empty());
        let remote = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080);
        set_xf_headers(&mut req, remote);
        assert_eq!(req.headers().get("X-Forwarded-For"), Some(&HeaderValue::from_static("::1")));
    }

    #[test]
    fn xff_header_can_append() {
        let mut req = Request::new(Body::empty());
        req.headers_mut().insert("X-Forwarded-For", HeaderValue::from_static("127.0.1.1"));
        let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        set_xf_headers(&mut req, remote);
        assert_eq!(req.headers().get("X-Forwarded-For"), Some(&HeaderValue::from_static("127.0.1.1,127.0.0.1")));
    }
}

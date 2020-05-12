#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

pub mod ratelimit;
pub mod settings;

use crate::ratelimit::RateLimitStore;
use crate::settings::Settings;
use http::Uri;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{client::HttpConnector, Body, Client as HyperClient, Request, Response, Server};
use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

// TODO: define a custom crate error type. This shortcut is okay, but won't ever provide great error
// handling or display.
pub type Res<T> = Result<T, Box<dyn Error>>;

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

    // Not implemented: set XFP, XFH headers
}

async fn proxy(
    client: HyperClient<HttpConnector>,
    remote_addr: SocketAddr,
    mut req: Request<Body>,
    dest: Uri,
) -> Res<Response<Body>> {
    // Add proxy headers
    set_proxy_headers(&mut req, remote_addr);

    // Replace request path
    *req.uri_mut() = dest;

    // Send request to upstream server. This ends up doing more parsing and validation than strictly
    // necessary. The request needs to be modified to add headers, but the response can be streamed
    // verbatim.
    let response = client.request(req).await?;

    Ok(response)
}

async fn handle<T>(
    counter_store: T,
    client: HyperClient<HttpConnector>,
    remote_addr: SocketAddr,
    req: Request<Body>,
) -> Result<Response<Body>, Infallible>
where
    T: RateLimitStore + Clone + Send + 'static,
{
    let path = req.uri().path().to_owned();
    let mut counter_store = counter_store.clone();

    if counter_store.under_limit(&path, 100).await.unwrap() {
        // Replace request base path
        let uri = req.uri();
        let mut parts = uri.clone().into_parts();

        // All requests in & out are currently insecure
        parts.scheme = Some("http".parse().unwrap());

        // Rewrite the uri authority to the upstream path. This could use more sophisticated
        // rewriting rules.
        parts.authority = Some(Settings::get_base_path().await);

        let dest = http::Uri::from_parts(parts).expect("unable to assemble destination uri");
        let response = proxy(client, remote_addr, req, dest).await;

        // Count this request
        let _ = task::spawn(async move {
            counter_store
                .incr(&path)
                .await
                .map_err(|e| println!("Unable to incr key: {:?}", e))
        });

        // Handle proxy request failures
        let response = response.unwrap_or_else(|_| {
            Response::builder()
                .status(500)
                .body("Internal Server Error".into())
                .unwrap()
        });

        Ok(response)
    } else {
        // Note: requests when over-limit are not counted

        let error_path = Settings::get_error_path().await;
        // Future improvement: cache error page
        let response = proxy(client, remote_addr, req, error_path).await;

        match response {
            Ok(mut response) => {
                // Future improvement: implement configurable status code behavior
                *response.status_mut() = hyper::StatusCode::TOO_MANY_REQUESTS;
                Ok(response)
            }

            // Error page request failed
            Err(_e) => Ok(Response::builder()
                .status(500)
                .body("Internal Server Error".into())
                .unwrap()),
        }
    }
}

pub struct RequestForwarder<T>
where
    T: RateLimitStore + Clone + Send,
{
    rate_limiter: T,
}

impl<T> RequestForwarder<T>
where
    T: RateLimitStore + Clone + Send + 'static,
{
    pub fn create(rate_limiter: T) -> Self {
        let rate_limiter = rate_limiter;

        Self { rate_limiter }
    }

    pub async fn run(self) -> Res<()> {
        // The server portion of this service is built on top of the Hyper crate. This should make
        // it easy to support accepting other http versions or TLS connections.

        let addr: SocketAddr = Settings::get_listen().await;
        println!("Listening on {}", addr);

        // This is a base client that will be cloned for proxying. This also initializes the client
        // connection pool.
        let client = HyperClient::builder()
            .keep_alive(true)
            .keep_alive_timeout(Duration::from_secs(120))
            .build_http();

        let make_service = make_service_fn(move |conn: &AddrStream| {
            // this is called once for every incoming connection
            // Consider explicitly implementing a tower service for this (better code organization)

            // Capture the incoming SocketAddr
            let remote = conn.remote_addr();

            // The amount of clones per request is unfortunate. TODO: this would be worth figuring
            // out some way to clean up.
            let rate_limiter = self.rate_limiter.clone();
            let client = client.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |body| {
                    // this is called for each incoming request
                    handle(rate_limiter.clone(), client.clone(), remote, body)
                }))
            }
        });

        let server = Server::bind(&addr).serve(make_service);

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        Ok(())
    }
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

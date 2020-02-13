use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;

mod config;
use config::{
    Config, ConfigProviderEnum, EnvStaticConfig, EtcdConfigProvider, StaticConfigProvider,
};

type Res<T> = Result<T, Box<dyn Error>>;

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

async fn handle(mut req: Request<Body>) -> Result<Response<Body>, Infallible> {
    use hyper::Client;
    // TODO use a client pool

    let uri = req.uri();

    println!("Uri: {:?}", &uri);
    println!("Request: {:?}", &req);
    let mut parts = uri.clone().into_parts();

    let counter_store = RedisCounterStore {};

    let res = if counter_store.get(req.uri().path()) < 100 {
        // pass path_and_query
        // TODO: pass req & response

        let base_path = "127.0.0.1:81";
        parts.scheme = Some("http".parse().unwrap());
        parts.authority = Some(base_path.parse().unwrap());

        let client = Client::new();

        // TODO only replace base path
        // TODO add proxy headers?
        *req.uri_mut() = http::Uri::from_parts(parts).unwrap();
        req.headers_mut().insert(
            "X-Did-Proxy",
            hyper::header::HeaderValue::from_static("Yep.       "),
        );

        // let result = client.request(req);
        // Ok(hyper::Response::builder().body(hyper::Body::wrap_stream(result)))

        // TODO: Stream response back
        Ok(client.request(req).await.unwrap())
    } else {
        // TODO: pass error
        // TODO: Set local cache to 'blocked' and set tokio timer to unblock
        // TODO: cache error (w/ dynamically configurable ttl)

        Ok(hyper::Response::builder()
            .status(429) // TODO get that value from hyper
            .header("X-Custom-Foo", "Bar")
            .body(hyper::Body::from(format!("{:?}\n\n{:?}", req.uri(), &req)))
            .unwrap())
    };

    // Note: requests at the very end of a second will be counted towards the
    // next second's quota. This is considered acceptable.
    counter_store.incr("some path");

    res
}

// Drop this into a handler for an easy proxy target
// Ok(hyper::Response::builder()
//     .status(200)
//     .header("X-Custom-Foo", "Bar")
//     .body(hyper::Body::from(format!("{:?}\n\n{:?}", req.uri(), &req)))
//     .unwrap());

struct RequestForwarder {
    static_config: EnvStaticConfig,
    dynamic_config: Config,
    counter_store: RedisCounterStore,
}

impl RequestForwarder {
    pub fn create(
        static_config: EnvStaticConfig,
        dynamic_config: Config,
        counter_store: RedisCounterStore,
    ) -> Self {
        Self {
            static_config,
            dynamic_config,
            counter_store,
        }
    }

    pub async fn run(self) -> Res<()> {
        // TODO: make this configurable
        let addr = SocketAddr::from(([127, 0, 0, 1], 80));

        let make_service =
            make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

        let server = Server::bind(&addr).serve(make_service);

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Res<()> {
    // todo static config via env (future: add file support)
    let static_config = EnvStaticConfig;

    let dynamic_config = match static_config.get_config_provider() {
        ConfigProviderEnum::File => {
            let config_path = static_config.get_config_path();
            let provider = StaticConfigProvider::from_file(config_path);
            Config::with_provider(provider)?
        }
        ConfigProviderEnum::Etcd => {
            let config_path = static_config.get_etcd_uri();
            // TODO: Etcd auth
            // EtcdConfigProvider::create(..)
            let provider = EtcdConfigProvider::connect();
            Config::with_provider(provider)?
        }
    };

    let forwarder = RequestForwarder::create(static_config, dynamic_config, RedisCounterStore {});
    forwarder.run().await?;
    Ok(())
}

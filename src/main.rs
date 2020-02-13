use async_trait::async_trait;
use futures::stream::Stream;
use futures_util::StreamExt;
use std::error::Error;
use std::sync::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;
use tokio::net::TcpStream;
use std::env;

fn connect_and_forward_really_quickly(/*RequestStream, ResponseAddr*/) {}

// accept..
// `GET URIHASH:MINUTE-1 URIHASH:MINUTE`
// if not limited
// Make request + stream response
// `INCR URIHASH:MINUTE`

type Res<T> = Result<T, Box<dyn Error>>;

#[async_trait]
trait ConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = ConfigInner>>>;
}

struct EtcdConfigProvider {
    client: etcd_rs::Client,
}

#[async_trait]
impl ConfigProvider for EtcdConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = ConfigInner>>> {
        use etcd_rs::*;

        // The etcd-rs api leaves a lot to be desired. I will do some refactoring and
        // see about upstreaming. It shouldn't be too hard to get this to look like
        // `async self.client.watch(KeyRange) -> impl Stream`.
        let mut watch = self.client.watch();
        watch.watch(WatchRequest::create(KeyRange::key("base_path"))).await;

        Ok(Box::new(watch.responses().filter_map(|val| async move {
            // TODO
            Some(ConfigInner {
                base_path: "".to_owned(),
                errorpath: "".to_owned(),
            })
        })))
    }
}

impl EtcdConfigProvider {
    fn connect() -> Self {
        todo!("connect")
        // let client = Client::connect(ClientConfig {
        //     endpoints: vec!["http://127.0.0.1:2379".to_owned()],
        //     auth: None,
        // })
        // .await?;

    }
}

struct StaticConfigProvider {
    // read config from toml
}

#[async_trait]
impl ConfigProvider for StaticConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = ConfigInner>>> {
        // todo return completed stream
        unimplemented!()
    }
}

impl StaticConfigProvider {
    pub fn from_file<T: AsRef<str>>(path: T) -> Self {
        StaticConfigProvider {
            // todo...
        }
    }
}

// The Config struct holds one copy of the provider and fans out all config changes to its clones
struct Config {
    inner: RwLock<ConfigInner>,
}

#[derive(Default)]
struct ConfigInner {
    base_path: String,
    errorpath: String,
    // todo other shit
}

impl Config {
    pub fn with_provider<T: 'static + ConfigProvider>(provider: T) -> Res<Config> {
        // Take generic config provider and wire it up to supply updates to config via mpsc channels
        // This has a few benefits: many clones of config can be kept up to date without any synchronization whatsoever.
        // This also keeps config accesses super fast (no indirection whatsoever)

        let this = Self {
            inner: RwLock::new(Default::default()),
        };

        // TODO: spawn_local is not appropriate here
        // TODO: update inner when provider sends updated config
        // task::spawn_local(async move {
        //     let stream = provider.get().await.unwrap(); // todo handle error

        //     while let Some(incoming) = stream.next().await {
        //         *this.inner.write().unwrap() = incoming;
        //     }
        // });

        Ok(this)
    }

    pub fn base_path(&self) -> String {
        // TODO: consider futures_locks::RwLock
        self.inner.read().unwrap().base_path.to_owned()
    }

    // pub fn error_path // TODO

    // interior mutability, call fn to get the latest config value?
}

trait CounterStore {
    // TODO: is u32 the right option here?
    fn get(&self, path: &str) -> u32;
    fn incr(&self, path: &str);
}

#[derive(Clone)]
struct RedisCounterStore {

}

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

struct RequestForwarder {
    static_config: EnvStaticConfig,
    dynamic_config: Config,
    counter_store: RedisCounterStore,
}

impl RequestForwarder {
    pub fn create(static_config: EnvStaticConfig, dynamic_config: Config, counter_store: RedisCounterStore) -> Self {
        Self { static_config, dynamic_config, counter_store }
    }

    fn process(&self, mut socket: TcpStream) {
        let counter_store = self.counter_store.clone();

        tokio::spawn(async move {
            // todo read just enough of the request to get the url
            // Turns out requests are small enough that this probably isn't a huge deal
            let mut buf = [0u8; 1024];

            // Peek bytes from the socket until we have the request path
            let n = match socket.peek(&mut buf).await {
                // socket closed
                Ok(n) if n == 0 => return,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("failed to read from socket; err = {:?}", e);
                    return;
                }
            };

            // Write the data back
            if let Err(e) = socket.write_all(&buf[0..n]).await {
                eprintln!("failed to write to socket; err = {:?}", e);
                return;
            }

            if counter_store.get("some path") < 100 {
                // TODO: pass req & response
            } else {
                // TODO: pass error
                // TODO: Set local cache to 'blocked' and set tokio timer to unblock
                // TODO: cache error (w/ dynamically configurable ttl)
            }

            // Note: requests at the very end of a second will be counted towards the
            // next second's quota. This is considered acceptable.
            counter_store.incr("some path");
        });
    }

    pub async fn run(self) -> Res<()> {
        use tokio::net::TcpListener;

        // TODO: make this configurable
        let mut listener = TcpListener::bind(self.static_config.get_listen_uri()).await?;

        loop {
            let (socket, _) = listener.accept().await?;
            self.process(socket);
        }
    }
}

enum ConfigProviderEnum {
    File,
    Etcd,
}

struct EnvStaticConfig;

impl EnvStaticConfig {
    pub fn get_config_provider(&self) -> ConfigProviderEnum {
        // match env::var("CONFIG_PROVIDER").unwrap().as_ref() {
        //     "file" => ConfigProviderEnum::File,
        //     "etcd" => ConfigProviderEnum::Etcd,
        //     _ => panic!("Invalid CONFIG_PROVIDER"), // todo this can be better
        // }
        ConfigProviderEnum::File
    }

    pub fn get_config_path(&self) -> String {
        // env::var("CONFIG_PATH").unwrap()
        "None".to_owned()
    }

    pub fn get_etcd_uri(&self) -> String {
        env::var("ETCD_URI").unwrap()
    }

    pub fn get_listen_uri(&self) -> String {
        "127.0.0.1:80".to_owned()
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
        },
        ConfigProviderEnum::Etcd => {
            let config_path = static_config.get_etcd_uri();
            // TODO: Etcd auth
            // EtcdConfigProvider::create(..)
            let provider = EtcdConfigProvider::connect();
            Config::with_provider(provider)?
        }
    };

    let forwarder = RequestForwarder::create(static_config, dynamic_config, RedisCounterStore { });
    forwarder.run().await?;
    Ok(())
}

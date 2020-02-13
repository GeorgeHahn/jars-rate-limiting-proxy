use crate::Res;
use async_trait::async_trait;
use futures::stream::Stream;
use futures_util::StreamExt;
use std::env;
use std::sync::RwLock;

#[async_trait]
pub trait ConfigProvider {
    async fn get(self) -> Res<Box<dyn Stream<Item = ConfigInner>>>;
}

pub struct EtcdConfigProvider {
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
        watch
            .watch(WatchRequest::create(KeyRange::key("base_path")))
            .await;

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
    pub fn connect() -> Self {
        todo!("connect")
        // let client = Client::connect(ClientConfig {
        //     endpoints: vec!["http://127.0.0.1:2379".to_owned()],
        //     auth: None,
        // })
        // .await?;
    }
}

pub struct StaticConfigProvider {
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
pub struct Config {
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
        // self.inner.read().unwrap().base_path.to_owned()
        "127.0.0.1:3000".to_owned()
    }

    // pub fn error_path // TODO

    // interior mutability, call fn to get the latest config value?
}

pub enum ConfigProviderEnum {
    File,
    Etcd,
}

pub struct EnvStaticConfig;

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

// TODO: Wipe out most of this config with a layered config provider
// Only code remaining here should be the etcd dynamic config support

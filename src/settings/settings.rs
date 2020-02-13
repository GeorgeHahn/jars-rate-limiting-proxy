use super::etcd::EtcdConfigProvider;
use crate::Res;
use config::{Config, ConfigError, Environment, File};
use futures_util::{StreamExt, TryFutureExt};
use http::{uri::Authority, Uri};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task;

/// On-disk settings representation
#[derive(Deserialize, Debug)]
pub struct SettingsSerdes {
    /// IP and port to listen for connections on
    pub listen: SocketAddr,

    /// Base uri to rewrite incoming requests
    pub base_path: String,

    /// Path to forward for errors
    pub error_path: String,

    /// Redis connection path (redis://<ip>:<port>/<db>)
    pub redis_uri: String,

    /// Etcd cluster Uris
    pub etcd_uris: Option<Vec<String>>,

    /// Set prefix for Etcd config keys
    pub etcd_key_prefix: Option<String>,
}

/// Parsed/runtime settings representation
#[derive(Clone, Debug)]
pub struct Settings {
    /// IP and port to listen for connections on
    pub listen: SocketAddr,

    /// Base uri to rewrite incoming requests
    pub base_path: Authority,

    /// Path to forward for errors
    pub error_path: Uri,

    /// Redis connection path (redis://<ip>:<port>/<db>)
    pub redis_uri: String,

    /// Etcd cluster Uris
    pub etcd_uris: Option<Vec<String>>,

    /// Set prefix for Etcd config keys
    pub etcd_key_prefix: String,
}

// Not all settings can currently be changed at runtime. This can be improved significantly with
// some effort.

/// A single runtime settings change
#[derive(Debug)]
pub enum DynamicSettings {
    BasePath(Authority),
    ErrorPath(Uri),
}

lazy_static! {
    // Application-wide configuration is implemented with global state. This seemed like an okay
    // tradeoff when I thought I would need threadlocals and fanout logic to avoid a high-contention
    // mutex. After some research, I found that tokio's RwLock is reasonable for this application
    // and avoids contention on reads. It would be well worth refactoring this to eliminate the
    // global state.
    //
    // Tokio RwLock is fair to readers & writers. Watch out for other RwLock implementations -
    // fairness guarantees differ. (This is very read-heavy; unfair implementations may block
    // writers.)
    static ref SETTINGS: Arc<RwLock<Settings>> = {
        let settings = Settings::load_and_validate();

        let etcd_uris = settings.etcd_uris.clone();
        let etcd_key_prefix = settings.etcd_key_prefix.clone();
        let settings = Arc::new(RwLock::new(settings));

        // Subscribe to dynamic settings
        if let Some(etcd_uris) = etcd_uris {
            // TODO: add an error when launched with an empty cluster config
            task::spawn(Settings::listen_for_dynamic_settings(settings.clone(), etcd_uris, etcd_key_prefix).map_err(|e| {
                println!("Unable to watch for dynamic settings changes, shutting down. (error {:?})", e);
                std::process::exit(1);
            }));
        }

        settings
    };
}

// Future: profile this. If Tokio's RwLock is causing a bottleneck, consider these options:
// - hold settings in an `evmap` (https://github.com/jonhoo/rust-evmap)
// - hold a copy of settings in threadlocal & fan out updates using an SPMC queue

impl Settings {
    pub async fn load() {
        // SETTINGS is lazily initialized; this line will cause load_and_validate to be called.
        SETTINGS.read().await;
    }

    /// Get the IP and port to listen for connections on
    pub async fn get_listen() -> SocketAddr {
        SETTINGS.read().await.listen.clone()
    }

    /// Get the Path to forward requests to
    pub async fn get_base_path() -> Authority {
        SETTINGS.read().await.base_path.clone()
    }

    /// Get the Path to forward for errors
    pub async fn get_error_path() -> Uri {
        SETTINGS.read().await.error_path.clone()
    }

    /// Get the Redis connection path
    pub async fn get_redis_uri() -> String {
        SETTINGS.read().await.redis_uri.clone()
    }

    /// Get the Etcd connection path
    pub async fn get_etcd_uris() -> Option<Vec<String>> {
        SETTINGS.read().await.etcd_uris.clone()
    }

    pub async fn get_etcd_key_prefix() -> String {
        SETTINGS.read().await.etcd_key_prefix.clone()
    }

    fn err_and_exit<T>(err: String) -> T {
        println!("{}", err);
        std::process::exit(1);
    }

    pub fn load_and_validate() -> Settings {
        let mut settings = Config::default();
        settings
            .merge(File::with_name("settings.toml").required(true))
            .unwrap_or_else(|_e| {
                println!("Settings file not found (looked for `settings.toml`)");
                std::process::exit(1);
            });

        // Env takes precedence. TODO: Verify that this is a reasonable decision & document it.
        settings
            .merge(Environment::with_prefix("JARS"))
            .unwrap_or_else(|_e| {
                println!("Error loading settings from environment");
                std::process::exit(1);
            });

        let settings: Result<SettingsSerdes, ConfigError> = settings.try_into();

        let settings = match settings {
            Ok(res) => Self {
                listen: res.listen,
                base_path: res.base_path.parse().unwrap_or_else(|e| {
                    Settings::err_and_exit(format!("Invalid base_path: {}", e))
                }),
                error_path: res.error_path.parse().unwrap_or_else(|e| {
                    Settings::err_and_exit(format!("Invalid error_path: {}", e))
                }),
                redis_uri: res.redis_uri,
                etcd_uris: res.etcd_uris,
                etcd_key_prefix: res.etcd_key_prefix.unwrap_or("jars_config_".to_owned()),
            },
            Err(_e) => {
                // TODO: this error message & path can both be improved
                println!("Settings file is invalid");
                std::process::exit(1);
            }
        };

        settings
    }

    pub async fn listen_for_dynamic_settings(
        settings: Arc<RwLock<Settings>>,
        etcd_uris: Vec<String>,
        etcd_prefix: String,
    ) -> Res<()> {
        let client = EtcdConfigProvider::connect(etcd_uris).await?;
        let mut stream = client.get_and_stream(etcd_prefix).await?;
        while let Some(incoming) = stream.next().await {
            println!("Updating settings value: {:?}", incoming);
            let mut writeable = settings.write().await;
            match incoming {
                DynamicSettings::BasePath(val) => writeable.base_path = val,
                DynamicSettings::ErrorPath(val) => writeable.error_path = val,
            }
        }
        Ok(())
    }
}

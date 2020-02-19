use config::{Config, ConfigError, Environment, File};
use http::uri::Authority;
use std::net::SocketAddr;
use std::sync::RwLock;

#[derive(Deserialize, Debug)]
pub struct SettingsSerdes {
    /// IP and port to listen for connections on
    pub listen: SocketAddr,

    /// Path to forward requests to
    pub base_path: String,

    /// Path to forward for errors
    pub error_path: String,

    /// Error page will be cached for the given number of seconds if this is set to a nonzero value
    pub error_ttl: Option<u32>,

    /// Redis URI
    pub redis_uri: String,

    /// Etcd (TODO)
    pub etcd_uri: Option<String>,
    // TODO: remaining etcd connection parameters
}

pub struct Settings {
    /// IP and port to listen for connections on
    pub listen: SocketAddr,

    /// Path to forward requests to
    pub base_path: Authority,

    /// Path to forward for errors
    pub error_path: Authority,

    /// Redis URI
    pub redis_uri: String,

    /// Error page will be cached for the given number of seconds if this is set to a nonzero value
    pub error_ttl: Option<u32>,
}

lazy_static! {
    static ref SETTINGS: RwLock<Settings> = RwLock::new(Settings::load_and_validate());
}

// TODO: profile this. If there's mutex contention, consider one of two options:
// - hold a copy of settings in threadlocal & update using an SPMC queue
// - hold settings in an `evmap`

impl Settings {
    pub fn load() {
        // SETTINGS is lazily initialized; this line will cause load_and_validate to be called.
        SETTINGS.read().unwrap();
    }

    /// Get the IP and port to listen for connections on
    pub fn get_listen() -> SocketAddr {
        SETTINGS.read().unwrap().listen.clone()
    }

    /// Get the Path to forward requests to
    pub fn get_base_path() -> Authority {
        SETTINGS.read().unwrap().base_path.clone()
    }

    /// Get the Path to forward for errors
    pub fn get_error_path() -> Authority { // TODO Authority -> Uri
        SETTINGS.read().unwrap().error_path.clone()
    }

    /// Get the Redis URI
    pub fn get_redis_uri() -> String {
        SETTINGS.read().unwrap().redis_uri.clone()
    }

    /// Get the Error page will be cached for the given number of seconds if this is set to a nonzero value
    pub fn get_error_ttl() -> Option<u32> {
        SETTINGS.read().unwrap().error_ttl.clone()
    }

    fn err_and_exit<T>(err: String) -> T{
        println!("{}", err);
        std::process::exit(1);
    }

    pub fn load_and_validate() -> Settings {
        // TODO: subscribe to etcd settings updates
        //     ConfigProviderEnum::Etcd => {
        //         let config_path = static_config.get_etcd_uri();
        //         // TODO: Etcd auth
        //         // EtcdConfigProvider::create(..)
        //         let provider = EtcdConfigProvider::connect();
        //         Settings::with_provider(provider)?
        //     }
        // };

        let mut settings = Config::default();
        settings
            .merge(File::with_name("settings.toml").required(true))
            .unwrap_or_else(|e| {
                println!("Settings file not found (looked for `settings.toml`)");
                std::process::exit(1);
            });

        // Env takes precedence. TODO: Verify that this is a reasonable decision & document it.
        settings
            .merge(Environment::with_prefix("JARS"))
            .unwrap_or_else(|e| {
                println!("Error loading settings from environment");
                std::process::exit(1);
            });

        let settings: Result<SettingsSerdes, ConfigError> = settings.try_into();

        match settings {
            Ok(res) => Self {
                listen: res.listen,
                base_path: res.base_path.parse().unwrap_or_else(|e| Settings::err_and_exit(format!("Invalid base_path: {}", e))),
                error_path: res.error_path.parse().unwrap_or_else(|e| Settings::err_and_exit(format!("Invalid error_path: {}", e))),
                error_ttl: res.error_ttl,
                redis_uri: res.redis_uri,
            },
            Err(e) => {
                println!("Unable to read settings file (invalid)");
                std::process::exit(1);
            }
        }
    }
}

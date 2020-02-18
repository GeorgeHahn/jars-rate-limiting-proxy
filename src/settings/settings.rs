use config::{Config, ConfigError, Environment, File};
use http::uri::Authority;
use std::net::SocketAddr;

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

    /// Error page will be cached for the given number of seconds if this is set to a nonzero value
    pub error_ttl: Option<u32>,
}

impl Settings {
    pub fn load_and_validate() -> Settings {
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
                base_path: res.base_path.parse().expect("invalid base_path"),
                error_path: res.error_path.parse().expect("invalid error_path"),
                error_ttl: res.error_ttl,
            },
            Err(e) => {
                println!("Unable to read settings file (invalid)");
                std::process::exit(1);
            }
        }
    }
}

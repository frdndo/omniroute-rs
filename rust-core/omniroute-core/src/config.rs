use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub port: u16,
    pub host: String,
    pub jwt_secret: String,
    pub api_key_secret: String,
    pub db_path: String,
    pub data_dir: String,
    pub allowed_hosts: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        config::Config::builder()
            .add_source(
                config::Environment::default()
                    .prefix("OMNIROUTE")
                    .separator("_")
                    .list_separator(","),
            )
            .set_default("port", 20128)?
            .set_default("host", "0.0.0.0")?
            .set_default("db_path", "./data/omniroute.db")?
            .set_default("data_dir", "./data")?
            .set_default("allowed_hosts", "localhost,127.0.0.1")?
            .build()?
            .try_deserialize()
    }
}

/// Gateway API keys from `OMNIROUTE_API_KEYS` (comma-separated).
/// Empty → auth disabled (dev mode).
pub fn gateway_keys_from_env() -> Vec<String> {
    std::env::var("OMNIROUTE_API_KEYS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Allowed hosts from `OMNIROUTE_ALLOWED_HOSTS` (comma-separated).
/// Empty → host guard disabled (dev mode).
pub fn allowed_hosts_from_env() -> Vec<String> {
    std::env::var("OMNIROUTE_ALLOWED_HOSTS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Upstream base URL overrides from `OMNIROUTE_BASE_URL_<PROVIDER>`
/// (e.g. OMNIROUTE_BASE_URL_OPENAI=http://127.0.0.1:9099).
/// Used for tests and self-hosted mirrors.
pub fn base_urls_from_env() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let prefix = "OMNIROUTE_BASE_URL_";
    for (k, v) in std::env::vars() {
        if let Some(rest) = k.strip_prefix(prefix) {
            let provider = rest.to_lowercase();
            if !v.trim().is_empty() {
                map.insert(provider, v.trim().to_string());
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_keys_env() {
        unsafe { std::env::set_var("OMNIROUTE_API_KEYS", "sk-a, sk-b ,") };
        let keys = gateway_keys_from_env();
        assert_eq!(keys, vec!["sk-a", "sk-b"]);
        unsafe { std::env::remove_var("OMNIROUTE_API_KEYS") };
    }

    #[test]
    fn test_gateway_keys_env_empty() {
        unsafe { std::env::remove_var("OMNIROUTE_API_KEYS") };
        assert!(gateway_keys_from_env().is_empty());
    }

    #[test]
    fn test_allowed_hosts_env() {
        unsafe { std::env::set_var("OMNIROUTE_ALLOWED_HOSTS", "localhost,127.0.0.1") };
        let hosts = allowed_hosts_from_env();
        assert_eq!(hosts, vec!["localhost", "127.0.0.1"]);
        unsafe { std::env::remove_var("OMNIROUTE_ALLOWED_HOSTS") };
    }
}

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

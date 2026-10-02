use std::env;
use std::str::FromStr;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub verbose: bool,
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid { key: &'static str, value: String },
}

impl FromStr for Config {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config = Config::default();
        for line in s.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (key, value) = line.split_once('=').ok_or(ConfigError::Missing("="))?;
            match key.trim() {
                "host" => config.host = value.trim().to_owned(),
                "port" => {
                    config.port = value.trim().parse().map_err(|_| ConfigError::Invalid {
                        key: "port",
                        value: value.to_owned(),
                    })?
                }
                "verbose" => config.verbose = matches!(value.trim(), "1" | "true" | "yes"),
                _ => {}
            }
        }
        Ok(config)
    }
}

pub fn from_env() -> Result<Config, ConfigError> {
    let host = env::var("APP_HOST").map_err(|_| ConfigError::Missing("APP_HOST"))?;
    let port = env::var("APP_PORT").unwrap_or_else(|_| "8080".into());
    format!("host={host}\nport={port}").parse()
}

use serde::Deserialize;
use std::{collections::HashMap, fs};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub switch: Switch,
    pub profiles: HashMap<String, Profile>,
}

#[derive(Debug, Deserialize)]
pub struct Switch {
    pub default: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "scheme", rename_all = "lowercase")]
pub enum Profile {
    Direct,
    Socks5 { host: String, port: u16 },
    Http { host: String, port: u16 },
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub pattern: String,
    pub profile: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read configuration file '{}': {}", path, e))?;

        serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse configuration file '{}': {}", path, e))
    }
}

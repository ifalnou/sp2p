use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub peers: Vec<String>,
    #[serde(default)]
    pub password: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peers: Vec::new(),
            password: Some("sp2p-default-net".to_string()),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        if !path.exists() {
            let default_config = Self::default();
            let toml = toml::to_string_pretty(&default_config).unwrap();
            fs::write(path, toml)?;
            info!("Created default config at {:?}", path);
            return Ok(default_config);
        }

        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }
}

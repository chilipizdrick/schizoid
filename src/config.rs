use clap::Parser;
use serde::{Deserialize, Serialize};
use serenity::prelude::TypeMapKey;

use crate::{args::Args, storage::Storage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub file_storage_path: String,

    pub max_attachment_size: u64, // in bytes
    pub max_attachment_count: u64,
    pub minecraft_server_ping: MinecraftServerPingConfig,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let args = Args::try_parse()?;
        let config_string = std::fs::read_to_string(args.config_path)?;
        let config = toml::from_str(&config_string)?;
        Ok(config)
    }

    pub fn storage<'a>(&'a self) -> Storage<'a> {
        Storage::new(&self.file_storage_path)
    }
}

/// # Configuration for the trusted users and guilds
///
/// These users are privileged to use commands, which upload attachments to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedConfig {
    /// User IDs that are trusted as admins
    pub users: Vec<u64>,
    /// Guild IDs, in which all commands are treated as privileged
    pub guilds: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftServerPingConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    // pub server_address: String,
    #[serde(default = "default_ping_timeout_ms")]
    pub ping_timeout_ms: u64,
    #[serde(default = "default_ping_interval_ms")]
    pub ping_interval_ms: u64,
}

fn default_enabled() -> bool {
    false
}

fn default_ping_timeout_ms() -> u64 {
    5_000
}

fn default_ping_interval_ms() -> u64 {
    10_000
}

pub struct ConfigKey;

impl TypeMapKey for ConfigKey {
    type Value = &'static Config;
}

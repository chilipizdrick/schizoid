pub mod args;
pub mod commands;
pub mod config;
pub mod database;
pub mod greeting_birthday_handler;
pub mod minecraft_server_handler;
pub mod storage;

use std::env;

use anyhow::anyhow;
use serenity::all::Context;

use crate::config::{Config, ConfigKey};

pub type PContext<'a> = poise::Context<'a, (), anyhow::Error>;

pub fn environment_variable_not_set_error(name: &str) -> anyhow::Error {
    anyhow!("{} environment variable not set", name)
}

pub fn load_env_var(name: &str) -> anyhow::Result<String> {
    env::var(name).map_err(|_| environment_variable_not_set_error(name))
}

async fn get_config(ctx: &Context) -> &'static Config {
    ctx.data.read().await.get::<ConfigKey>().unwrap()
}

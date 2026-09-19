pub mod args;
pub mod commands;
pub mod config;
pub mod database;
pub mod greeting_birthday_handler;
pub mod minecraft_server_handler;

use std::{env, io, path::PathBuf};

use anyhow::anyhow;
use serenity::all::{Context, GuildId, UserId};

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

fn choose_guild_speciffic_file_with_fallback(
    file_storage_path: &str,
    subdir_name: &str,
    fallback_file_name: &str,
    guild_id: GuildId,
) -> anyhow::Result<PathBuf> {
    let subdir_path = PathBuf::from(file_storage_path).join(subdir_name);
    let guild_speciffic_path = subdir_path.join(guild_id.to_string());

    let path = match std::fs::read_dir(&guild_speciffic_path) {
        Ok(mut read_dir) => read_dir
            .next()
            .ok_or_else(|| anyhow!("No files in guild directory"))??
            .path(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => subdir_path.join(fallback_file_name),
        Err(err) => return Err(err.into()),
    };

    Ok(path)
}

fn choose_member_speciffic_file_with_fallback(
    file_storage_path: &str,
    subdir_name: &str,
    fallback_file_name: &str,
    guild_id: GuildId,
    user_id: UserId,
) -> anyhow::Result<PathBuf> {
    let subdir_path = PathBuf::from(file_storage_path).join(subdir_name);
    let member_speciffic_path = subdir_path
        .join(guild_id.to_string())
        .join(user_id.to_string());

    let path = match std::fs::read_dir(&member_speciffic_path) {
        Ok(mut read_dir) => read_dir
            .next()
            .ok_or_else(|| anyhow!("No files in guild directory"))??
            .path(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => subdir_path.join(fallback_file_name),
        Err(err) => return Err(err.into()),
    };

    Ok(path)
}

/// Returns user's defined greating on this server, if it exists, or guild greeting file path (even if it does not exist)
fn greeting_filepath(
    file_storage_path: &str,
    guild_id: GuildId,
    user_id: UserId,
) -> anyhow::Result<PathBuf> {
    let member_greeting = member_greering_filepath(file_storage_path, guild_id, user_id)?;
    if member_greeting.exists() {
        return Ok(member_greeting);
    }
    guild_greeting_filepath(file_storage_path, guild_id)
}

fn member_greering_filepath(
    file_storage_path: &str,
    guild_id: GuildId,
    user_id: UserId,
) -> anyhow::Result<PathBuf> {
    choose_member_speciffic_file_with_fallback(
        file_storage_path,
        "greetings",
        "default.ogg",
        guild_id,
        user_id,
    )
}

fn guild_greeting_filepath(file_storage_path: &str, guild_id: GuildId) -> anyhow::Result<PathBuf> {
    choose_guild_speciffic_file_with_fallback(
        file_storage_path,
        "greetings",
        "default.ogg",
        guild_id,
    )
}

fn guild_birthday_congratulation_filepath(
    file_storage_path: &str,
    guild_id: GuildId,
) -> anyhow::Result<PathBuf> {
    choose_guild_speciffic_file_with_fallback(
        file_storage_path,
        "birthday_congratulations",
        "default.ogg",
        guild_id,
    )
}

fn guild_speciffic_dir_path(file_storage_path: &str, subdir: &str, guild_id: GuildId) -> PathBuf {
    [file_storage_path, subdir, &guild_id.to_string()]
        .iter()
        .collect()
}

fn member_speciffic_dir_path(
    file_storage_path: &str,
    subdir: &str,
    guild_id: GuildId,
    user_id: UserId,
) -> PathBuf {
    [
        file_storage_path,
        subdir,
        &guild_id.to_string(),
        &user_id.to_string(),
    ]
    .iter()
    .collect()
}

#[allow(unused)]
fn greeting_dir_path(file_storage_path: &str) -> PathBuf {
    [file_storage_path, "greetings"].iter().collect()
}

#[allow(unused)]
fn guild_greeting_dir_path(file_storage_path: &str, guild_id: GuildId) -> PathBuf {
    guild_speciffic_dir_path(file_storage_path, "greetings", guild_id)
}

#[allow(unused)]
fn birthday_congratulation_dir_path(file_storage_path: &str) -> PathBuf {
    [file_storage_path, "birthday_congratulations"]
        .iter()
        .collect()
}

#[allow(unused)]
fn guild_birthday_congratulation_dir_path(file_storage_path: &str, guild_id: GuildId) -> PathBuf {
    guild_speciffic_dir_path(file_storage_path, "birthday_congratulations", guild_id)
}

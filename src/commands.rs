use std::path::{Path, PathBuf};

use anyhow::anyhow;
use chrono_tz::Tz;
use poise::command;
use serenity::all::{Attachment, ChannelId, Color, EditRole, GuildId, Member, UserId};
use sqlx::query;
use tokio::fs;

use crate::{
    PContext,
    config::Config,
    database::{DBConnKey, MonthDayDate},
    guild_birthday_congratulation_filepath, guild_greeting_filepath, guild_speciffic_dir_path,
    member_speciffic_dir_path,
};

#[command(slash_command)]
pub async fn ping(ctx: PContext<'_>) -> anyhow::Result<()> {
    ctx.reply("Pong!").await?;
    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn greet(ctx: PContext<'_>, voice_channel: Option<ChannelId>) -> anyhow::Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let file_storage_path = &get_config(ctx).await.file_storage_path;
    let filepath = guild_greeting_filepath(file_storage_path, guild_id)?;

    play_only_audio_optionally_in_voice_channel(ctx, filepath, voice_channel).await?;

    ctx.reply("Greeting...").await?;

    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn congratulate(
    ctx: PContext<'_>,
    voice_channel: Option<ChannelId>,
) -> anyhow::Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let file_storage_path = &get_config(ctx).await.file_storage_path;
    let filepath = guild_birthday_congratulation_filepath(file_storage_path, guild_id)?;

    play_only_audio_optionally_in_voice_channel(ctx, filepath, voice_channel).await?;

    ctx.reply("Congratulating...").await?;

    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn toggle_server_greeting(ctx: PContext<'_>) -> anyhow::Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let guild = db_conn.get_or_insert_guild(guild_id).await?;

    let guild_id_str = guild_id.to_string();
    let new_greeting_state = !guild.greeting_enabled;
    query!(
        "UPDATE guilds SET greeting_enabled = ? WHERE id = ?",
        new_greeting_state,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    let reply = match new_greeting_state {
        true => "Server greeting enabled.",
        false => "Server greeting disabled.",
    };
    ctx.reply(reply).await?;

    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn set_greeting_interval(
    ctx: PContext<'_>,
    new_interval_secs: u32,
) -> anyhow::Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let guild_id_str = guild_id.to_string();
    query!(
        "UPDATE guilds SET greeting_interval = ? WHERE id = ?",
        new_interval_secs,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    ctx.reply(format!(
        "Greeting interval set to {new_interval_secs} seconds."
    ))
    .await?;

    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn set_birthday(ctx: PContext<'_>, date: String) -> anyhow::Result<()> {
    let user_id = ctx.author().id;
    set_birthday_for_user(ctx, user_id, &date).await?;
    ctx.reply(format!("Set your birthday to be {date}."))
        .await?;
    Ok(())
}

#[command(slash_command, guild_only, owners_only, ephemeral)]
pub async fn set_user_birthday(
    ctx: PContext<'_>,
    member: Member,
    date: String,
) -> anyhow::Result<()> {
    let user_id = member.user.id;
    set_birthday_for_user(ctx, user_id, &date).await?;
    ctx.reply(format!("Set <@{user_id}>'s birthday to be {date}."))
        .await?;
    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn unset_birthday(ctx: PContext<'_>, member: Member) -> anyhow::Result<()> {
    unset_birthday_for_user(ctx, member.user.id).await?;
    ctx.reply("Your birthday was unset.").await?;
    Ok(())
}

#[command(slash_command, guild_only, owners_only, ephemeral)]
pub async fn unset_user_birthday(ctx: PContext<'_>, member: Member) -> anyhow::Result<()> {
    let user_id = member.user.id;
    unset_birthday_for_user(ctx, user_id).await?;
    ctx.reply("Unset birthday for <@user_id>.").await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn set_greeting(ctx: PContext<'_>, attachment: Attachment) -> anyhow::Result<()> {
    let content = attachment.download().await?;
    let filename = attachment.filename;
    set_guild_greeting(ctx, &filename, content).await?;
    ctx.reply(format!("Set greeting to {filename}.")).await?;
    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn set_my_greeting(ctx: PContext<'_>, attachment: Attachment) -> anyhow::Result<()> {
    let content = attachment.download().await?;
    let filename = attachment.filename;
    // set_guild_greeting(ctx, &filename, content).await?;
    set_member_greeting(ctx, &filename, content).await?;
    ctx.reply(format!("Set your greeting to {filename}."))
        .await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn set_birthday_congratulation(
    ctx: PContext<'_>,
    attachment: Attachment,
) -> anyhow::Result<()> {
    let content = attachment.download().await?;
    let filename = attachment.filename;
    set_guild_birthday_congratulation(ctx, &filename, content).await?;
    ctx.reply(format!("Set greeting to {filename}.")).await?;
    Ok(())
}

#[command(slash_command, guild_only, ephemeral)]
pub async fn color(ctx: PContext<'_>, color: String) -> anyhow::Result<()> {
    let author = ctx.author_member().await.unwrap();
    let user_id_str = author.user.id.to_string();
    let guild_id = ctx.guild_id().unwrap();

    let hex_color_value =
        parse_hex_color(&color).map_err(|err| anyhow!("Could not parse color string: {err}"))?;
    let color = Color::new(hex_color_value);

    let guild_roles = guild_id.roles(ctx.http()).await?;
    let maybe_role = guild_roles
        .values()
        .find(|role| role.name == user_id_str)
        .cloned();

    match maybe_role {
        Some(mut role) => {
            log::debug!("Found role: {:?}", role);
            role.edit(ctx.http(), EditRole::new().colour(color)).await?;
            log::debug!("Edited role: {:?}, set color to #{}", role, color.hex());
        }
        None => {
            let builder = EditRole::new()
                .name(&user_id_str)
                .colour(color)
                .mentionable(false);
            let guild_id = ctx.guild_id().unwrap();
            log::debug!("Creating role: {:?}", builder);
            let role = guild_id.create_role(ctx.http(), builder).await?;
            author.add_role(ctx.http(), role.id).await?;
            log::debug!("Assigning role to user");
        }
    }

    ctx.reply(format!(
        "Set <@{user_id_str}> role color to #{}.",
        color.hex()
    ))
    .await?;

    Ok(())
}

#[command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn set_timezone(ctx: PContext<'_>, timezone: String) -> anyhow::Result<()> {
    let timezone: Tz = timezone.parse()?;

    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let guild_id_str = guild_id.to_string();
    let timezone_str = timezone.to_string();
    query!(
        "UPDATE guilds SET timezone = ? WHERE id = ?",
        timezone_str,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    ctx.reply(format!("Set timezone to {timezone}.")).await?;

    Ok(())
}

fn parse_hex_color(color: &str) -> anyhow::Result<u32> {
    let color = color.strip_prefix('#').unwrap_or(color);
    let color = color.strip_prefix("0x").unwrap_or(color);
    let color = color.strip_prefix("0X").unwrap_or(color);

    Ok(u32::from_str_radix(color, 16)?)
}

async fn set_guild_greeting(
    ctx: PContext<'_>,
    filename: &str,
    content: Vec<u8>,
) -> anyhow::Result<()> {
    set_guild_audio_file(ctx, "greetings", filename, content).await
}

async fn set_member_greeting(
    ctx: PContext<'_>,
    filename: &str,
    content: Vec<u8>,
) -> anyhow::Result<()> {
    set_member_audio_file(ctx, "greetings", filename, content).await
}

async fn set_guild_birthday_congratulation(
    ctx: PContext<'_>,
    filename: &str,
    content: Vec<u8>,
) -> anyhow::Result<()> {
    set_guild_audio_file(ctx, "birthday_congratulations", filename, content).await
}

async fn set_guild_audio_file(
    ctx: PContext<'_>,
    subdir: &str,
    filename: &str,
    contents: Vec<u8>,
) -> anyhow::Result<()> {
    let config = get_config(ctx).await;
    let storage_path = &config.file_storage_path;
    let guild_id = ctx.guild_id().unwrap();
    let dir_path = guild_speciffic_dir_path(storage_path, subdir, guild_id);

    create_dir_at_path_and_write_file_there(dir_path, filename, contents).await
}

async fn set_member_audio_file(
    ctx: PContext<'_>,
    subdir: &str,
    filename: &str,
    contents: Vec<u8>,
) -> anyhow::Result<()> {
    let config = get_config(ctx).await;
    let storage_path = &config.file_storage_path;
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;
    let dir_path = member_speciffic_dir_path(storage_path, subdir, guild_id, user_id);

    create_dir_at_path_and_write_file_there(dir_path, filename, contents).await
}

async fn create_dir_at_path_and_write_file_there(
    dir_path: PathBuf,
    filename: &str,
    contents: Vec<u8>,
) -> anyhow::Result<()> {
    // This is done to ensure that the directory is empty before writing to it
    if exists(&dir_path).await {
        fs::remove_dir_all(&dir_path).await?;
    }

    fs::create_dir_all(&dir_path).await?;

    let filepath = dir_path.join(filename);
    fs::write(&filepath, contents).await?;

    Ok(())
}

async fn exists(path: impl AsRef<Path>) -> bool {
    fs::metadata(path).await.is_ok()
}

async fn set_birthday_for_user(
    ctx: PContext<'_>,
    user_id: UserId,
    date: &str,
) -> anyhow::Result<()> {
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();
    let MonthDayDate { month, day } = date.parse()?;
    let user_id_str = user_id.to_string();
    query!(
        "UPDATE users SET birthday_month = ?, birthday_day = ? WHERE id = ?",
        month,
        day,
        user_id_str,
    )
    .execute(&db_conn.pool)
    .await?;
    Ok(())
}

async fn unset_birthday_for_user(ctx: PContext<'_>, user_id: UserId) -> anyhow::Result<()> {
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();
    let user_id_str = user_id.to_string();
    query!(
        "UPDATE users SET birthday_month = NULL, birthday_day = NULL WHERE id = ?",
        user_id_str,
    )
    .execute(&db_conn.pool)
    .await?;
    Ok(())
}

/// Plays audio in user's voice channel, if he is connected to one
async fn play_only_audio_optionally_in_voice_channel(
    ctx: PContext<'_>,
    audio_file_path: PathBuf,
    voice_channel_id: Option<ChannelId>,
) -> anyhow::Result<()> {
    let (guild_id, voice_channel_id) = {
        let guild = ctx.guild().unwrap();

        let channel_id = voice_channel_id
            .or_else(|| {
                let author = ctx.author();
                let voice_state = guild.voice_states.get(&author.id);
                voice_state.and_then(|vs| vs.channel_id)
            })
            .ok_or_else(|| {
                anyhow!("Make sure you are in a voice channel or provide a voice channel.")
            })?;

        (guild.id, channel_id)
    };

    play_only_audio_in_voice_channel(ctx, guild_id, voice_channel_id, audio_file_path).await?;

    Ok(())
}

async fn play_only_audio_in_voice_channel(
    ctx: PContext<'_>,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    audio_file_path: PathBuf,
) -> anyhow::Result<()> {
    let input = songbird::input::File::new(audio_file_path);
    let client = songbird::get(ctx.serenity_context()).await.unwrap().clone();
    let call = client.join(guild_id, voice_channel_id).await?;
    let mut call_lock = call.lock().await;
    let _ = call_lock.play_only_input(input.into());

    Ok(())
}

async fn get_config(ctx: PContext<'_>) -> &'static Config {
    super::get_config(ctx.serenity_context()).await
}

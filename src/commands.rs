use std::num::ParseIntError;

use anyhow::{Result, anyhow};
use chrono_tz::Tz;
use poise::{CreateReply, command};
use serenity::{
    all::{Attachment, ChannelId, Color, EditRole, GuildId, UserId},
    builder::CreateAttachment,
    model::user::User,
};
use songbird::{CoreEvent, input::Input};
use sqlx::query;

use crate::{
    PContext,
    config::Config,
    database::{DBConnKey, MonthDayDate},
    storage::{self, RemovalResult},
    voice_clip_recorder::{VCRStateKey, VoiceClipRecorder},
};

/// Respond with "Pong!"
#[command(slash_command)]
pub async fn ping(ctx: PContext<'_>) -> Result<()> {
    ctx.reply("Pong!").await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands("greeting_play", "greeting_toggle", "greeting_set", "greeting_remove")
)]
pub async fn greeting(_: PContext<'_>) -> Result<()> {
    Ok(())
}

/// Play your greeting
#[command(slash_command, guild_only, ephemeral, rename = "play")]
pub async fn greeting_play(
    ctx: PContext<'_>,
    #[description = "Voice channel to join"] voice_channel: Option<ChannelId>,
) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;
    let storage = get_config(ctx).await.storage();
    let input = storage.read_member_greeting(guild_id, user_id).await?;
    play_only_audio_optionally_in_voice_channel(ctx, input, voice_channel).await?;

    ctx.reply("Greetings!").await?;

    Ok(())
}

/// Toggle your greetings
#[command(slash_command, guild_only, ephemeral, rename = "toggle")]
pub async fn greeting_toggle(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let member = db_conn.get_or_insert_member(user_id, guild_id).await?;

    let user_id_str = member.user_id.to_string();
    let guild_id_str = member.guild_id.to_string();
    let new_greeting_state = !member.greeting_enabled;
    query!(
        "UPDATE members SET greeting_enabled = ? WHERE guild_id = ? and user_id = ?",
        new_greeting_state,
        guild_id_str,
        user_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    let reply = match new_greeting_state {
        true => "Greetings for you are enabled.",
        false => "Greetings for you are disabled.",
    };
    ctx.reply(reply).await?;

    Ok(())
}

/// Set your greeting
#[command(slash_command, guild_only, ephemeral, rename = "set")]
pub async fn greeting_set(ctx: PContext<'_>, attachment: Attachment) -> Result<()> {
    check_attachment_size(ctx, &attachment).await?;

    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;
    let storage = get_config(ctx).await.storage();

    let file = storage::File::from_attachment(&attachment).await?;
    storage
        .save_member_greeting(guild_id, user_id, file)
        .await?;

    ctx.reply(format!("Set your greeting to {}.", attachment.filename))
        .await?;
    Ok(())
}

/// Remove your greeting
#[command(slash_command, guild_only, ephemeral, rename = "remove")]
pub async fn greeting_remove(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;
    let storage = get_config(ctx).await.storage();

    let result = storage.delete_member_greeting(guild_id, user_id).await?;

    let reply = match result {
        RemovalResult::Existed => "Your greeting was removed.",
        RemovalResult::DidNotExist => "Your greeting did not exist.",
    };

    ctx.reply(reply).await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands("surveillance_start", "surveillance_stop"),
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn surveillance(_: PContext<'_>) -> Result<()> {
    Ok(())
}

/// Start recording up to the last minute of users' audio in voice channel
#[command(slash_command, guild_only, rename = "start")]
pub async fn surveillance_start(
    ctx: PContext<'_>,
    #[description = "Voice channel to join"] voice_channel: Option<ChannelId>,
) -> Result<()> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().unwrap();
    let channel_id = voice_channel
        .or_else(|| {
            ctx.guild()
                .unwrap()
                .voice_states
                .get(&ctx.author().id)
                .and_then(|vs| vs.channel_id)
        })
        .ok_or_else(|| {
            anyhow!("Make sure you are in a voice channel or provide a voice channel.")
        })?;

    let songbird = songbird::get(ctx.serenity_context()).await.unwrap();
    let handler_lock = songbird.join(guild_id, channel_id).await?;
    let mut handler = handler_lock.lock().await;

    let data = ctx.serenity_context().data.read().await;
    let vcr_state = data.get::<VCRStateKey>().unwrap();
    let vcr = VoiceClipRecorder::with_state(vcr_state.clone());

    handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), vcr.clone());
    handler.add_global_event(CoreEvent::ClientDisconnect.into(), vcr.clone());
    handler.add_global_event(CoreEvent::VoiceTick.into(), vcr);

    ctx.reply("Joined voice channel and started surveillance!")
        .await?;

    Ok(())
}

/// Stop recording up to the last minute of users' audio in voice channel
#[command(slash_command, guild_only, rename = "stop")]
pub async fn surveillance_stop(ctx: PContext<'_>) -> Result<()> {
    let songbird = songbird::get(ctx.serenity_context()).await.unwrap();
    let guild_id = ctx.guild_id().unwrap();
    songbird.remove(guild_id).await?;
    ctx.reply("Surveillance is no more!").await?;
    Ok(())
}

/// Output up to the last minute of user's audio
#[command(slash_command, guild_only)]
pub async fn clip(ctx: PContext<'_>, #[description = "User to clip"] user: User) -> Result<()> {
    ctx.defer().await?;

    let data = ctx.serenity_context().data.read().await;
    let vcr_state = data.get::<VCRStateKey>().unwrap();
    let user_id = user.id;

    let ogg_bytes = {
        let buffers = vcr_state.buffers.read();
        buffers
            .get(&user_id)
            .ok_or_else(|| anyhow!("No recorded audio found for <@{}>!", user_id))?
            .to_ogg_bytes()?
    };

    // let clip_duration_secs = (ogg_bytes.len() as f64 / SAMPLE_RATE as f64).ceil() as u32;
    // let time_str = match clip_duration_secs {
    //     60 => "1 minute".to_string(),
    //     secs => format!("{} seconds", secs),
    // };

    // let wav_bytes = tokio::task::spawn_blocking(move || pcm_to_wav_bytes(&ogg_bytes)).await??;

    let attachment =
        CreateAttachment::bytes(ogg_bytes, format!("{}_clip.ogg", user.display_name()));

    let reply = CreateReply::default()
        .content(format!(
            "Up to the last minute of audio recorded from <@{user_id}>.",
        ))
        .attachment(attachment);
    ctx.send(reply).await?;

    Ok(())
}

/// Make bot leave voice chanel
#[command(slash_command, guild_only, ephemeral)]
pub async fn leave(ctx: PContext<'_>) -> Result<()> {
    let songbird = songbird::get(ctx.serenity_context()).await.unwrap();
    let guild_id = ctx.guild_id().unwrap();
    songbird.leave(guild_id).await?;
    ctx.reply("Left voice channel.").await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands("guild_greeting", "guild_congratulation", "guild_timezone"),
    rename = "server",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn guild(_: PContext<'_>) -> Result<()> {
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommands(
        "guild_greeting_play",
        "guild_greeting_toggle",
        "guild_greeting_set",
        "guild_greeting_remove",
        "guild_greeting_set_interval"
    ),
    rename = "greeting"
)]
pub async fn guild_greeting(_: PContext<'_>) -> Result<()> {
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands(
        "guild_congratulation_play",
        "guild_congratulation_toggle",
        "guild_congratulation_set",
        "guild_congratulation_remove"
    ),
    rename = "congratulation"
)]
pub async fn guild_congratulation(_: PContext<'_>) -> Result<()> {
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands("guild_timezone_set", "guild_timezone_remove")
)]
pub async fn guild_timezone(_: PContext<'_>) -> Result<()> {
    Ok(())
}

/// Play server greeting
///
/// It is the default for users, who did not set their own greetings
#[command(slash_command, guild_only, rename = "play")]
pub async fn guild_greeting_play(
    ctx: PContext<'_>,
    #[description = "Voice channel to join"] voice_channel: Option<ChannelId>,
) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let storage = get_config(ctx).await.storage();
    let input = storage.read_guild_greeting(guild_id).await?;
    play_only_audio_optionally_in_voice_channel(ctx, input, voice_channel).await?;

    ctx.reply("Greetings!").await?;

    Ok(())
}

/// Toggle server greetings
///
/// This setting also controlls users' configured greetings, even if they were enabled
#[command(slash_command, guild_only, rename = "toggle")]
pub async fn guild_greeting_toggle(ctx: PContext<'_>) -> Result<()> {
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

/// Set default server greeting
#[command(slash_command, guild_only, rename = "set")]
pub async fn guild_greeting_set(ctx: PContext<'_>, attachment: Attachment) -> Result<()> {
    check_attachment_size(ctx, &attachment).await?;

    let guild_id = ctx.guild_id().unwrap();
    let storage = get_config(ctx).await.storage();

    let file = storage::File::from_attachment(&attachment).await?;
    storage.save_guild_greeting(guild_id, file).await?;

    ctx.reply(format!("Set server greeting to {}.", attachment.filename))
        .await?;
    Ok(())
}

/// Remove default server greeting
#[command(slash_command, guild_only, rename = "remove")]
pub async fn guild_greeting_remove(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let storage = get_config(ctx).await.storage();

    let result = storage.delete_guild_greeting(guild_id).await?;

    let reply = match result {
        RemovalResult::Existed => "Server greeting was removed.",
        RemovalResult::DidNotExist => "Server greeting did not exist.",
    };

    ctx.reply(reply).await?;
    Ok(())
}

/// Set interval in seconds for greetings to be played on users joining voice channels on the server
#[command(slash_command, guild_only, rename = "set-interval")]
pub async fn guild_greeting_set_interval(ctx: PContext<'_>, new_interval_secs: u32) -> Result<()> {
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

/// Play server congratulations
#[command(slash_command, guild_only, rename = "play")]
pub async fn guild_congratulation_play(
    ctx: PContext<'_>,
    voice_channel: Option<ChannelId>,
) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let storage = get_config(ctx).await.storage();
    let input = storage.read_guild_congratulation(guild_id).await?;
    play_only_audio_optionally_in_voice_channel(ctx, input, voice_channel).await?;

    ctx.reply("Congratulations!").await?;

    Ok(())
}

/// Toggle server congratulations
#[command(slash_command, guild_only, rename = "toggle")]
pub async fn guild_congratulation_toggle(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let guild = db_conn.get_or_insert_guild(guild_id).await?;

    let guild_id_str = guild_id.to_string();
    let new_congratulation_state = !guild.greeting_enabled;
    query!(
        "UPDATE guilds SET birthday_enabled = ? WHERE id = ?",
        new_congratulation_state,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    let reply = match new_congratulation_state {
        true => "Server birthday congratulations enabled.",
        false => "Server birthday congratulations disabled.",
    };
    ctx.reply(reply).await?;

    Ok(())
}

/// Set server congratulations
#[command(slash_command, guild_only, rename = "set")]
pub async fn guild_congratulation_set(ctx: PContext<'_>, attachment: Attachment) -> Result<()> {
    check_attachment_size(ctx, &attachment).await?;

    let guild_id = ctx.guild_id().unwrap();

    let file = storage::File::from_attachment(&attachment).await?;
    let storage = get_config(ctx).await.storage();

    storage.save_guild_congratulation(guild_id, file).await?;

    ctx.reply(format!(
        "Set server birthday congratulation to {}.",
        attachment.filename
    ))
    .await?;
    Ok(())
}

/// Remove server congratulations
#[command(slash_command, guild_only, rename = "remove")]
pub async fn guild_congratulation_remove(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let storage = get_config(ctx).await.storage();

    let result = storage.delete_guild_congratulation(guild_id).await?;

    let reply = match result {
        RemovalResult::Existed => "Server birthday congratulation was removed.",
        RemovalResult::DidNotExist => "Server birthday congratulation did not exist.",
    };

    ctx.reply(reply).await?;
    Ok(())
}

#[command(slash_command, guild_only, rename = "set")]
pub async fn guild_timezone_set(ctx: PContext<'_>, timezone: String) -> Result<()> {
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

    ctx.reply(format!("Set server timezone to {timezone}."))
        .await?;

    Ok(())
}

#[command(slash_command, guild_only, rename = "remove")]
pub async fn guild_timezone_remove(ctx: PContext<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.serenity_context().data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let guild_id_str = guild_id.to_string();
    query!(
        "UPDATE guilds SET timezone = null WHERE id = ?",
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    ctx.reply(format!("Unset server's timezone.")).await?;

    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands(
        "birthday_set",
        "birthday_remove",
        "birthday_set_for_user",
        "birthday_remove_for_user"
    )
)]
pub async fn birthday(_: PContext<'_>) -> Result<()> {
    Ok(())
}

/// Set your own birthday date in MM/DD format
#[command(slash_command, guild_only, ephemeral, rename = "set")]
pub async fn birthday_set(ctx: PContext<'_>, date: String) -> Result<()> {
    let user_id = ctx.author().id;
    set_birthday_for_user(ctx, user_id, &date).await?;
    ctx.reply(format!("Set your birthday to be {date}."))
        .await?;
    Ok(())
}

/// Remove (unset) info about your birthday
#[command(slash_command, guild_only, ephemeral, rename = "remove")]
pub async fn birthday_remove(ctx: PContext<'_>) -> Result<()> {
    unset_birthday_for_user(ctx, ctx.author().id).await?;
    ctx.reply("Your birthday was unset.").await?;
    Ok(())
}

/// Set user's birthday date in MM/DD format
#[command(
    slash_command,
    guild_only,
    owners_only,
    ephemeral,
    rename = "set-for-user"
)]
pub async fn birthday_set_for_user(ctx: PContext<'_>, user: User, date: String) -> Result<()> {
    let user_id = user.id;
    set_birthday_for_user(ctx, user_id, &date).await?;
    ctx.reply(format!("Set <@{user_id}>'s birthday to be {date}."))
        .await?;
    Ok(())
}

/// Remove info about user's birthday
#[command(
    slash_command,
    guild_only,
    owners_only,
    ephemeral,
    rename = "remove-for-user"
)]
pub async fn birthday_remove_for_user(ctx: PContext<'_>, user: User) -> Result<()> {
    let user_id = user.id;
    unset_birthday_for_user(ctx, user_id).await?;
    ctx.reply("Unset birthday for <@user_id>.").await?;
    Ok(())
}

#[command(
    slash_command,
    guild_only,
    subcommand_required,
    subcommands("color_set", "color_remove")
)]
pub async fn color(_: PContext<'_>) -> Result<()> {
    Ok(())
}

/// Set your personal role color
#[command(slash_command, guild_only, ephemeral, rename = "set")]
pub async fn color_set(ctx: PContext<'_>, color: String) -> Result<()> {
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

/// Remove your personal role color
#[command(slash_command, guild_only, ephemeral, rename = "remove")]
pub async fn color_remove(ctx: PContext<'_>) -> Result<()> {
    let author = ctx.author_member().await.unwrap();
    let user_id_str = author.user.id.to_string();
    let guild_id = ctx.guild_id().unwrap();

    let mut guild_roles = guild_id.roles(ctx.http()).await?;
    let maybe_role = guild_roles
        .values_mut()
        .find(|role| role.name == user_id_str);

    if let Some(role) = maybe_role {
        role.delete(ctx.http()).await?;
        ctx.reply("Your role has been deleted.").await?;
    } else {
        ctx.reply("Your role was not found").await?;
    }

    Ok(())
}

fn parse_hex_color(color: &str) -> Result<u32, ParseIntError> {
    let color = color.strip_prefix('#').unwrap_or(color);
    let color = color.strip_prefix("0x").unwrap_or(color);
    let color = color.strip_prefix("0X").unwrap_or(color);

    Ok(u32::from_str_radix(color, 16)?)
}

async fn set_birthday_for_user(ctx: PContext<'_>, user_id: UserId, date: &str) -> Result<()> {
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

async fn unset_birthday_for_user(ctx: PContext<'_>, user_id: UserId) -> Result<()> {
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

/// Plays audio in a provided voice channel, or in user's voice channel, if he is connected to one
async fn play_only_audio_optionally_in_voice_channel(
    ctx: PContext<'_>,
    input: Input,
    voice_channel_id: Option<ChannelId>,
) -> Result<()> {
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

    play_only_audio_in_voice_channel(ctx, guild_id, voice_channel_id, input).await?;

    Ok(())
}

async fn play_only_audio_in_voice_channel(
    ctx: PContext<'_>,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    input: Input,
) -> Result<()> {
    let client = songbird::get(ctx.serenity_context()).await.unwrap().clone();
    let call = client.join(guild_id, voice_channel_id).await?;
    let mut call_lock = call.lock().await;
    let _ = call_lock.play_only_input(input);

    Ok(())
}

async fn check_attachment_size(ctx: PContext<'_>, attachment: &Attachment) -> Result<()> {
    let config = get_config(ctx).await;
    if attachment.size as u64 > config.max_attachment_size {
        return Err(anyhow!(
            "The attachment size is bigger than configured maximum!"
        ));
    }
    Ok(())
}

async fn get_config(ctx: PContext<'_>) -> &'static Config {
    super::get_config(ctx.serenity_context()).await
}

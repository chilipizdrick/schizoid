use chrono::{Datelike, Utc};
use serenity::all::{Context, EventHandler, GuildId, UserId, VoiceState};
use sqlx::query;

use crate::{
    database::{DBConnKey, DatabaseConnection, Guild, Member, MonthDayDate, User},
    get_config, guild_birthday_congratulation_filepath, guild_greeting_filepath,
};

/// Hander for greeting and congratulating on birthdays
pub struct GreetingBirthdayHandler;

// TODO: Maybe add a text message congratulation, which would check for birthdays each hour or so (in ready event)
#[serenity::async_trait]
impl EventHandler for GreetingBirthdayHandler {
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        if let Err(err) = voice_state_update_fallible(ctx, old, new).await {
            log::error!("Error running greeting handler: {err}");
        }
    }
}

async fn voice_state_update_fallible(
    ctx: Context,
    old: Option<VoiceState>,
    new: VoiceState,
) -> anyhow::Result<()> {
    // Greet users only when they join a voice channel, not when they move to another one
    if old.is_some() {
        log::debug!("User's old voice state is not none, returning");
        return Ok(());
    }

    if ctx.cache.current_user().id == new.user_id {
        log::debug!("This user is bot, returning");
        return Ok(());
    }

    let Some(channel_id) = new.channel_id else {
        log::debug!("No channel id in new voice state, returning");
        return Ok(());
    };

    let Some(guild_id) = new.guild_id else {
        log::debug!("No guild id in new voice state, returning");
        return Ok(());
    };

    let user_id = new.user_id;

    let data = ctx.data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();

    let (guild, user, member) = tokio::try_join!(
        db_conn.get_or_insert_guild(guild_id),
        db_conn.get_or_insert_user(user_id),
        db_conn.get_or_insert_member(user_id, guild_id)
    )?;

    let should_greet = should_greet(&guild, &member);
    let should_congratulate = should_congratulate(&guild, &member, &user);
    log::debug!("Should greet: {should_greet:?}, should congratulate: {should_congratulate:?}");

    let file_storage_path = &get_config(&ctx).await.file_storage_path;

    let strategy = match (should_greet, should_congratulate) {
        (_, true) => Strategy::Congratulation,
        (true, false) => Strategy::Greeting,
        (false, false) => return Ok(()),
    };

    let audio_file_path = match strategy {
        Strategy::Greeting => guild_greeting_filepath(file_storage_path, guild_id)?,
        Strategy::Congratulation => {
            guild_birthday_congratulation_filepath(file_storage_path, guild_id)?
        }
    };

    let audio_file = songbird::input::File::new(audio_file_path);

    let client = songbird::get(&ctx).await.unwrap();

    let call = client.join(guild_id, channel_id).await?;
    let _ = call.lock().await.play_only_input(audio_file.into());

    // Update last greeting timestamp anyway, even if the user was congratulated
    update_last_greeting(db_conn, user_id, guild_id).await?;
    if strategy == Strategy::Congratulation {
        update_last_birthday_congratulation(db_conn, user_id, guild_id).await?
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    Greeting,
    Congratulation,
}

fn should_greet(guild: &Guild, member: &Member) -> bool {
    if !(guild.greeting_enabled && member.greeting_enabled) {
        log::debug!("Greeting is disabled for guild or member, returning");
        return false;
    }

    let now = Utc::now();

    log::debug!("Now: {now:?}");
    log::debug!("Member last greeting: {:?}", member.last_greeting);
    log::debug!("Guild greeting interval: {:?}", guild.greeting_interval);

    member
        .last_greeting
        .is_none_or(|last_greeting| last_greeting + guild.greeting_interval < now)
}

fn should_congratulate(guild: &Guild, member: &Member, user: &User) -> bool {
    if !guild.birthday_enabled {
        return false;
    }

    let Some(birthday) = user.birthday else {
        return false;
    };

    let now = Utc::now();
    let guild_local_time = now.with_timezone(&guild.timezone);

    let today_date = MonthDayDate::from(guild_local_time);

    if birthday != today_date {
        return false;
    }

    log::debug!("Local time: {now:?}");
    log::debug!(
        "Member last congratulation: {:?}",
        member.last_birthday_congratulation
    );

    member
        .last_birthday_congratulation
        .is_none_or(|last_congratulation| last_congratulation.year() < guild_local_time.year())
}

async fn update_last_greeting(
    db_conn: &DatabaseConnection,
    user_id: UserId,
    guild_id: GuildId,
) -> anyhow::Result<()> {
    let user_id_str = user_id.to_string();
    let guild_id_str = guild_id.to_string();
    let naive_now = Utc::now().naive_utc();
    query!(
        r#"UPDATE members SET last_greeting = ?
           WHERE user_id = ? AND guild_id = ?"#,
        naive_now,
        user_id_str,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    Ok(())
}

async fn update_last_birthday_congratulation(
    db_conn: &DatabaseConnection,
    user_id: UserId,
    guild_id: GuildId,
) -> anyhow::Result<()> {
    let user_id_str = user_id.to_string();
    let guild_id_str = guild_id.to_string();
    let naive_now = Utc::now().naive_utc();
    query!(
        r#"UPDATE members SET last_birthday_congratulation = ?
           WHERE user_id = ? AND guild_id = ?"#,
        naive_now,
        user_id_str,
        guild_id_str,
    )
    .execute(&db_conn.pool)
    .await?;

    Ok(())
}

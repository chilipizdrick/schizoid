use std::{sync::Arc, time::Duration};

use parking_lot::RwLock;
use poise::FrameworkOptions;
use schizoid::{
    commands::*,
    config::{Config, ConfigKey},
    database::{DBConnKey, DatabaseConnection},
    greeting_birthday_handler::GreetingBirthdayHandler,
    load_env_var,
    minecraft_server_handler::MinecraftServerHandler,
    voice_clip_recorder::{VCRStateKey, VoiceClipRecorderState},
};
use serenity::all::{ClientBuilder, GatewayIntents};
use songbird::{
    SerenityInit,
    driver::{Channels, DecodeConfig, DecodeMode, SampleRate},
};
use sqlx::SqlitePool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load()?;
    let config: &'static Config = Box::leak(Box::new(config));

    log::info!("Starting with the following config:\n{config:?}");

    let database_url = load_env_var("DATABASE_URL")?;
    let pool = SqlitePool::connect(&database_url).await?;
    let db_conn = DatabaseConnection::new(pool);

    let options = FrameworkOptions {
        skip_checks_for_owners: true,
        commands: vec![
            ping(),
            greet(),
            congratulate(),
            toggle_server_greeting(),
            toggle_my_greeting(),
            set_greeting_interval(),
            set_birthday(),
            set_user_birthday(),
            unset_birthday(),
            unset_user_birthday(),
            set_server_greeting(),
            remove_server_greeting(),
            set_my_greeting(),
            remove_my_greeting(),
            set_birthday_congratulation(),
            remove_birthday_congratulation(),
            color(),
            set_timezone(),
            start_surveillance(),
            leave(),
            clip(),
        ],
        ..Default::default()
    };
    let intents = GatewayIntents::all();
    let framework = poise::Framework::builder()
        .options(options)
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(())
            })
        })
        .build();

    let decode_config = DecodeConfig::new(Channels::Mono, SampleRate::Hz48000);
    let decode_mode = DecodeMode::Decode(decode_config);
    let songbird_config = songbird::Config::default().decode_mode(decode_mode);
    let voice_clip_recorder_state = Arc::new(RwLock::new(VoiceClipRecorderState::default()));

    let token = load_env_var("DISCORD_TOKEN")?;
    let mut client_builder = ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird_from_config(songbird_config)
        .event_handler(GreetingBirthdayHandler)
        .type_map_insert::<DBConnKey>(db_conn)
        .type_map_insert::<ConfigKey>(config)
        .type_map_insert::<VCRStateKey>(voice_clip_recorder_state);

    if config.minecraft_server_ping.enabled {
        let address = load_env_var("MINECRAFT_SERVER_ADDRESS")?;
        let ping_timeout = Duration::from_millis(config.minecraft_server_ping.ping_timeout_ms);
        let ping_interval = Duration::from_millis(config.minecraft_server_ping.ping_interval_ms);
        let mc_server_ping_handler =
            MinecraftServerHandler::new(address, ping_timeout, ping_interval);

        client_builder = client_builder.event_handler(mc_server_ping_handler);
    }

    let mut client = client_builder.await?;

    client.start().await?;

    Ok(())
}

use std::time::Duration;

use poise::FrameworkOptions;
use schizoid::{
    commands::*,
    config::{Config, ConfigKey},
    database::{DBConnKey, DatabaseConnection},
    greeting_birthday_handler::GreetingBirthdayHandler,
    load_env_var,
    minecraft_server_handler::MinecraftServerHandler,
    voice_clip_recorder::VCRStateKey,
};
use serenity::{
    all::{ClientBuilder, GatewayIntents},
    model::id::UserId,
};
use songbird::{SerenityInit, driver::DecodeMode};
use sqlx::SqlitePool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load()?;
    let config: &'static Config = Box::leak(Box::new(config));

    log::info!("Starting with the following config:\n{config:?}");

    let database_url = load_env_var("DATABASE_URL")?;
    let owners = load_env_var("OWNER_USER_ID")
        .into_iter()
        .map(|uid_str| uid_str.parse::<u64>().ok().map(UserId::from))
        .flatten()
        .collect();

    let pool = SqlitePool::connect(&database_url).await?;
    let db_conn = DatabaseConnection::new(pool);

    let commands = vec![
        ping(),
        greeting(),
        surveillance(),
        clip(),
        leave(),
        guild(),
        birthday(),
        color(),
    ];

    let options = FrameworkOptions {
        owners,
        commands,
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

    let songbird_config = songbird::Config::default().decode_mode(DecodeMode::Decrypt);

    let token = load_env_var("DISCORD_TOKEN")?;
    let mut client_builder = ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird_from_config(songbird_config)
        .event_handler(GreetingBirthdayHandler)
        .type_map_insert::<DBConnKey>(db_conn)
        .type_map_insert::<ConfigKey>(config)
        .type_map_insert::<VCRStateKey>(Default::default());

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

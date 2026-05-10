use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::anyhow;
use itertools::Itertools;
use mcping::JavaResponse;
use poise::serenity_prelude as serenity;
use serenity::all::{EventHandler, Ready};

#[derive(Debug)]
struct MinecraftServerPingHandler {
    is_loop_running: AtomicBool,
    server_address: String,
    server_ping_timeout: Duration,
    server_ping_interval: Duration,
}

impl MinecraftServerPingHandler {
    fn new(
        server_address: String,
        server_ping_timeout: Duration,
        server_ping_interval: Duration,
    ) -> Self {
        Self {
            is_loop_running: AtomicBool::new(false),
            server_address,
            server_ping_timeout,
            server_ping_interval,
        }
    }

    fn from_env() -> anyhow::Result<Self> {
        let server_ping_timeout = std::env::var("MINECRAFT_SERVER_PING_TIMEOUT")
            .map(|v| v.parse::<u64>())
            .unwrap_or(Ok(10))?;
        let server_ping_interval = std::env::var("MINECRAFT_SERVER_PING_INTERVAL")
            .map(|v| v.parse::<u64>())
            .unwrap_or(Ok(10))?;
        let server_address = std::env::var("MINECRAFT_SERVER_ADDRESS")
            .map_err(|_| env_err("MINECRAFT_SERVER_ADDRESS"))?;

        Ok(Self::new(
            server_address,
            Duration::from_secs(server_ping_timeout),
            Duration::from_secs(server_ping_interval),
        ))
    }
}

#[serenity::async_trait]
impl EventHandler for MinecraftServerPingHandler {
    async fn ready(&self, ctx: serenity::Context, _: Ready) {
        use serenity::gateway::ActivityData;
        use serenity::model::user::OnlineStatus;

        let request = mcping::Java {
            server_address: self.server_address.clone(),
            timeout: Some(self.server_ping_timeout),
        };
        let status = OnlineStatus::Online;

        let ping_interval = self.server_ping_interval;
        if !self.is_loop_running.swap(true, Ordering::Relaxed) {
            tokio::spawn(async move {
                loop {
                    match mcping::tokio::get_status(&request).await {
                        Ok((latency, response)) => {
                            let activity_str = format_minecraft_server_status(
                                &request.server_address,
                                latency,
                                &response,
                            );
                            let activity = ActivityData::custom(activity_str);
                            ctx.set_presence(Some(activity), status);
                        }
                        Err(err) => {
                            log::warn!("minecraft server ping failed: {err}");
                            ctx.reset_presence();
                        }
                    }
                    tokio::time::sleep(ping_interval).await;
                }
            });
        }
    }
}

fn truncate_with_ellipsis(text: &str, max_length: usize) -> String {
    if text.chars().count() <= max_length {
        text.to_string()
    } else if max_length <= 3 {
        "...".chars().take(max_length).collect()
    } else {
        let truncated: String = text.chars().take(max_length - 3).collect();
        format!("{}...", truncated)
    }
}

fn format_minecraft_server_status(
    server_address: &str,
    latency: u64,
    response: &JavaResponse,
) -> String {
    let description_text = truncate_with_ellipsis(response.description.text(), 20);

    let mut status = format!(
        "⛏️ {}/{} • {} • {} • {}ms • {}",
        response.players.online,
        response.players.max,
        server_address,
        response.version.name,
        latency,
        description_text,
    );

    if let Some(players) = &response.players.sample {
        status.push_str(" • Online: ");
        let player_names = players.iter().map(|p| p.name.as_str()).join(", ");
        status += player_names.as_str();
    }

    status
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let token = std::env::var("DISCORD_TOKEN").map_err(|_| env_err("DISCORD_TOKEN"))?;
    let intents = serenity::GatewayIntents::non_privileged();

    let minecraft_server_ping_handler = MinecraftServerPingHandler::from_env()?;

    let mut client = serenity::ClientBuilder::new(token, intents)
        .event_handler(minecraft_server_ping_handler)
        .await?;

    client.start().await?;

    Ok(())
}

fn env_err(env_var: &str) -> anyhow::Error {
    anyhow!("{} environment variable not set", env_var)
}

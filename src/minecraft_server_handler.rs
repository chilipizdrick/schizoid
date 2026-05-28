use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use mcping::JavaResponse;
use serenity::all::{Context, EventHandler, Ready};

#[derive(Debug)]
pub struct MinecraftServerHandler {
    is_loop_running: AtomicBool,
    server_address: String,
    ping_timeout: Duration,
    ping_interval: Duration,
}

impl MinecraftServerHandler {
    pub fn new(server_address: String, ping_timeout: Duration, ping_interval: Duration) -> Self {
        Self {
            is_loop_running: AtomicBool::new(false),
            server_address,
            ping_timeout,
            ping_interval,
        }
    }
}

#[serenity::async_trait]
impl EventHandler for MinecraftServerHandler {
    async fn ready(&self, ctx: Context, _: Ready) {
        use serenity::gateway::ActivityData;
        use serenity::model::user::OnlineStatus;

        let request = mcping::Java {
            server_address: self.server_address.clone(),
            timeout: Some(self.ping_timeout),
        };
        let status = OnlineStatus::Online;

        let ping_interval = self.ping_interval;
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
        let player_names = players
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        status += player_names.as_str();
    }

    status
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

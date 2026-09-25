use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::Result;
use mcping::JavaResponse;
use serenity::{
    all::{Context, EventHandler, Ready},
    builder::CreateMessage,
    futures::future::join_all,
    model::id::ChannelId,
};
use sqlx::query;

use crate::database::DBConnKey;

#[derive(Debug)]
pub struct MinecraftServerHandler {
    is_loop_running: AtomicBool,
    server_address: String,
    ping_timeout: Duration,
    ping_interval: Duration,
    failed_pings_before_alert: u32,
}

impl MinecraftServerHandler {
    pub fn new(
        server_address: String,
        ping_timeout: Duration,
        ping_interval: Duration,
        failed_pings_before_alert: u32,
    ) -> Self {
        Self {
            server_address,
            ping_timeout,
            ping_interval,
            failed_pings_before_alert,
            is_loop_running: false.into(),
        }
    }
}

#[serenity::async_trait]
impl EventHandler for MinecraftServerHandler {
    async fn ready(&self, ctx: Context, _: Ready) {
        use serenity::gateway::ActivityData;
        use serenity::model::user::OnlineStatus;

        let ctx = ctx.clone();
        let request = mcping::Java {
            server_address: self.server_address.clone(),
            timeout: Some(self.ping_timeout),
        };
        let status = OnlineStatus::Online;
        let failed_pings_before_alert = self.failed_pings_before_alert;
        let ping_interval = self.ping_interval;

        let mut consecutive_failed_pings = 0u32;
        let mut alert_fired = false;

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

                            consecutive_failed_pings = 0;
                            if alert_fired {
                                if let Err(err) =
                                    alert_all_guilds_minecraft_server_is_up(&ctx).await
                                {
                                    log::error!("error reporting minecraft server is up: {err}");
                                } else {
                                    alert_fired = false;
                                }
                            }
                        }
                        Err(err) => {
                            log::info!("minecraft server ping failed: {err}");

                            ctx.reset_presence();

                            consecutive_failed_pings = consecutive_failed_pings.saturating_add(1);
                            if consecutive_failed_pings >= failed_pings_before_alert && !alert_fired
                            {
                                if let Err(err) =
                                    alert_all_guilds_minecraft_server_is_down(&ctx).await
                                {
                                    log::error!("error reporting minecraft server is down: {err}");
                                } else {
                                    alert_fired = true;
                                }
                            }
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

async fn alert_all_guilds_minecraft_server_is_down(ctx: &Context) -> Result<()> {
    let status = CreateMessage::new().content("[ALERT] Minecraft server is down!");
    alert_all_guilds_minecraft_server_status(ctx, status).await
}

async fn alert_all_guilds_minecraft_server_is_up(ctx: &Context) -> Result<()> {
    let status = CreateMessage::new().content("[ALERT] Minecraft server is back up!");
    alert_all_guilds_minecraft_server_status(ctx, status).await
}

async fn alert_all_guilds_minecraft_server_status(
    ctx: &Context,
    status: CreateMessage,
) -> Result<()> {
    let data = ctx.data.read().await;
    let db_conn = data.get::<DBConnKey>().unwrap();
    let records = query!(
        "SELECT minecraft_text_channel_id FROM guilds WHERE minecraft_text_channel_id IS NOT NULL"
    )
    .fetch_all(&db_conn.pool)
    .await?;

    let futures = records
        .into_iter()
        .map(|record| {
            // In general it should be impossible for us to fail parsing id here
            record.minecraft_text_channel_id.unwrap().parse().map(|id| {
                let channel_id = ChannelId::new(id);
                channel_id.send_message(&ctx.http, status.clone())
            })
        })
        .flatten();

    for res in join_all(futures).await {
        if let Err(err) = res {
            log::error!("{err}");
        }
    }

    Ok(())
}

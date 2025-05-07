use anyhow::{Context, Result};
use dotenvy::dotenv;
use futures::StreamExt as _;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use twilight_model::gateway::payload::outgoing::UpdateVoiceState;

use gramophone::net::VoiceWebSocket;
use gramophone_types::OpCode;
use gramophone_types::payload::Event as VoiceGatewayEvent;
use gramophone_types::payload::outgoing::Identify;

use twilight_gateway::{CloseFrame, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_model::gateway::event::Event as GatewayEvent;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

#[path = "common/mod.rs"]
mod common;
use self::common::{OptionalExt, init_tracing, parse_id_from_env};

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenv().ok();

    let token = dotenvy::var("TOKEN").context("Missing `TOKEN` environment variable")?;
    let guild_id = parse_id_from_env::<GuildMarker>("GUILD_ID")?;
    let channel_id = parse_id_from_env::<ChannelMarker>("CHANNEL_ID")?;
    let user_id = parse_id_from_env::<UserMarker>("USER_ID")?;
    init_tracing()?;

    let mut should_connect_voice = true;
    let mut ws = None::<VoiceWebSocket>;
    let mut shard = Shard::new(
        ShardId::ONE,
        token,
        Intents::GUILDS | Intents::GUILD_VOICE_STATES,
    );

    let mut endpoint: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut token: Option<String> = None;

    let (close_tx, mut close_rx) = mpsc::channel::<()>(1);
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("could not detect CTRL+C");

        close_tx.send(()).await.expect("close_rx is dropped");
    });

    loop {
        tokio::select! {
            _ = close_rx.recv() => {
                info!("ctrl+c detected, closing all connections...");
                if let Some(ws) = ws.as_mut() {
                    ws.close(CloseFrame::NORMAL);
                }
                shard.close(CloseFrame::NORMAL);
                break;
            },
            event = ws.as_mut().map(|v| v.next()).optional() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!("voice ws: got an error: {error}");
                        continue;
                    },
                    None => {
                        warn!("voice ws: closed");
                        shard.close(CloseFrame::NORMAL);
                        break;
                    }
                };
                info!(?event, "voice ws: received voice event");

                if let VoiceGatewayEvent::GatewayHello(..) = &event {
                    debug!("voice ws: identifying connection");

                    let ws = ws.as_mut().expect("unexpected logic");
                    ws.send(&json!({
                        "op": OpCode::Identify,
                        "d": Identify {
                            guild_id,
                            user_id,
                            session_id: session_id.clone().expect("unexpected logic"),
                            token: token.clone().expect("unexpected logic"),
                        }
                    }));
                }
            },
            entry = shard.next_event(EventTypeFlags::all()) => {
                let event = match entry {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!("shard: got an error: {error}");
                        continue;
                    },
                    None => break,
                };

                match &event {
                    GatewayEvent::Ready(..) => {
                        info!("shard: bot is ready. sending update voice state");
                        shard.command(&UpdateVoiceState::new(guild_id, channel_id, true, false));
                    }
                    GatewayEvent::VoiceStateUpdate(data) => {
                        if data.guild_id != Some(guild_id)
                            || data.member.as_ref().map(|v| v.user.id) != Some(user_id)
                            || data.channel_id.is_none()
                        {
                            continue;
                        }

                        debug!("shard: received voice state update");
                        session_id = Some(data.session_id.to_string());
                    }
                    GatewayEvent::VoiceServerUpdate(data) if data.guild_id == guild_id => {
                        debug!("shard: received voice server update");
                        endpoint.clone_from(&data.endpoint);
                        token = Some(data.token.to_string());
                    }
                    _ => {
                        debug!(event = ?event.kind(), "shard: received event");
                    }
                }

                if should_connect_voice && session_id.is_some() && endpoint.is_some() && token.is_some() {
                    should_connect_voice = false;

                    info!("voice ws: received all parameters, connecting...");
                    ws = Some(VoiceWebSocket::new(endpoint.clone().expect("unexpected logic")));
                }
            }
        }
    }

    // so twilight-gateway can process something closing the connection.
    _ = shard.next().await;

    if let Some(ws) = ws.as_mut() {
        _ = ws.next().await;
    }

    Ok(())
}

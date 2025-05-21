use anyhow::{Context, Result};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use gramophone::client::{VoiceClient, VoiceConnectionInfo};
use gramophone_types::payload::Event;

use twilight_gateway::{CloseFrame, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_model::gateway::event::Event as GatewayEvent;
use twilight_model::gateway::payload::outgoing::UpdateVoiceState;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

#[path = "common/mod.rs"]
mod common;
use self::common::{OptionalExt, init_tracing, parse_id_from_env};

#[tokio::main]
async fn main() -> Result<()> {
    let token = dotenvy::var("TOKEN").context("Missing `TOKEN` environment variable")?;
    let guild_id = parse_id_from_env::<GuildMarker>("GUILD_ID")?;
    let channel_id = parse_id_from_env::<ChannelMarker>("CHANNEL_ID")?;
    let user_id = parse_id_from_env::<UserMarker>("USER_ID")?;
    init_tracing()?;

    let mut client: Option<VoiceClient> = None;
    let mut shard = Shard::new(
        ShardId::ONE,
        token,
        Intents::GUILDS | Intents::GUILD_VOICE_STATES,
    );

    let mut info = VoiceConnectionInfo::builder()
        .guild_id(guild_id)
        .user_id(user_id)
        .optional();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                warn!("ctrl+c detected, closing all connections...");
                if let Some(client) = client.as_mut() {
                    client.close(CloseFrame::NORMAL);
                }
                shard.close(CloseFrame::NORMAL);
                break;
            },
            event = client.as_mut().map(|v| v.next_event()).optional() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!(?error, "voice client: got an error");
                        continue;
                    },
                    None => {
                        warn!("voice client: closed");
                        shard.close(CloseFrame::NORMAL);
                        break;
                    }
                };

                match &event {
                    Event::Connected(..) => {
                        let transport = client.as_mut().expect("unexpected logic").transport();
                        info!("voice client: ready to transmit voice data!");
                    },
                    _ => {
                        info!(?event, "voice client: received event");
                    }
                }
            },
            entry = shard.next_event(EventTypeFlags::all()) => {
                let event = match entry {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        warn!(?error, "shard: got an error");
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
                        info = info.set_session_id(data.session_id.to_string());
                    },
                    GatewayEvent::VoiceServerUpdate(data) if data.guild_id == guild_id => {
                        debug!("shard: received voice server update");

                        let endpoint = data.endpoint.as_ref().expect("endpoint should be defined").to_string();
                        info = info.set_endpoint(endpoint).set_token(data.token.clone());
                    },
                    _ => {
                        debug!(event = ?event.kind(), "shard: received event");
                    }
                }

                if client.is_none() {
                    client = info.build_optional().map(VoiceClient::new);
                    if client.is_some() {
                        info!("voice client: received all parameters, connecting...");
                        info = info.clear();
                    }
                }
            }
        }
    }

    // so twilight can process something closing the connection.
    _ = timeout(Duration::from_secs(3), shard.next()).await;

    // same as well with gramophone...
    if let Some(client) = client.as_mut() {
        _ = timeout(Duration::from_secs(3), client.next()).await;
    }

    Ok(())
}

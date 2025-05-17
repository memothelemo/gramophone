use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use gramophone::client::{ConnectionInfoBuilder, VoiceClient, VoiceClientEvent};

use std::time::Duration;

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

    let mut info = ConnectionInfoBuilder::new()
        .optional()
        .set_guild_id(guild_id)
        .set_user_id(user_id);

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
            event = client.as_mut().map(|v| v.next()).optional() => {
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
                info!(?event, "voice client: received event");

                match event {
                    VoiceClientEvent::Event(event) => {

                    },
                    VoiceClientEvent::Reconnecting => todo!(),
                    VoiceClientEvent::Connected(info) =>{

                    },
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

                        let endpoint = data.endpoint.as_ref().unwrap().to_string();
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
                    }
                    info = info.clear();
                }
            }
        }
    }

    // so twilight can process something closing the connection.
    _ = timeout(Duration::from_secs(3), shard.next());

    if let Some(client) = client.as_mut() {
        _ = timeout(Duration::from_secs(3), client.next());
    }

    Ok(())
}

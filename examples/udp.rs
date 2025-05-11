use anyhow::Result;
use gramophone::net::VoiceUdp;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Notify;
use tokio::time::Instant;
use tracing::{info, warn};

#[path = "common/mod.rs"]
mod common;

#[tracing::instrument(skip_all)]
async fn udp_server(close_notify: Arc<Notify>) -> Result<()> {
    let stream = UdpSocket::bind("127.0.0.1:12345").await?;
    loop {
        let mut buf = [0u8; 8];
        tokio::select! {
            () = close_notify.notified() => {
                break;
            },
            _ = stream.recv(&mut buf) => {
                info!(?buf, "received UDP packet");
            }
        };
    }
    drop(stream);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    self::common::init_tracing()?;

    let closed = Arc::new(Notify::new());
    let handle = tokio::spawn({
        let closed = closed.clone();
        async {
            if let Err(error) = udp_server(closed).await {
                warn!(?error, "error occurred in UDP server");
            }
        }
    });

    tokio::task::spawn_blocking(move || {
        info!("connecting udp");

        let mut udp = VoiceUdp::new();
        udp.connect(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
            .expect("should connect to the UDP server");

        #[allow(unused_assignments)]
        let mut now = Instant::now();
        for n in 0u32..300u32 {
            now = Instant::now();
            udp.send(&n.to_be_bytes()).expect("should send packet");
            let elapsed = now.elapsed();
            info!("sending time: {elapsed:.2?}");
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    });

    tokio::signal::ctrl_c().await?;
    closed.notify_waiters();
    tokio::task::yield_now().await;

    info!("waiting for the UDP server to shut down...");
    closed.notify_waiters();
    handle.await?;

    Ok(())
}

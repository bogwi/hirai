//! Network module for UDP multicast event broadcasting and listening.
//!
//! This module provides asynchronous functions for sending and receiving
//! file events over UDP multicast, as well as multicast group management utilities.

use crate::config::AppConfig;
use crate::event::Event;
use anyhow::Result;
use std::net::Ipv4Addr;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::watch::Receiver as WatchReceiver;
use tracing::{debug, error, info, warn};

const MAX_DATAGRAM_SIZE: usize = 8192;

/// Checks if a given `SocketAddr` is a multicast address (IPv4 or IPv6).
///
/// # Arguments
/// * `socket_addr` - The socket address to check.
///
/// # Returns
/// * `true` if the address is multicast, `false` otherwise.
fn is_socket_addr_multicast(socket_addr: &SocketAddr) -> bool {
    match socket_addr.ip() {
        IpAddr::V4(ipv4) => ipv4.is_multicast(),
        IpAddr::V6(ipv6) => ipv6.is_multicast(),
    }
}

/// Joins the UDP socket to the specified multicast group (IPv4 only).
///
/// # Arguments
/// * `socket` - The UDP socket to join the group with.
/// * `multicast_addr_str` - The multicast group address as a string.
///
/// # Errors
/// Returns an error if the address is not multicast, is not IPv4, or if joining fails.
async fn join_multicast(socket: &UdpSocket, multicast_addr_str: &str) -> Result<()> {
    let multicast_socket_addr: SocketAddr = multicast_addr_str.parse()?;
    if !is_socket_addr_multicast(&multicast_socket_addr) {
        return Err(anyhow::anyhow!(
            "Address is not multicast: {}",
            multicast_addr_str
        ));
    }

    let ip_addr = match multicast_socket_addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(_) => return Err(anyhow::anyhow!("IPv6 multicast not yet supported")),
    };

    let interface = Ipv4Addr::UNSPECIFIED;
    socket.join_multicast_v4(ip_addr, interface)?;
    info!(
        "Joined multicast group {} on interface {}",
        ip_addr, interface
    );
    Ok(())
}

/// Runs the UDP event broadcaster.
///
/// This function serializes incoming file events and sends them as UDP datagrams
/// to the configured multicast address. It listens for shutdown signals to exit cleanly.
///
/// # Arguments
/// * `app_config` - Shared application configuration.
/// * `event_rx` - Receiver for file events to broadcast.
/// * `shutdown_signal` - Watch channel for shutdown notification.
///
/// # Errors
/// Returns an error if configuration is invalid or socket operations fail.
pub async fn run_broadcaster(
    app_config: Arc<AppConfig>,
    mut event_rx: Receiver<Event>,
    shutdown_signal: WatchReceiver<bool>,
) -> Result<()> {
    if app_config.folders_to_watch.is_empty() {
        return Err(anyhow::anyhow!("No folders specified for broadcaster mode"));
    }
    let target_addr_str = app_config.multicast_addr.clone();
    let target_socket_addr: SocketAddr = target_addr_str.parse()?;

    let local_addr: SocketAddr = "0.0.0.0:0".parse()?;
    let socket = UdpSocket::bind(local_addr).await?;
    info!("Broadcaster started, sending to {}", target_socket_addr);

    let mut shutdown = shutdown_signal.clone();

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                debug!("Broadcaster received event: {:?}", event);
                match serde_json::to_vec(&event) {
                    Ok(payload) => {
                        if payload.len() > MAX_DATAGRAM_SIZE {
                            warn!("Serialized event too large for UDP: {} bytes", payload.len());
                            continue;
                        }
                        match socket.send_to(&payload, &target_socket_addr).await {
                            Ok(bytes_sent) => {
                                debug!("Sent {} bytes for event: {:?}", bytes_sent, event.op);
                            }
                            Err(e) => {
                                error!("Failed to send UDP multicast message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize event to JSON: {}", e);
                    }
                }
            }
            Ok(()) = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Broadcaster shutting down due to signal.");
                    break;
                }
            }
            else => {
                info!("Broadcaster event channel closed or shutdown error. Exiting.");
                break;
            }
        }
    }
    Ok(())
}

/// Runs the UDP event listener.
///
/// This function binds to the configured multicast group and port, receives UDP datagrams,
/// deserializes them into file events, and optionally forwards them to a web event channel.
/// It listens for shutdown signals to exit cleanly.
///
/// # Arguments
/// * `app_config` - Shared application configuration.
/// * `event_tx_for_web` - Optional sender to forward received events to a web server.
/// * `shutdown_signal` - Watch channel for shutdown notification.
///
/// # Errors
/// Returns an error if configuration is invalid or socket operations fail.
pub async fn run_listener(
    app_config: Arc<AppConfig>,
    event_tx_for_web: Option<Sender<Event>>,
    shutdown_signal: WatchReceiver<bool>,
) -> Result<()> {
    let listen_addr_str = app_config.multicast_addr.clone();
    let listen_socket_addr: SocketAddr = listen_addr_str.parse()?;

    if !is_socket_addr_multicast(&listen_socket_addr) {
        return Err(anyhow::anyhow!(
            "Address is not multicast: {}",
            listen_addr_str
        ));
    }

    let local_bind_addr_str = format!("0.0.0.0:{}", listen_socket_addr.port());
    let local_bind_addr: SocketAddr = local_bind_addr_str.parse()?;

    let socket = UdpSocket::bind(local_bind_addr).await?;
    info!(
        "Listener attempting to bind to {} and join multicast group {}",
        local_bind_addr, listen_addr_str
    );

    join_multicast(&socket, &listen_addr_str).await?;
    info!(
        "Listener started, listening on multicast group {}",
        listen_addr_str
    );

    let mut buffer = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut shutdown = shutdown_signal.clone();

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buffer) => {
                match result {
                    Ok((len, src_addr)) => {
                        let data = &buffer[..len];
                        match serde_json::from_slice::<Event>(data) {
                            Ok(event) => {
                                info!("Listener received event from {}: Path: {}, Op: {}", src_addr, event.path, event.op);
                                if let Some(tx) = &event_tx_for_web {
                                    if let Err(e) = tx.send(event.clone()).await {
                                        error!("Failed to send event to web hub: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize event from {}: {}. Data (UTF-8 lossy): {}", src_addr, e, String::from_utf8_lossy(data));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving UDP packet: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }
            Ok(()) = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Listener shutting down due to signal.");
                    break;
                }
            }
            else => {
                 info!("Listener shutdown or channel error. Exiting.");
                 break;
            }
        }
    }
    Ok(())
}

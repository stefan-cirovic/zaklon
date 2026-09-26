//! Lets phones find the hub on the local network without typing addresses:
//! DNS-SD (`_zaklon._tcp`) plus a tiny UDP beacon for networks that block
//! multicast DNS. Both answer with the same fields that are in the pairing QR.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::Serialize;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

use crate::HubState;

pub const SERVICE_TYPE: &str = "_zaklon._tcp.local.";
/// A discovery request must be at least this long, so the reply is never
/// bigger than the request (no use as a traffic amplifier).
pub const BEACON_MIN_REQUEST: usize = 512;
/// At most this many replies per second.
const BEACON_MAX_PER_SECOND: u32 = 20;

/// Keeps the mDNS daemon alive; dropping it unregisters the service.
pub struct Discovery {
    _mdns: Option<ServiceDaemon>,
}

#[derive(Serialize)]
struct BeaconReply<'a> {
    v: u8,
    id: &'a str,
    name: &'a str,
    port: u16,
    fp: &'a str,
}

/// Interface names that belong to virtual machines, WSL or containers; phones
/// can never reach those addresses, so they are listed last.
const VIRTUAL_HINTS: &[&str] = &["vethernet", "wsl", "hyper-v", "vmware", "virtualbox", "vbox", "docker", "loopback", "tailscale", "zerotier"];

pub fn lan_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut real = Vec::new();
    let mut virtual_ = Vec::new();
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                    continue;
                }
                let lower = name.to_lowercase();
                if VIRTUAL_HINTS.iter().any(|h| lower.contains(h)) {
                    virtual_.push(v4);
                } else {
                    real.push(v4);
                }
            }
        }
    }
    real.sort();
    real.dedup();
    virtual_.sort();
    virtual_.dedup();
    real.extend(virtual_);
    real
}

pub async fn start(state: Arc<HubState>) -> Result<Discovery> {
    let cfg = state.config();
    let mdns = match ServiceDaemon::new() {
        Ok(daemon) => {
            let host = format!("{}.local.", cfg.hub_id.split('-').next().unwrap_or("zaklon"));
            let ips: Vec<IpAddr> = lan_ipv4_addresses().into_iter().map(IpAddr::V4).collect();
            let props = [
                ("v", "1"),
                ("id", cfg.hub_id.as_str()),
                ("name", cfg.hub_name.as_str()),
                ("fp", state.identity.fingerprint.as_str()),
            ];
            match ServiceInfo::new(SERVICE_TYPE, &cfg.hub_id, &host, &ips[..], cfg.port, &props[..]) {
                Ok(info) => match daemon.register(info) {
                    Ok(()) => {
                        info!("DNS-SD service registered as {SERVICE_TYPE}");
                        Some(daemon)
                    }
                    Err(e) => {
                        warn!("DNS-SD register failed: {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("DNS-SD service info failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            warn!("DNS-SD unavailable: {e}");
            None
        }
    };

    let beacon_port = cfg.beacon_port;
    let socket = UdpSocket::bind(("0.0.0.0", beacon_port))
        .await
        .with_context(|| format!("binding UDP beacon on {beacon_port}"))?;
    socket.set_broadcast(true).ok();
    tokio::spawn(beacon_loop(socket, state));
    Ok(Discovery { _mdns: mdns })
}

async fn beacon_loop(socket: UdpSocket, state: Arc<HubState>) {
    let mut buf = [0u8; 2048];
    let mut window = std::time::Instant::now();
    let mut sent_in_window = 0u32;
    loop {
        let Ok((n, peer)) = socket.recv_from(&mut buf).await else { continue };
        if n < BEACON_MIN_REQUEST || !buf[..n].starts_with(b"ZAKLON?") {
            continue;
        }
        if window.elapsed() >= std::time::Duration::from_secs(1) {
            window = std::time::Instant::now();
            sent_in_window = 0;
        }
        if sent_in_window >= BEACON_MAX_PER_SECOND {
            continue;
        }
        sent_in_window += 1;
        let cfg = state.config();
        let reply = BeaconReply {
            v: 1,
            id: &cfg.hub_id,
            name: &cfg.hub_name,
            port: cfg.port,
            fp: &state.identity.fingerprint,
        };
        if let Ok(json) = serde_json::to_vec(&reply) {
            debug!(%peer, "beacon reply");
            let _ = socket.send_to(&json, peer).await;
        }
    }
}

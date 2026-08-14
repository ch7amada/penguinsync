//! Candidate addresses for the QR code.
//!
//! The first connection must never depend on discovery (docs/protocol.md
//! §2), so the QR carries real dialable addresses, not just "listen on
//! whatever". The daemon listens on the wildcard address; this guesses which
//! of the machine's addresses a phone on the same LAN could actually reach.

use std::net::{IpAddr, SocketAddr, UdpSocket};

/// Best-effort candidate addresses for `listen_addr`'s port. If the listener
/// is already bound to a specific (non-wildcard) address, that's the only
/// candidate — the operator chose it deliberately. Otherwise, guesses the
/// outbound-facing local IP via a connected UDP socket (a route lookup; no
/// packet is actually sent) and falls back to loopback if even that fails,
/// so pairing on the same machine still works.
pub fn candidate_addrs(listen_addr: SocketAddr) -> Vec<SocketAddr> {
    if !listen_addr.ip().is_unspecified() {
        return vec![listen_addr];
    }

    let mut addrs = Vec::new();
    if let Some(ip) = guess_outbound_ip() {
        addrs.push(SocketAddr::new(ip, listen_addr.port()));
    }
    addrs.push(SocketAddr::new(
        IpAddr::from([127, 0, 0, 1]),
        listen_addr.port(),
    ));
    addrs
}

fn guess_outbound_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connecting a UDP socket only performs a routing-table lookup for the
    // local address to use — no packet is sent, and 8.8.8.8 need not be
    // reachable for this to succeed.
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_address_is_its_own_only_candidate() {
        let addr: SocketAddr = "192.168.1.5:58210".parse().unwrap();
        assert_eq!(candidate_addrs(addr), vec![addr]);
    }

    #[test]
    fn wildcard_address_includes_loopback_fallback() {
        let addr: SocketAddr = "0.0.0.0:58210".parse().unwrap();
        let candidates = candidate_addrs(addr);
        assert!(candidates.contains(&"127.0.0.1:58210".parse().unwrap()));
    }
}

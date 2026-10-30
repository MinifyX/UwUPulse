//! Small things about this machine: its name, its LAN addresses, the clock.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{SystemTime, UNIX_EPOCH};

use uwupulse_proto::limits::{truncate_chars, MAX_NAME_CHARS};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The name phones see: the host name without `.local`, cut to 64
/// characters, control characters dropped.
pub fn desktop_name() -> String {
    let raw = gethostname::gethostname().to_string_lossy().into_owned();
    let raw = raw.strip_suffix(".local").unwrap_or(&raw);
    let clean: String = raw.chars().filter(|c| !c.is_control()).collect();
    let clean = truncate_chars(clean.trim(), MAX_NAME_CHARS).to_owned();
    if clean.is_empty() {
        "UwUPulse".to_owned()
    } else {
        clean
    }
}

/// IPv4 addresses on the LAN: not loopback, not link-local. IPv6 is left
/// out on purpose — OBS on another machine and the phone both reach the
/// desktop over IPv4, and temporary IPv6 addresses would make the list long.
pub fn lan_ips() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if !v4.is_link_local() && !v4.is_unspecified() => Some(v4),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The addresses that go into a pairing URI's `h=`: only the local network
/// (RFC 1918, link-local). The phone refuses any other host, so a QR code
/// cannot send it to the internet; a public or CGNAT address here would only
/// make it reject the whole code.
pub fn pairing_hosts(ips: impl IntoIterator<Item = Ipv4Addr>) -> Vec<IpAddr> {
    ips.into_iter()
        .filter(|ip| ip.is_private() || ip.is_link_local())
        .map(IpAddr::V4)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_hosts_are_local_only() {
        let ips = [
            "192.168.1.10",
            "10.0.0.2",
            "172.16.5.1",
            "169.254.3.4",
            "203.0.113.7",
            "100.64.0.1",
            "172.32.0.1",
            "127.0.0.1",
        ]
        .map(|s| s.parse::<Ipv4Addr>().unwrap());
        let want: Vec<IpAddr> = ["192.168.1.10", "10.0.0.2", "172.16.5.1", "169.254.3.4"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(pairing_hosts(ips), want);
    }
}

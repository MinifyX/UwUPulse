//! The pairing URI the desktop shows as a QR code.
//!
//! `uwupulse://pair?v=1&id=<desk-id>&n=<name>&pk=<b64 spk>&s=<b64 secret>&p=<port>&h=<ip1>,<ip2>`
//!
//! Values are percent-encoded (everything but `A-Z a-z 0-9 - . _ ~`); the
//! hosts are encoded one by one and joined with a literal comma.

use std::net::IpAddr;

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::limits::{valid_device_id, valid_name};
use crate::{b64_decode_32, b64_encode, PROTOCOL_VERSION};

const PREFIX: &str = "uwupulse://pair?";
/// RFC 3986's unreserved characters stay as they are.
const VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
/// More addresses than any real machine has; a cap for parsing.
const MAX_HOSTS: usize = 16;
/// A sanity cap for the whole URI.
const MAX_URI_LEN: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingUri {
    pub id: String,
    pub name: String,
    pub pk: [u8; 32],
    pub secret: [u8; 32],
    pub port: u16,
    pub hosts: Vec<IpAddr>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("not a valid UwUPulse pairing link")]
pub struct InvalidPairingUri;

impl PairingUri {
    pub fn to_uri(&self) -> String {
        let enc = |s: &str| utf8_percent_encode(s, VALUE).to_string();
        let hosts: Vec<String> = self.hosts.iter().map(|h| enc(&h.to_string())).collect();
        format!(
            "{PREFIX}v={PROTOCOL_VERSION}&id={}&n={}&pk={}&s={}&p={}&h={}",
            enc(&self.id),
            enc(&self.name),
            b64_encode(&self.pk),
            b64_encode(&self.secret),
            self.port,
            hosts.join(",")
        )
    }

    pub fn parse(uri: &str) -> Result<Self, InvalidPairingUri> {
        if uri.len() > MAX_URI_LEN {
            return Err(InvalidPairingUri);
        }
        let query = uri.strip_prefix(PREFIX).ok_or(InvalidPairingUri)?;
        let dec = |s: &str| {
            percent_decode_str(s)
                .decode_utf8()
                .map(|c| c.into_owned())
                .map_err(|_| InvalidPairingUri)
        };
        let (mut v, mut id, mut name, mut pk, mut secret, mut port, mut hosts) =
            (None, None, None, None, None, None, None);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').ok_or(InvalidPairingUri)?;
            let slot = match key {
                "v" => &mut v,
                "id" => &mut id,
                "n" => &mut name,
                "pk" => &mut pk,
                "s" => &mut secret,
                "p" => &mut port,
                "h" => &mut hosts,
                // Newer desktops may add fields.
                _ => continue,
            };
            if slot.replace(value).is_some() {
                return Err(InvalidPairingUri);
            }
        }
        if v.ok_or(InvalidPairingUri)? != PROTOCOL_VERSION.to_string() {
            return Err(InvalidPairingUri);
        }
        let id = dec(id.ok_or(InvalidPairingUri)?)?;
        let name = dec(name.ok_or(InvalidPairingUri)?)?;
        if !valid_device_id(&id) || !valid_name(&name) {
            return Err(InvalidPairingUri);
        }
        let pk = b64_decode_32(pk.ok_or(InvalidPairingUri)?).ok_or(InvalidPairingUri)?;
        let secret = b64_decode_32(secret.ok_or(InvalidPairingUri)?).ok_or(InvalidPairingUri)?;
        let port: u16 = port
            .ok_or(InvalidPairingUri)?
            .parse()
            .map_err(|_| InvalidPairingUri)?;
        if port == 0 {
            return Err(InvalidPairingUri);
        }
        let mut list = Vec::new();
        let hosts = hosts.unwrap_or("");
        if !hosts.is_empty() {
            for h in hosts.split(',') {
                if list.len() == MAX_HOSTS {
                    return Err(InvalidPairingUri);
                }
                list.push(dec(h)?.parse().map_err(|_| InvalidPairingUri)?);
            }
        }
        Ok(Self {
            id,
            name,
            pk,
            secret,
            port,
            hosts: list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PairingUri {
        PairingUri {
            id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".into(),
            name: "Lorin's Mac & Co".into(),
            pk: [7; 32],
            secret: [9; 32],
            port: 8746,
            hosts: vec![
                "192.0.2.10".parse().unwrap(),
                "2001:db8::1".parse().unwrap(),
            ],
        }
    }

    #[test]
    fn round_trip() {
        let p = sample();
        let uri = p.to_uri();
        assert!(uri.contains("n=Lorin%27s%20Mac%20%26%20Co"), "{uri}");
        assert!(uri.ends_with("&h=192.0.2.10,2001%3Adb8%3A%3A1"), "{uri}");
        assert_eq!(PairingUri::parse(&uri).unwrap(), p);
    }

    #[test]
    fn rejects_garbage() {
        let uri = sample().to_uri();
        let bad = [
            String::new(),
            "uwupulse://pair?".into(),
            uri.replace("v=1", "v=2"),
            uri.replace("p=8746", "p=0"),
            uri.replace("p=8746", "p=70000"),
            uri.replace("&s=", "&x="),
            uri.replace("uwupulse://", "https://"),
            format!("{uri}&v=1"),
            uri.replace("192.0.2.10", "not-an-ip"),
            format!("{uri}{}", ",192.0.2.1".repeat(20)),
            uri.replace("n=Lorin", "n=%FF"),
        ];
        for b in bad {
            assert_eq!(PairingUri::parse(&b), Err(InvalidPairingUri), "{b}");
        }
        let no_hosts = uri.split("&h=").next().unwrap().to_owned();
        assert!(PairingUri::parse(&no_hosts).unwrap().hosts.is_empty());
    }
}

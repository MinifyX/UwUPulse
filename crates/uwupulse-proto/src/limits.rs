//! The protocol's limits, in one place, and the checks that enforce them.

use std::time::Duration;

/// Lowest heart rate taken; anything below is a sensor glitch.
pub const BPM_MIN: u16 = 20;
/// Highest heart rate taken.
pub const BPM_MAX: u16 = 250;
/// Messages per second one link connection may send before it is closed.
pub const MAX_MSGS_PER_SEC: u32 = 20;
/// Longest device or source name, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Largest frame, text or binary, either direction.
pub const MAX_FRAME_BYTES: usize = 16 * 1024;
/// Most RR intervals one `hr` message may carry.
pub const MAX_RR: usize = 32;
/// The desktop closes a link that sent nothing for this long.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// The phone pings this often.
pub const PING_INTERVAL: Duration = Duration::from_secs(10);
/// How long the desktop waits for the user to confirm a code.
pub const SAS_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a QR offer stays valid.
pub const OFFER_TTL: Duration = Duration::from_secs(300);
/// Link connections at the same time.
pub const MAX_LINK_CONNECTIONS: usize = 8;
/// Of those, connections that have not reached `ready` yet.
pub const MAX_PREAUTH_CONNECTIONS: usize = 4;
/// Connections before `ready` from one address.
pub const MAX_PREAUTH_PER_IP: usize = 2;

/// Whether a heart rate is plausible enough to keep.
pub fn valid_bpm(bpm: i64) -> bool {
    (BPM_MIN as i64..=BPM_MAX as i64).contains(&bpm)
}

/// A name shown in the UI: 1 to 64 characters, no control characters.
pub fn valid_name(name: &str) -> bool {
    let mut count = 0usize;
    for c in name.chars() {
        if c.is_control() {
            return false;
        }
        count += 1;
        if count > MAX_NAME_CHARS {
            return false;
        }
    }
    count > 0 && !name.trim().is_empty()
}

/// A device ID: a UUID in its canonical lowercase spelling
/// (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
pub fn valid_device_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
}

/// Cuts a string to at most `max` characters, on a character boundary.
pub fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_range() {
        assert!(!valid_bpm(19));
        assert!(valid_bpm(20));
        assert!(valid_bpm(250));
        assert!(!valid_bpm(251));
        assert!(!valid_bpm(-1));
    }

    #[test]
    fn names() {
        assert!(valid_name("Lorin's iPhone"));
        assert!(!valid_name(""));
        assert!(!valid_name("   "));
        assert!(!valid_name("a\nb"));
        assert!(valid_name(&"ä".repeat(64)));
        assert!(!valid_name(&"ä".repeat(65)));
        assert_eq!(truncate_chars("äöü", 2), "äö");
        assert_eq!(truncate_chars("ab", 5), "ab");
    }

    #[test]
    fn device_ids() {
        assert!(valid_device_id("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!valid_device_id("0F8FAD5B-D9CB-469F-A165-70867728950E"));
        assert!(!valid_device_id("0f8fad5bd9cb469fa16570867728950e"));
        assert!(!valid_device_id("0f8fad5b-d9cb-469f-a165-70867728950g"));
    }
}

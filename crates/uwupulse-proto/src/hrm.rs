//! The BLE Heart Rate Measurement characteristic (0x2A37) of the Heart Rate
//! service (0x180D), as chest straps send it.
//!
//! ```text
//! flags  u8   bit 0: heart rate is u16 (else u8)
//!             bit 1-2: sensor contact (bit 2 = supported, bit 1 = detected)
//!             bit 3: energy expended (u16, kJ) follows
//!             bit 4: RR intervals (u16 each, 1/1024 s) fill the rest
//! hr     u8 | u16 little-endian
//! ```
//!
//! Straps are not always tidy: a short packet is `None`, a half RR interval
//! at the end is ignored, nothing here panics.

/// The Heart Rate service.
pub const HEART_RATE_SERVICE: u16 = 0x180D;
/// The Heart Rate Measurement characteristic.
pub const HEART_RATE_MEASUREMENT: u16 = 0x2A37;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartRate {
    pub bpm: u16,
    /// `None` when the strap doesn't report contact.
    pub contact: Option<bool>,
    pub energy_kj: Option<u16>,
    /// RR intervals in milliseconds, rounded.
    pub rr_ms: Vec<u32>,
}

/// Parses one notification of 0x2A37.
pub fn parse(data: &[u8]) -> Option<HeartRate> {
    let (&flags, mut rest) = data.split_first()?;
    let take_u16 = |rest: &mut &[u8]| -> Option<u16> {
        let (b, tail) = rest.split_first_chunk::<2>()?;
        *rest = tail;
        Some(u16::from_le_bytes(*b))
    };
    let bpm = if flags & 0x01 != 0 {
        take_u16(&mut rest)?
    } else {
        let (&b, tail) = rest.split_first()?;
        rest = tail;
        b as u16
    };
    let contact = (flags & 0x04 != 0).then_some(flags & 0x02 != 0);
    let energy_kj = if flags & 0x08 != 0 {
        Some(take_u16(&mut rest)?)
    } else {
        None
    };
    let mut rr_ms = Vec::new();
    if flags & 0x10 != 0 {
        while let Some(rr) = take_u16(&mut rest) {
            rr_ms.push(((rr as u32) * 1000 + 512) / 1024);
        }
    }
    Some(HeartRate {
        bpm,
        contact,
        energy_kj,
        rr_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_heart_rate() {
        let hr = parse(&[0x00, 72]).unwrap();
        assert_eq!(hr.bpm, 72);
        assert_eq!(hr.contact, None);
        assert!(hr.rr_ms.is_empty());
    }

    #[test]
    fn u16_contact_energy_rr() {
        // u16 bpm 300, contact supported + detected, energy 0x0102, RR 1024 and 512.
        let hr = parse(&[0x1F, 0x2C, 0x01, 0x02, 0x01, 0x00, 0x04, 0x00, 0x02]).unwrap();
        assert_eq!(hr.bpm, 300);
        assert_eq!(hr.contact, Some(true));
        assert_eq!(hr.energy_kj, Some(0x0102));
        assert_eq!(hr.rr_ms, vec![1000, 500]);
    }

    #[test]
    fn rr_without_energy_and_odd_tail() {
        let hr = parse(&[0x10, 60, 0x00, 0x04, 0x07]).unwrap();
        assert_eq!(hr.bpm, 60);
        assert_eq!(hr.rr_ms, vec![1000]);
    }

    #[test]
    fn malformed_never_panics() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x00]), None);
        assert_eq!(parse(&[0x01, 0x50]), None);
        assert_eq!(parse(&[0x08, 0x50, 0x01]), None);
        // Every combination of flags and short lengths.
        for flags in 0..=255u8 {
            for len in 0..8 {
                let mut d = vec![flags];
                d.extend(std::iter::repeat_n(0xFF, len));
                let _ = parse(&d);
            }
        }
    }
}

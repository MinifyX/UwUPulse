//! `docs/protocol-vectors.json`: the protocol, byte for byte, from fixed keys.
//!
//! The Swift side (`PulseKit`) runs the same cases against the same file. The
//! file is written when it is missing or with `UPDATE_VECTORS=1`; otherwise
//! this test fails when the code no longer produces exactly what it says.

use serde_json::{json, Value};

use crate::frame::{nonce, LinkCipher};
use crate::handshake::{
    commitment, schedule, DesktopHandshake, PhoneHandshake, PhoneTarget, StaticKeypair,
};
use crate::messages::{DeskMsg, PhoneMsg, Src, WatchAction};
use crate::pairing::PairingUri;
use crate::{b64_encode, hrm};

const PHONE_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const DESK_ID: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 32 bytes that differ per position, so a byte-order slip shows.
fn key(seed: u8) -> [u8; 32] {
    std::array::from_fn(|i| seed ^ (i as u8).wrapping_mul(7))
}

fn party(id: &str, name: &str, s: &StaticKeypair, e: &StaticKeypair) -> Value {
    json!({
        "id": id,
        "name": name,
        "static_private": hex(&s.secret_bytes()),
        "static_public": hex(&s.public_bytes()),
        "static_public_b64": b64_encode(&s.public_bytes()),
        "ephemeral_private": hex(&e.secret_bytes()),
        "ephemeral_public": hex(&e.public_bytes()),
        "ephemeral_public_b64": b64_encode(&e.public_bytes()),
    })
}

fn frames(cipher: &mut LinkCipher, msgs: &[String]) -> Vec<Value> {
    msgs.iter()
        .enumerate()
        .map(|(i, m)| {
            json!({
                "counter": i,
                "nonce": hex(&nonce(i as u64)),
                "plaintext": m,
                "plaintext_hex": hex(m.as_bytes()),
                "ciphertext": hex(&cipher.seal(m.as_bytes()).unwrap()),
            })
        })
        .collect()
}

fn case(name: &str, target: PhoneTarget, seed: u8) -> Value {
    let phone_s = StaticKeypair::from_secret_bytes(key(0x10));
    let desk_s = StaticKeypair::from_secret_bytes(key(0x20));
    let phone_e = StaticKeypair::from_secret_bytes(key(seed));
    let desk_e = StaticKeypair::from_secret_bytes(key(seed.wrapping_add(1)));
    let psk = match &target {
        PhoneTarget::Qr { secret, .. } => *secret,
        _ => [0; 32],
    };

    let phone = PhoneHandshake::with_ephemeral(
        &phone_s,
        PHONE_ID,
        "Lorin's iPhone",
        target.clone(),
        phone_e.secret_bytes(),
    );
    let m1 = phone.m1().to_owned();
    let desk = DesktopHandshake::receive(&m1).unwrap();
    let (m2, pending) = desk
        .respond_with_ephemeral(&desk_s, DESK_ID, "Studio Mac", &psk, desk_e.secret_bytes())
        .unwrap();
    let done = phone.finish(&m2).unwrap();
    let (keys, m3) = (done.keys, done.m3);
    assert_eq!(keys, pending.finish(m3.as_deref()).unwrap());
    let sas = matches!(target, PhoneTarget::Sas);
    assert_eq!(m3.is_some(), sas);
    let commit = sas.then(|| b64_encode(&commitment(&phone_e.public_bytes())));

    let dh_e = phone_e.dh(&desk_e.public_bytes()).unwrap();
    let dh_s = phone_s.dh(&desk_s.public_bytes()).unwrap();
    let sched = schedule(
        m1.as_bytes(),
        m2.as_bytes(),
        m3.as_deref().map(str::as_bytes),
        &dh_e,
        &dh_s,
        &psk,
    );
    assert_eq!(sched.keys, keys);

    let p2d: Vec<String> = [
        PhoneMsg::Auth,
        PhoneMsg::Hr {
            ts: 1_767_225_600_000,
            bpm: 72,
            src: Src::Watch,
            dev: "Apple Watch".into(),
            rr: Some(vec![833, 829]),
        },
        PhoneMsg::Ping,
    ]
    .iter()
    .map(|m| serde_json::to_string(m).unwrap())
    .collect();
    let mut d2p_msgs = Vec::new();
    if name != "known" {
        d2p_msgs.push(DeskMsg::Paired {
            id: DESK_ID.into(),
            name: "Studio Mac".into(),
        });
    }
    d2p_msgs.extend([
        DeskMsg::Ready,
        DeskMsg::Pong,
        DeskMsg::Cmd {
            action: WatchAction::Start,
        },
    ]);
    let d2p: Vec<String> = d2p_msgs
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();

    let p2d = frames(&mut LinkCipher::phone(&keys), &p2d);
    let d2p = frames(&mut LinkCipher::desktop(&keys), &d2p);

    json!({
        "name": name,
        "mode": target.mode().as_str(),
        "phone": party(PHONE_ID, "Lorin's iPhone", &phone_s, &phone_e),
        "desktop": party(DESK_ID, "Studio Mac", &desk_s, &desk_e),
        "psk": hex(&psk),
        "commit": commit,
        "m1": m1,
        "m2": m2,
        "m3": m3,
        "dh_ephemeral": hex(&dh_e),
        "dh_static": hex(&dh_s),
        "ikm": hex(&sched.ikm),
        "th": hex(&keys.th),
        "okm": hex(&sched.okm),
        "k_p2d": hex(&keys.k_p2d),
        "k_d2p": hex(&keys.k_d2p),
        "sas": keys.sas,
        "frames_p2d": p2d,
        "frames_d2p": d2p,
    })
}

fn generate() -> Value {
    let desk_pk = StaticKeypair::from_secret_bytes(key(0x20)).public_bytes();
    let secret = key(0x5A);
    let cases = vec![
        case("known", PhoneTarget::Known { desk_spk: desk_pk }, 0x30),
        case(
            "qr",
            PhoneTarget::Qr {
                desk_spk: desk_pk,
                secret,
            },
            0x40,
        ),
        case("sas", PhoneTarget::Sas, 0x50),
    ];
    let uri = PairingUri {
        id: DESK_ID.into(),
        name: "Studio Mac".into(),
        pk: desk_pk,
        secret,
        port: 8746,
        hosts: vec!["192.168.1.10".parse().unwrap(), "fe80::1".parse().unwrap()],
    };
    let hrm_cases: Vec<Value> = [
        &[0x00, 0x48][..],
        &[0x06, 0x50],
        &[0x01, 0x2C, 0x01],
        &[0x18, 0x40, 0x10, 0x00, 0x00, 0x04, 0x41, 0x03],
        &[0x10, 0x3C, 0x00, 0x04, 0x07],
        &[0x01, 0x50],
        &[],
    ]
    .iter()
    .map(|d| match hrm::parse(d) {
        Some(h) => json!({
            "input": hex(d), "bpm": h.bpm, "contact": h.contact,
            "energy_kj": h.energy_kj, "rr_ms": h.rr_ms,
        }),
        None => json!({ "input": hex(d), "invalid": true }),
    })
    .collect();

    json!({
        "description": "UwUPulse link protocol v1 test vectors, generated by crates/uwupulse-proto (cargo test, UPDATE_VECTORS=1 to regenerate). All byte strings are lowercase hex unless the key ends in _b64 (base64url, no padding). m1/m2/m3 are the exact UTF-8 text frames; in sas, m1 carries commit = SHA-256(\"UwUPulse-commit-v1\" || phone ephemeral_public) instead of epk (field commit: base64url as in m1, despite the name; null outside sas), and m3 is the phone's reveal (null otherwise); th = SHA-256(\"UwUPulse-link-v1\" || m1 || m2), in sas SHA-256(\"UwUPulse-link-v1\" || m1 || m2 || m3); ikm = dh_ephemeral || dh_static || psk; okm = HKDF-SHA256(salt = th, ikm, info = \"uwupulse link keys\", L = 68); k_p2d = okm[0..32], k_d2p = okm[32..64]; sas = u32_be(okm[64..68]) mod 1000000, zero-padded to 6 digits. Frames: ChaCha20-Poly1305, nonce = 00000000 || u64_le(counter), empty AAD, ciphertext includes the 16-byte tag. X25519 private keys are given unclamped, as stored.",
        "version": 1,
        "transcript_label": "UwUPulse-link-v1",
        "hkdf_info": "uwupulse link keys",
        "commit_label": "UwUPulse-commit-v1",
        "cases": cases,
        "pairing_uri": {
            "uri": uri.to_uri(),
            "id": uri.id,
            "name": uri.name,
            "pk": hex(&uri.pk),
            "secret": hex(&uri.secret),
            "port": uri.port,
            "hosts": ["192.168.1.10", "fe80::1"],
        },
        "heart_rate_measurement": hrm_cases,
    })
}

#[test]
fn protocol_vectors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/protocol-vectors.json"
    );
    let fresh = generate();
    let update = std::env::var("UPDATE_VECTORS").is_ok_and(|v| v == "1");
    match std::fs::read_to_string(path) {
        Ok(text) if !update => {
            let stored: Value = serde_json::from_str(&text).expect("vectors file is JSON");
            assert_eq!(
                stored, fresh,
                "docs/protocol-vectors.json differs from what the code produces; \
                 rerun with UPDATE_VECTORS=1 if the protocol changed on purpose"
            );
        }
        _ => {
            let mut text = serde_json::to_string_pretty(&fresh).unwrap();
            text.push('\n');
            std::fs::write(path, text).expect("write vectors");
        }
    }
}

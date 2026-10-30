//! The phone link: pairing in all three ways, refusals, and closing on
//! anything malformed.

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use uwupulse_core::proto::{
    b64_decode_32, DeskMsg, ErrorCode, PairingUri, PhoneHandshake, PhoneMsg, PhoneTarget, Src,
    StaticKeypair, WatchAction, WatchState,
};
use uwupulse_core::types::{PairDone, PairRequest};
use uwupulse_core::Event;

use crate::{
    handshake, link, next_msg, phone_id, refused_status, start, wait_for, ws_connect,
    ws_connect_with, Phone,
};

fn desk_spk(env: &crate::Env) -> [u8; 32] {
    b64_decode_32(&env.pulse.identity().1).unwrap()
}

async fn expect_ready(phone: &mut Phone) {
    assert_eq!(phone.recv().await, Some(DeskMsg::Ready));
}

async fn pair_request(events: &mut tokio::sync::broadcast::Receiver<Event>) -> PairRequest {
    wait_for(events, |e| match e {
        Event::PairRequest(r) => Some(r.clone()),
        _ => None,
    })
    .await
}

async fn pair_done(events: &mut tokio::sync::broadcast::Receiver<Event>) -> PairDone {
    wait_for(events, |e| match e {
        Event::PairDone(d) => Some(d.clone()),
        _ => None,
    })
    .await
}

/// A fresh QR offer as the phone sees it.
fn qr_target(env: &crate::Env) -> PhoneTarget {
    let offer = env.pulse.create_pairing().unwrap();
    let uri = PairingUri::parse(&offer.uri).unwrap();
    assert_eq!(uri.port, env.port);
    assert!(uri.hosts.is_empty(), "LAN is off in tests");
    assert!(offer.qr_svg.starts_with("<?xml") || offer.qr_svg.contains("<svg"));
    PhoneTarget::Qr {
        desk_spk: uri.pk,
        secret: uri.secret,
    }
}

/// Pairs a fresh phone via QR; returns its key and ID.
async fn pair_by_qr(env: &crate::Env) -> (StaticKeypair, String, Phone) {
    let key = StaticKeypair::generate();
    let id = phone_id();
    let mut phone = link(env.port, &key, &id, qr_target(env)).await.unwrap();
    let (desk_id, _) = env.pulse.identity();
    assert_eq!(
        phone.recv().await,
        Some(DeskMsg::Paired {
            id: desk_id,
            name: "Test Desk".into()
        })
    );
    expect_ready(&mut phone).await;
    (key, id, phone)
}

#[tokio::test]
async fn qr_pairing_then_known_reconnect() {
    let env = start().await;
    let mut events = env.pulse.subscribe();
    let (key, id, mut phone) = pair_by_qr(&env).await;
    assert_eq!(phone.desk.name, "Test Desk");

    // Paired and connected, as a source.
    wait_for(&mut events, |e| match e {
        Event::Paired(list) => list.iter().any(|d| d.id == id && d.connected).then_some(()),
        _ => None,
    })
    .await;
    assert!(env.pulse.state().sources.iter().any(|s| s.id == id));

    // Heart rates flow; out-of-range ones are dropped without closing.
    for bpm in [5, 300, 87] {
        phone
            .send(&PhoneMsg::Hr {
                ts: 0,
                bpm,
                src: Src::Watch,
                dev: "Apple Watch".into(),
                rr: None,
            })
            .await;
    }
    let s = wait_for(&mut events, |e| match e {
        Event::Hr(s) => Some(s.clone()),
        _ => None,
    })
    .await;
    assert_eq!(s.bpm, 87);
    assert_eq!(s.dev, "Apple Watch");
    // ts 0 is far off; the desktop's clock is used instead.
    assert!(s.ts > 1_600_000_000_000);

    phone.send(&PhoneMsg::Ping).await;
    assert_eq!(phone.recv().await, Some(DeskMsg::Pong));

    phone
        .send(&PhoneMsg::Status {
            watch: WatchState::Running,
            ble: vec![],
            battery: Some(0.5),
        })
        .await;
    wait_for(&mut events, |e| match e {
        Event::State(st) => st
            .sources
            .iter()
            .any(|s| s.detail.as_deref() == Some("Watch: running · battery 50%"))
            .then_some(()),
        _ => None,
    })
    .await;

    // Watch commands reach the phone.
    env.pulse
        .send_watch_command(&id, WatchAction::Start)
        .await
        .unwrap();
    assert_eq!(
        phone.recv().await,
        Some(DeskMsg::Cmd {
            action: WatchAction::Start
        })
    );

    // The QR offer was used up.
    let again = link(
        env.port,
        &StaticKeypair::generate(),
        &phone_id(),
        PhoneTarget::Qr {
            desk_spk: desk_spk(&env),
            secret: [7; 32],
        },
    )
    .await;
    assert_eq!(again.err().as_deref(), Some("no_offer"));

    // A reconnect as a known device replaces the old connection.
    let target = PhoneTarget::Known {
        desk_spk: desk_spk(&env),
    };
    let mut second = link(env.port, &key, &id, target).await.unwrap();
    expect_ready(&mut second).await;
    assert!(phone.closed().await);
    second.send(&PhoneMsg::Ping).await;
    assert_eq!(second.recv().await, Some(DeskMsg::Pong));

    // Forgetting the phone drops it.
    env.pulse.remove_paired(&id).unwrap();
    assert!(second.closed().await);
    assert!(env.pulse.list_paired().is_empty());
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn sas_accept_and_reject() {
    let env = start().await;
    let mut events = env.pulse.subscribe();

    // Accepted: paired, then ready.
    let key = StaticKeypair::generate();
    let id = phone_id();
    let mut phone = link(env.port, &key, &id, PhoneTarget::Sas).await.unwrap();
    let req = wait_for(&mut events, |e| match e {
        Event::PairRequest(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(req.code, phone.keys.sas);
    assert_eq!(req.device_name, "Test iPhone");

    // Only one code comparison at a time.
    let busy = link(
        env.port,
        &StaticKeypair::generate(),
        &phone_id(),
        PhoneTarget::Sas,
    )
    .await;
    assert_eq!(busy.err().as_deref(), Some("busy"));

    assert!(env.pulse.answer_pair_request("nope", true).is_err());
    env.pulse
        .answer_pair_request(&req.request_id, true)
        .unwrap();
    assert!(matches!(phone.recv().await, Some(DeskMsg::Paired { .. })));
    expect_ready(&mut phone).await;
    let done = wait_for(&mut events, |e| match e {
        Event::PairDone(d) => Some(d.clone()),
        _ => None,
    })
    .await;
    assert!(done.ok);
    assert!(env.pulse.list_paired().iter().any(|d| d.id == id));

    // Rejected: an error, then closed, nothing stored.
    let other = phone_id();
    let mut phone2 = link(
        env.port,
        &StaticKeypair::generate(),
        &other,
        PhoneTarget::Sas,
    )
    .await
    .unwrap();
    let req = wait_for(&mut events, |e| match e {
        Event::PairRequest(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    env.pulse
        .answer_pair_request(&req.request_id, false)
        .unwrap();
    assert_eq!(
        phone2.recv().await,
        Some(DeskMsg::Error {
            code: ErrorCode::Rejected
        })
    );
    assert!(phone2.closed().await);
    assert!(!env.pulse.list_paired().iter().any(|d| d.id == other));
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn refusals() {
    let env = start().await;

    // Never paired.
    let target = PhoneTarget::Known {
        desk_spk: desk_spk(&env),
    };
    let r = link(
        env.port,
        &StaticKeypair::generate(),
        &phone_id(),
        target.clone(),
    )
    .await;
    assert_eq!(r.err().as_deref(), Some("unknown_device"));

    // A paired ID with another key.
    let (_, id, _phone) = pair_by_qr(&env).await;
    let r = link(env.port, &StaticKeypair::generate(), &id, target).await;
    assert_eq!(r.err().as_deref(), Some("unknown_device"));

    // No offer open.
    let r = link(
        env.port,
        &StaticKeypair::generate(),
        &phone_id(),
        PhoneTarget::Qr {
            desk_spk: desk_spk(&env),
            secret: [1; 32],
        },
    )
    .await;
    assert_eq!(r.err().as_deref(), Some("no_offer"));

    // Another protocol version gets an answer.
    let mut ws = ws_connect(env.port, "/link").await.unwrap();
    let hello = format!(
        r#"{{"t":"hello","v":2,"id":"{}","name":"x","spk":"{}","epk":"{}","mode":"sas"}}"#,
        phone_id(),
        "A".repeat(43),
        "A".repeat(43)
    );
    ws.send(Message::text(hello)).await.unwrap();
    match next_msg(&mut ws).await {
        Some(Message::Text(t)) => assert_eq!(t.as_str(), r#"{"t":"error","code":"version"}"#),
        other => panic!("{other:?}"),
    }

    // Garbage just closes.
    for junk in [Message::text("{]"), Message::binary(vec![0u8; 64])] {
        let mut ws = ws_connect(env.port, "/link").await.unwrap();
        ws.send(junk).await.unwrap();
        assert!(next_msg(&mut ws).await.is_none());
    }
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn bad_frames_close() {
    let env = start().await;
    let (key, id, mut phone) = pair_by_qr(&env).await;
    let known = || PhoneTarget::Known {
        desk_spk: desk_spk(&env),
    };

    // A frame that doesn't decrypt.
    phone.ws.send(Message::binary(vec![9u8; 40])).await.unwrap();
    assert!(phone.closed().await);

    // Text after the handshake.
    let mut p = link(env.port, &key, &id, known()).await.unwrap();
    expect_ready(&mut p).await;
    p.ws.send(Message::text("{}")).await.unwrap();
    assert!(p.closed().await);

    // A replayed frame (counter mismatch).
    let mut p = link(env.port, &key, &id, known()).await.unwrap();
    expect_ready(&mut p).await;
    let frame = p.cipher.seal_msg(&PhoneMsg::Ping).unwrap();
    p.ws.send(Message::binary(frame.clone())).await.unwrap();
    assert_eq!(p.recv().await, Some(DeskMsg::Pong));
    p.ws.send(Message::binary(frame)).await.unwrap();
    assert!(p.closed().await);

    // Valid JSON that isn't a message.
    let mut p = link(env.port, &key, &id, known()).await.unwrap();
    expect_ready(&mut p).await;
    let frame = p.cipher.seal(b"[1,2,3]").unwrap();
    p.ws.send(Message::binary(frame)).await.unwrap();
    assert!(p.closed().await);

    // Too large.
    let mut p = link(env.port, &key, &id, known()).await.unwrap();
    expect_ready(&mut p).await;
    let _ = p.ws.send(Message::binary(vec![0u8; 20_000])).await;
    assert!(p.closed().await);

    // Flooding: far more than 20 messages a second, so that a burst split
    // across two rate windows still trips the limit under a loaded runner.
    let mut p = link(env.port, &key, &id, known()).await.unwrap();
    expect_ready(&mut p).await;
    for _ in 0..200 {
        let frame = p.cipher.seal_msg(&PhoneMsg::Ping).unwrap();
        if p.ws.send(Message::binary(frame)).await.is_err() {
            break;
        }
    }
    assert!(p.closed().await);
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn sas_needs_both_confirmations() {
    let env = start().await;
    let mut events = env.pulse.subscribe();

    // The request shows up right after the keys, before the phone's `auth`.
    // Accepted here first, then confirmed on the phone: paired.
    let id = phone_id();
    let mut phone = handshake(env.port, &StaticKeypair::generate(), &id, PhoneTarget::Sas)
        .await
        .unwrap();
    let req = pair_request(&mut events).await;
    assert_eq!(req.code, phone.keys.sas);
    env.pulse
        .answer_pair_request(&req.request_id, true)
        .unwrap();
    phone.send(&PhoneMsg::Auth).await;
    assert!(matches!(phone.recv().await, Some(DeskMsg::Paired { .. })));
    expect_ready(&mut phone).await;
    assert!(pair_done(&mut events).await.ok);
    assert!(env.pulse.list_paired().iter().any(|d| d.id == id));

    // Accepted here, but the phone never confirms and leaves: not paired.
    let id = phone_id();
    let phone = handshake(env.port, &StaticKeypair::generate(), &id, PhoneTarget::Sas)
        .await
        .unwrap();
    let req = pair_request(&mut events).await;
    env.pulse
        .answer_pair_request(&req.request_id, true)
        .unwrap();
    drop(phone);
    let done = pair_done(&mut events).await;
    assert_eq!(done.request_id, req.request_id);
    assert!(!done.ok);
    assert!(!env.pulse.list_paired().iter().any(|d| d.id == id));

    // Confirmed on the phone, never accepted here: the phone leaves, not
    // paired either.
    let id = phone_id();
    let mut phone = link(env.port, &StaticKeypair::generate(), &id, PhoneTarget::Sas)
        .await
        .unwrap();
    let req = pair_request(&mut events).await;
    phone.send(&PhoneMsg::Ping).await;
    drop(phone);
    let done = pair_done(&mut events).await;
    assert_eq!(done.request_id, req.request_id);
    assert!(!done.ok);
    assert!(!env.pulse.list_paired().iter().any(|d| d.id == id));
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn sas_commitment_mismatch_is_rejected() {
    let env = start().await;
    let mut events = env.pulse.subscribe();

    let mut ws = ws_connect(env.port, "/link").await.unwrap();
    let hs = PhoneHandshake::new(
        &StaticKeypair::generate(),
        &phone_id(),
        "Test iPhone",
        PhoneTarget::Sas,
    );
    ws.send(Message::text(hs.m1())).await.unwrap();
    assert!(matches!(next_msg(&mut ws).await, Some(Message::Text(_))));
    // A key other than the committed one.
    let other = StaticKeypair::generate().public_b64();
    ws.send(Message::text(format!(
        r#"{{"t":"reveal","epk":"{other}"}}"#
    )))
    .await
    .unwrap();
    match next_msg(&mut ws).await {
        Some(Message::Text(t)) => assert_eq!(t.as_str(), r#"{"t":"error","code":"rejected"}"#),
        other => panic!("{other:?}"),
    }
    assert!(next_msg(&mut ws).await.is_none());

    // The SAS slot is free again, and the failed attempt never asked the
    // user: the first request is the honest phone's.
    let phone = link(
        env.port,
        &StaticKeypair::generate(),
        &phone_id(),
        PhoneTarget::Sas,
    )
    .await
    .unwrap();
    assert_eq!(pair_request(&mut events).await.code, phone.keys.sas);
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn paired_id_with_other_key_is_rejected() {
    let env = start().await;
    let mut events = env.pulse.subscribe();
    let (key, id, _phone) = pair_by_qr(&env).await;

    // QR: same ID, another key.
    let mut p = link(env.port, &StaticKeypair::generate(), &id, qr_target(&env))
        .await
        .unwrap();
    assert_eq!(
        p.recv().await,
        Some(DeskMsg::Error {
            code: ErrorCode::Rejected
        })
    );
    assert!(p.closed().await);

    // SAS: same, even when the user accepts.
    let mut p = link(env.port, &StaticKeypair::generate(), &id, PhoneTarget::Sas)
        .await
        .unwrap();
    let req = pair_request(&mut events).await;
    env.pulse
        .answer_pair_request(&req.request_id, true)
        .unwrap();
    assert_eq!(
        p.recv().await,
        Some(DeskMsg::Error {
            code: ErrorCode::Rejected
        })
    );
    assert!(!pair_done(&mut events).await.ok);

    // The old pairing stays, and pairing again with the same key is fine.
    let known = PhoneTarget::Known {
        desk_spk: desk_spk(&env),
    };
    let mut p = link(env.port, &key, &id, known).await.unwrap();
    expect_ready(&mut p).await;
    let mut p = link(env.port, &key, &id, qr_target(&env)).await.unwrap();
    assert!(matches!(p.recv().await, Some(DeskMsg::Paired { .. })));
    expect_ready(&mut p).await;
    assert_eq!(env.pulse.list_paired().len(), 1);
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn browser_origin_is_forbidden() {
    let env = start().await;
    let r = ws_connect_with(env.port, "/link", &[("Origin", "http://example.com")]).await;
    assert_eq!(refused_status(r), Some(403));
    env.pulse.shutdown().await;
}

#[tokio::test]
async fn pre_ready_connections_per_address_are_limited() {
    let env = start().await;
    // One connection that never says hello holds a slot …
    let idle = ws_connect(env.port, "/link").await.unwrap();
    // … a second one gives its slot back once it is `ready` …
    let (_, _, _phone) = pair_by_qr(&env).await;
    let idle2 = ws_connect(env.port, "/link").await.unwrap();
    // … and a third one before `ready` from the same address is too many.
    assert_eq!(
        refused_status(ws_connect(env.port, "/link").await),
        Some(503)
    );
    drop((idle, idle2));
    env.pulse.shutdown().await;
}

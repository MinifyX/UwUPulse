import CryptoKit
import Foundation
import Testing
@testable import PulseKit

@Suite struct EncodingTests {
    @Test func b64RoundTrip() {
        for n in 0..<40 {
            let data = Data((0..<n).map { UInt8(truncatingIfNeeded: $0 &* 37 &+ 250) })
            let s = B64.encode(data)
            #expect(!s.contains("=") && !s.contains("+") && !s.contains("/"))
            #expect(B64.decode(s) == data)
        }
    }

    @Test func b64RejectsNonURLAlphabet() {
        #expect(B64.decode("ab+c") == nil)
        #expect(B64.decode("abc=") == nil)
        #expect(B64.decode("a") == nil)
        #expect(B64.decode("ab c") == nil)
    }

    @Test func hex() {
        #expect(Data(hex: "00ffA0")?.hexString == "00ffa0")
        #expect(Data(hex: "0") == nil)
        #expect(Data(hex: "zz") == nil)
    }
}

@Suite struct HandshakeTests {
    let identity = LinkIdentity.generate()
    let desktop = TestDesktop()

    @Test func knownRoundTrip() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: "Test iPhone", mode: .known, expectedDesktopKey: desktop.publicKey)
        var accepted = try desktop.respond(to: phone.m1, knownPhoneKeys: [identity.publicKey])
        var session = try phone.finish(m2: accepted.m2)

        #expect(session.sas == accepted.session.sas)
        #expect(session.sas.count == 6)
        #expect(session.desktopId == desktop.id)
        #expect(accepted.phone.name == "Test iPhone")
        #expect(accepted.phone.mode == .known)

        // Phone → desktop, several frames (counter advances).
        for bpm in [60, 61, 62] {
            let hr = LinkMessage.hr(HeartRateMessage(ts: 1_700_000_000_000, bpm: bpm, src: .watch, dev: "Watch"))
            let frame = try session.seal(hr)
            let opened = try accepted.session.open(frame)
            #expect(opened == hr)
        }
        // Desktop → phone.
        let ready = try accepted.session.seal(.ready)
        let readyOpened = try session.open(ready)
        #expect(readyOpened == .ready)
        let cmd = try accepted.session.seal(.cmd(.start))
        let cmdOpened = try session.open(cmd)
        #expect(cmdOpened == .cmd(.start))
    }

    @Test func qrUsesPSK() throws {
        let secret = Data((0..<32).map { UInt8($0) })
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .qr, expectedDesktopKey: desktop.publicKey, expectedDesktopId: desktop.id, psk: secret)
        let good = try desktop.respond(to: phone.m1, psk: secret)
        #expect(try phone.finish(m2: good.m2).sas == good.session.sas)

        // Desktop with a different secret derives different keys: the first frame fails.
        let phone2 = try PhoneHandshake(identity: identity, deviceName: "P", mode: .qr, expectedDesktopKey: desktop.publicKey, expectedDesktopId: desktop.id, psk: secret)
        var bad = try desktop.respond(to: phone2.m1, psk: LinkProtocol.zeroPSK)
        var session = try phone2.finish(m2: bad.m2)
        let auth = try session.seal(.auth)
        #expect(throws: LinkCryptoError.decryptionFailed) { try bad.session.open(auth) }
    }

    @Test func qrRequiresDesktopId() throws {
        let secret = Data(repeating: 7, count: 32)
        #expect(throws: HandshakeError.idMismatch) {
            try PhoneHandshake(identity: identity, deviceName: "P", mode: .qr, expectedDesktopKey: desktop.publicKey, psk: secret)
        }
        let phone = try PhoneHandshake(
            identity: identity, deviceName: "P", mode: .qr, expectedDesktopKey: desktop.publicKey,
            expectedDesktopId: "5c0ffee0-0000-4000-8000-0000000000ff", psk: secret
        )
        let accepted = try desktop.respond(to: phone.m1, psk: secret)
        #expect(throws: HandshakeError.idMismatch) { try phone.finish(m2: accepted.m2) }
    }

    @Test func sasAcceptsAnyKey() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .sas, expectedDesktopKey: nil)
        let accepted = try desktop.respond(to: phone.m1, m3: phone.m3)
        let session = try phone.finish(m2: accepted.m2)
        #expect(session.sas == accepted.session.sas)
        #expect(session.desktopKey == desktop.publicKey)
    }

    @Test func sasCommitment() throws {
        let ephemeral = Curve25519.KeyAgreement.PrivateKey()
        let epk = ephemeral.publicKey.rawRepresentation
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .sas, expectedDesktopKey: nil, ephemeral: ephemeral)
        let json = try #require(JSONSerialization.jsonObject(with: phone.m1) as? [String: Any])
        #expect(json["epk"] == nil, "sas m1 must not reveal epk")
        var expected = Data("UwUPulse-commit-v1".utf8)
        expected.append(epk)
        let commit = Data(SHA256.hash(data: expected))
        #expect(B64.decode(json["commit"] as? String ?? "") == commit)
        #expect(LinkKeySchedule.commitment(ephemeralPublicKey: epk) == commit)

        // m3 reveals exactly the committed key.
        let m3 = try #require(phone.m3)
        let reveal = try #require(JSONSerialization.jsonObject(with: m3) as? [String: Any])
        #expect(reveal["t"] as? String == "reveal")
        #expect(B64.decode(reveal["epk"] as? String ?? "") == epk)
        #expect(Set(reveal.keys) == ["t", "epk"])

        // A desktop seeing a different key in m3 rejects it.
        let other = try PhoneHandshake(identity: identity, deviceName: "P", mode: .sas, expectedDesktopKey: nil)
        #expect(throws: HandshakeError.desktop(.rejected)) { try desktop.respond(to: phone.m1, m3: other.m3) }

        // known/qr: epk in m1, no commitment, no m3.
        let known = try PhoneHandshake(identity: identity, deviceName: "P", mode: .known, expectedDesktopKey: desktop.publicKey)
        let knownJSON = try #require(JSONSerialization.jsonObject(with: known.m1) as? [String: Any])
        #expect(knownJSON["commit"] == nil && knownJSON["epk"] is String && known.m3 == nil)
    }

    @Test func sasTranscriptIncludesM3() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .sas, expectedDesktopKey: nil)
        let accepted = try desktop.respond(to: phone.m1, m3: phone.m3)
        let session = try phone.finish(m2: accepted.m2)
        let m3 = try #require(phone.m3)
        var preimage = Data("UwUPulse-link-v1".utf8)
        preimage.append(phone.m1); preimage.append(accepted.m2); preimage.append(m3)
        #expect(session.transcriptHash == Data(SHA256.hash(data: preimage)))
        #expect(session.transcriptHash == LinkKeySchedule.transcriptHash(m1: phone.m1, m2: accepted.m2, m3: m3))
        #expect(session.transcriptHash != LinkKeySchedule.transcriptHash(m1: phone.m1, m2: accepted.m2))
    }

    @Test func keyMismatchAborts() throws {
        let other = TestDesktop()
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .known, expectedDesktopKey: other.publicKey)
        let accepted = try desktop.respond(to: phone.m1, knownPhoneKeys: [identity.publicKey])
        #expect(throws: HandshakeError.keyMismatch) { try phone.finish(m2: accepted.m2) }
    }

    @Test func knownRequiresKey() {
        #expect(throws: HandshakeError.invalidKey) {
            try PhoneHandshake(identity: identity, deviceName: "P", mode: .known, expectedDesktopKey: nil)
        }
    }

    @Test func desktopErrorFrame() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .known, expectedDesktopKey: desktop.publicKey)
        #expect(throws: HandshakeError.desktop(.unknownDevice)) {
            try phone.finish(m2: Data(#"{"t":"error","code":"unknown_device"}"#.utf8))
        }
        #expect(throws: HandshakeError.malformed) { try phone.finish(m2: Data("garbage".utf8)) }
        #expect(throws: HandshakeError.unsupportedVersion) {
            try phone.finish(m2: Data(#"{"t":"hello","v":2,"id":"x","name":"x","spk":"AA","epk":"AA"}"#.utf8))
        }
        #expect(throws: HandshakeError.invalidKey) {
            try phone.finish(m2: Data(#"{"t":"hello","v":1,"id":"x","name":"x","spk":"AA","epk":"AA"}"#.utf8))
        }
    }

    @Test func m1Shape() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: String(repeating: "x", count: 100), mode: .sas, expectedDesktopKey: nil)
        let json = try #require(JSONSerialization.jsonObject(with: phone.m1) as? [String: Any])
        #expect(json["t"] as? String == "hello")
        #expect(json["v"] as? Int == 1)
        #expect(json["mode"] as? String == "sas")
        #expect(json["id"] as? String == identity.id)
        #expect((json["name"] as? String)?.count == 64)
        #expect(B64.decode(json["spk"] as? String ?? "") == identity.publicKey)
    }

    @Test func tamperReplayAndSize() throws {
        let phone = try PhoneHandshake(identity: identity, deviceName: "P", mode: .sas, expectedDesktopKey: nil)
        var accepted = try desktop.respond(to: phone.m1, m3: phone.m3)
        var session = try phone.finish(m2: accepted.m2)
        var frame = try session.seal(.ping)
        frame[frame.startIndex] ^= 1
        #expect(throws: LinkCryptoError.decryptionFailed) { try accepted.session.open(frame) }

        // Replay of an earlier frame fails because the counter moved on.
        let first = try accepted.session.seal(.pong)
        let firstOpened = try session.open(first)
        #expect(firstOpened == .pong)
        #expect(throws: LinkCryptoError.decryptionFailed) { try session.open(first) }

        #expect(throws: LinkCryptoError.frameTooLarge) { try session.open(Data(count: LinkProtocol.maxFrameBytes + 1)) }
        #expect(throws: LinkCryptoError.decryptionFailed) { try session.open(Data(count: 3)) }
        var cipher = FrameCipher(key: SymmetricKey(size: .bits256))
        #expect(throws: LinkCryptoError.frameTooLarge) { try cipher.seal(Data(count: LinkProtocol.maxFrameBytes)) }
    }

    @Test func nonceLayout() {
        #expect(FrameCipher.nonce(counter: 0) == Data(count: 12))
        #expect(FrameCipher.nonce(counter: 0x0102).hexString == "000000000201000000000000")
    }

    @Test func sasFormatting() {
        #expect(LinkKeySchedule.formatSAS(42) == "000042")
        #expect(LinkKeySchedule.formatSAS(999_999) == "999999")
        #expect(LinkKeySchedule.formatSAS(1_000_007) == "000007")
    }
}

@Suite struct MessageTests {
    @Test func statusEncodesNullBattery() throws {
        let data = try LinkMessage.status(StatusMessage(watch: .idle, ble: [], battery: nil)).jsonData()
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["battery"] is NSNull)
        #expect(try LinkMessage.decode(data) == .status(StatusMessage(watch: .idle, ble: [], battery: nil)))
    }

    @Test func roundTrips() throws {
        let messages: [LinkMessage] = [
            .auth, .ready, .ping, .reveal(epk: "AAAA"), .pong, .cmd(.stop), .error(.rejected), .paired(id: "a", name: "b"),
            .hr(HeartRateMessage(ts: 1, bpm: 80, src: .ble, dev: "Strap", rr: [800, 810])),
            .status(StatusMessage(watch: .running, ble: ["Strap"], battery: 0.5)),
        ]
        for m in messages { #expect(try LinkMessage.decode(m.jsonData()) == m) }
        #expect(try LinkMessage.decode(Data(#"{"t":"future"}"#.utf8)) == .unknown("future"))
    }

    @Test func workoutChannel() {
        let hr = WorkoutChannelMessage.heartRate(WatchHeartRate(bpm: 72, ts: 5))
        #expect(WorkoutChannelMessage.decode(hr.encoded()) == hr)
        #expect(WorkoutChannelMessage.decode(WorkoutChannelMessage.command(.stop).encoded()) == .command(.stop))
        #expect(WorkoutChannelMessage.decode(Data("{}".utf8)) == nil)
    }

    @Test func limits() {
        #expect(LinkLimits.isValidBPM(20) && LinkLimits.isValidBPM(250))
        #expect(!LinkLimits.isValidBPM(19) && !LinkLimits.isValidBPM(251))
        #expect(String(repeating: "ä", count: 70).limited().count == 64)
    }

    @Test func backoffAndRateLimit() {
        var b = Backoff()
        let first = [b.next(jitter: 1), b.next(jitter: 1), b.next(jitter: 1)]
        #expect(first == [1, 2, 4])
        for _ in 0..<10 { _ = b.next(jitter: 1) }
        let capped = b.next(jitter: 1)
        #expect(capped == 30)
        var r = RateLimiter(limit: 2)
        let results = [r.allow(now: 0), r.allow(now: 0.5), r.allow(now: 0.9), r.allow(now: 1.0)]
        #expect(results == [true, true, false, true])
    }
}

@Suite struct PairingURITests {
    let pk = B64.encode(Data(repeating: 1, count: 32))
    let s = B64.encode(Data(repeating: 2, count: 32))

    @Test func parsesValidURI() throws {
        let offer = try PairingOffer(uri: "uwupulse://pair?v=1&id=desk-1&n=Mini%27s+Mac%20Studio&pk=\(pk)&s=\(s)&p=8746&h=192.168.1.10,fe80::1%25en0,desk.local")
        #expect(offer.desktopId == "desk-1")
        #expect(offer.name == "Mini's Mac Studio")
        #expect(offer.publicKey == Data(repeating: 1, count: 32))
        #expect(offer.secret == Data(repeating: 2, count: 32))
        #expect(offer.port == 8746)
        #expect(offer.hosts == ["192.168.1.10", "fe80::1%en0", "desk.local"])
        // No hosts at all (LAN off on the desktop) is fine: the app falls back to Bonjour.
        #expect(try PairingOffer(uri: "uwupulse://pair?v=1&id=a&pk=\(pk)&s=\(s)&p=1").hosts.isEmpty)
    }

    @Test func allowedHosts() {
        let allowed = [
            "10.0.0.5", "172.16.0.1", "172.31.255.254", "192.168.1.10", "169.254.3.4", "127.0.0.1",
            "fe80::1", "FE80::abcd:1%en0", "febf::1", "fd12:3456:789a::1", "fc00::1", "::1",
            "::ffff:192.168.1.10", "desk.local", "Studio-Mac.local", "a.b.local.",
        ]
        for host in allowed { #expect(PairingOffer.isAllowedHost(host), "\(host) should be allowed") }
        let rejected = [
            "203.0.113.5", "172.32.0.1", "172.15.0.1", "11.0.0.1", "192.169.0.1", "0.0.0.0", "255.255.255.255",
            "2001:db8::1", "fec0::1", "::", "::ffff:203.0.113.5", "203.0.113.5%en0", "fd00::1%en0",
            "example.com", "local", ".local", "desk.local.example.com", "desk-.local", "de sk.local",
            "desk.local/x", "desk.local:80", "user@desk.local", "10.0.0.5:80", "010.0.0.5", "10.0.0", "",
        ]
        for host in rejected { #expect(!PairingOffer.isAllowedHost(host), "\(host) should be rejected") }
    }

    @Test func rejectsPublicHosts() {
        for h in ["203.0.113.5", "example.com", "192.168.1.10,203.0.113.5", "bad/host"] {
            #expect(throws: PairingOffer.ParseError.invalid("h"), "\(h)") {
                try PairingOffer(uri: "uwupulse://pair?v=1&id=a&pk=\(pk)&s=\(s)&p=8746&h=\(h)")
            }
        }
    }

    @Test func rejectsInvalid() {
        #expect(throws: PairingOffer.ParseError.notAPairingURI) { try PairingOffer(uri: "https://example.com/pair?v=1") }
        #expect(throws: PairingOffer.ParseError.unsupportedVersion) { try PairingOffer(uri: "uwupulse://pair?v=2&id=a&pk=\(pk)&s=\(s)&p=1") }
        #expect(throws: PairingOffer.ParseError.missing("id")) { try PairingOffer(uri: "uwupulse://pair?v=1&pk=\(pk)&s=\(s)&p=1") }
        #expect(throws: PairingOffer.ParseError.invalid("pk")) { try PairingOffer(uri: "uwupulse://pair?v=1&id=a&pk=AAAA&s=\(s)&p=1") }
        #expect(throws: PairingOffer.ParseError.invalid("p")) { try PairingOffer(uri: "uwupulse://pair?v=1&id=a&pk=\(pk)&s=\(s)&p=70000") }
        #expect(throws: PairingOffer.ParseError.invalid("s")) { try PairingOffer(uri: "uwupulse://pair?v=1&id=a&pk=\(pk)&s=\(s)%zz&p=1") }
    }
}

@Suite struct HeartRateMeasurementTests {
    @Test func uint8() {
        let m = HeartRateMeasurement(data: Data([0x00, 72]))
        #expect(m?.bpm == 72)
        #expect(m?.sensorContact == nil)
        #expect(m?.rrIntervals == [])
    }

    @Test func uint16WithContactEnergyAndRR() {
        // flags: 16-bit HR, contact supported + detected, energy, RR
        let m = HeartRateMeasurement(data: Data([0x1F, 0x2C, 0x01, 0x10, 0x00, 0x00, 0x04, 0x00, 0x02]))
        #expect(m?.bpm == 300)
        #expect(m?.sensorContact == true)
        #expect(m?.energyExpended == 16)
        #expect(m?.rrIntervals == [1000, 500])
    }

    @Test func malformed() {
        #expect(HeartRateMeasurement(data: Data()) == nil)
        #expect(HeartRateMeasurement(data: Data([0x00])) == nil)
        #expect(HeartRateMeasurement(data: Data([0x01, 0x50])) == nil)
        #expect(HeartRateMeasurement(data: Data([0x08, 0x50, 0x01])) == nil)
        // Odd trailing RR byte is ignored.
        #expect(HeartRateMeasurement(data: Data([0x10, 0x50, 0x00, 0x04, 0x07]))?.rrIntervals == [1000])
    }

    @Test func fuzzNeverTraps() {
        var rng = SystemRandomNumberGenerator()
        for _ in 0..<20_000 {
            let n = Int.random(in: 0...24, using: &rng)
            _ = HeartRateMeasurement(data: Data((0..<n).map { _ in UInt8.random(in: 0...255, using: &rng) }))
        }
        // Slices with a non-zero start index.
        let big = Data([0xFF, 0x1F, 0x50, 0x00])
        _ = HeartRateMeasurement(data: big.dropFirst())
    }
}

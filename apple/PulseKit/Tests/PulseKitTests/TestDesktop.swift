import CryptoKit
import Foundation
@testable import PulseKit

/// Test-only desktop role of the link handshake, mirroring what the Rust core does.
struct TestDesktop {
    let id = "5c0ffee0-0000-4000-8000-000000000001"
    let name = "Test Desktop"
    let staticKey: Curve25519.KeyAgreement.PrivateKey

    init(staticKey: Curve25519.KeyAgreement.PrivateKey = .init()) { self.staticKey = staticKey }

    var publicKey: Data { staticKey.publicKey.rawRepresentation }

    struct Accepted {
        let m2: Data
        let phone: HelloMessage
        var session: DesktopSession
    }

    /// Answers m1 like the desktop would. `knownPhoneKeys` emulates the pairing store.
    /// In `sas` mode `m3` (the phone's `reveal`) is required and checked against the commitment.
    func respond(
        to m1: Data, m3: Data? = nil, psk: Data = LinkProtocol.zeroPSK, knownPhoneKeys: [Data] = [],
        ephemeral: Curve25519.KeyAgreement.PrivateKey = .init()
    ) throws -> Accepted {
        guard case .hello(let hello) = try LinkMessage.decode(m1), hello.v == 1 else {
            throw HandshakeError.unsupportedVersion
        }
        guard let spk = B64.decode(hello.spk) else { throw HandshakeError.invalidKey }
        let epk: Data
        if hello.mode == .sas {
            guard hello.epk == nil, let commit = hello.commit.flatMap(B64.decode),
                  let m3, case .reveal(let revealed) = try LinkMessage.decode(m3),
                  let key = B64.decode(revealed),
                  LinkKeySchedule.commitment(ephemeralPublicKey: key) == commit
            else { throw HandshakeError.desktop(.rejected) }
            epk = key
        } else {
            guard let key = hello.epk.flatMap(B64.decode) else { throw HandshakeError.invalidKey }
            epk = key
        }
        if hello.mode == .known, !knownPhoneKeys.contains(spk) {
            throw HandshakeError.desktop(.unknownDevice)
        }
        let m2 = try LinkMessage.hello(HelloMessage(
            id: id, name: name, spk: B64.encode(publicKey),
            epk: B64.encode(ephemeral.publicKey.rawRepresentation), mode: nil
        )).jsonData()
        let keys = try LinkKeySchedule.derive(
            ownStatic: staticKey, ownEphemeral: ephemeral,
            peerStatic: try .init(rawRepresentation: spk), peerEphemeral: try .init(rawRepresentation: epk),
            psk: psk, m1: m1, m2: m2, m3: hello.mode == .sas ? m3 : nil
        )
        return Accepted(m2: m2, phone: hello, session: DesktopSession(keys: keys))
    }
}

/// Desktop direction: receives with k_p2d, sends with k_d2p.
struct DesktopSession {
    let sas: String
    var receiver: FrameCipher
    var sender: FrameCipher

    init(keys: LinkSessionKeys) {
        sas = keys.sas
        receiver = FrameCipher(key: keys.phoneToDesktop)
        sender = FrameCipher(key: keys.desktopToPhone)
    }

    mutating func open(_ frame: Data) throws -> LinkMessage { try LinkMessage.decode(receiver.open(frame)) }
    mutating func seal(_ message: LinkMessage) throws -> Data { try sender.seal(message.jsonData()) }
}

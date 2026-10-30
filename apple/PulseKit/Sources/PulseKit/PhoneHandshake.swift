import CryptoKit
import Foundation

/// Static identity of this phone: device id (lowercase UUID v4) and X25519 key pair.
public struct LinkIdentity: Sendable {
    public let id: String
    public let privateKey: Curve25519.KeyAgreement.PrivateKey

    public init(id: String, privateKey: Curve25519.KeyAgreement.PrivateKey) {
        self.id = id
        self.privateKey = privateKey
    }

    public static func generate() -> LinkIdentity {
        LinkIdentity(id: UUID().uuidString.lowercased(), privateKey: .init())
    }

    public var publicKey: Data { privateKey.publicKey.rawRepresentation }
}

public enum HandshakeError: Error, Equatable, Sendable {
    case desktop(LinkErrorCode)
    case malformed
    case unsupportedVersion
    case keyMismatch
    /// `qr`: m2 carries a different desktop id than the QR code.
    case idMismatch
    case invalidKey
    case invalidPSK
}

/// Phone side of the link handshake (steps 1–5). Pure: the caller does the I/O.
///
/// Order on the wire: send `m1`, receive m2 and call `finish(m2:)`, then in `sas` mode send `m3`
/// (only after `finish` succeeded) before any encrypted frame.
public struct PhoneHandshake: Sendable {
    public let mode: HandshakeMode
    /// Exact bytes of m1, to be sent as a text frame.
    public let m1: Data
    /// `sas` only: exact bytes of m3 (`reveal`), to be sent as a text frame after m2 arrived.
    public let m3: Data?
    private let identity: LinkIdentity
    private let ephemeral: Curve25519.KeyAgreement.PrivateKey
    private let expectedDesktopKey: Data?
    private let expectedDesktopId: String?
    private let psk: Data

    /// - Parameters:
    ///   - expectedDesktopKey: required for `known` (stored key) and `qr` (QR `pk`); nil for `sas`.
    ///   - expectedDesktopId: `qr`: the desktop id from the QR code (required); m2 must carry it.
    ///   - psk: QR secret for `qr`, otherwise 32 zero bytes.
    public init(
        identity: LinkIdentity,
        deviceName: String,
        mode: HandshakeMode,
        expectedDesktopKey: Data?,
        expectedDesktopId: String? = nil,
        psk: Data = LinkProtocol.zeroPSK,
        ephemeral: Curve25519.KeyAgreement.PrivateKey = .init()
    ) throws {
        guard psk.count == 32 else { throw HandshakeError.invalidPSK }
        if mode != .sas {
            guard let key = expectedDesktopKey, key.count == 32 else { throw HandshakeError.invalidKey }
        }
        if mode == .qr {
            guard let id = expectedDesktopId, !id.isEmpty else { throw HandshakeError.idMismatch }
        }
        self.mode = mode
        self.identity = identity
        self.ephemeral = ephemeral
        self.expectedDesktopKey = expectedDesktopKey
        self.expectedDesktopId = expectedDesktopId
        self.psk = psk
        let epk = ephemeral.publicKey.rawRepresentation
        let hello: HelloMessage
        if mode == .sas {
            // Commit to the ephemeral key so a man in the middle cannot grind m2 for a matching code.
            hello = HelloMessage(
                id: identity.id,
                name: deviceName.limited(),
                spk: B64.encode(identity.publicKey),
                epk: nil,
                commit: B64.encode(LinkKeySchedule.commitment(ephemeralPublicKey: epk)),
                mode: mode
            )
            self.m3 = try LinkMessage.reveal(epk: B64.encode(epk)).jsonData()
        } else {
            hello = HelloMessage(
                id: identity.id,
                name: deviceName.limited(),
                spk: B64.encode(identity.publicKey),
                epk: B64.encode(epk),
                mode: mode
            )
            self.m3 = nil
        }
        self.m1 = try LinkMessage.hello(hello).jsonData()
    }

    public var m1String: String { String(decoding: m1, as: UTF8.self) }
    public var m3String: String? { m3.map { String(decoding: $0, as: UTF8.self) } }

    /// Processes the desktop's reply (m2, exact bytes received) and derives the session.
    public func finish(m2: Data) throws -> LinkSession {
        let message: LinkMessage
        do { message = try LinkMessage.decode(m2) } catch { throw HandshakeError.malformed }
        let hello: HelloMessage
        switch message {
        case .hello(let h): hello = h
        case .error(let code): throw HandshakeError.desktop(code)
        default: throw HandshakeError.malformed
        }
        guard hello.v == LinkProtocol.version else { throw HandshakeError.unsupportedVersion }
        guard let spk = B64.decode(hello.spk), spk.count == 32,
              let epk = hello.epk.flatMap(B64.decode), epk.count == 32,
              let peerStatic = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: spk),
              let peerEphemeral = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: epk)
        else { throw HandshakeError.invalidKey }
        if mode != .sas {
            guard let expected = expectedDesktopKey, constantTimeEqual(expected, spk) else {
                throw HandshakeError.keyMismatch
            }
        }
        if mode == .qr, hello.id != expectedDesktopId { throw HandshakeError.idMismatch }
        let keys: LinkSessionKeys
        do {
            keys = try LinkKeySchedule.derive(
                ownStatic: identity.privateKey, ownEphemeral: ephemeral,
                peerStatic: peerStatic, peerEphemeral: peerEphemeral,
                psk: psk, m1: m1, m2: m2, m3: m3
            )
        } catch { throw HandshakeError.invalidKey }
        return LinkSession(
            desktopId: hello.id.limited(),
            desktopName: hello.name.limited(),
            desktopKey: spk,
            keys: keys
        )
    }
}

/// Encrypted session after the handshake (phone role: sends with k_p2d, receives with k_d2p).
public struct LinkSession: Sendable {
    public let desktopId: String
    public let desktopName: String
    public let desktopKey: Data
    public let sas: String
    public let transcriptHash: Data
    private var sender: FrameCipher
    private var receiver: FrameCipher

    public init(desktopId: String, desktopName: String, desktopKey: Data, keys: LinkSessionKeys) {
        self.desktopId = desktopId
        self.desktopName = desktopName
        self.desktopKey = desktopKey
        self.sas = keys.sas
        self.transcriptHash = keys.transcriptHash
        self.sender = FrameCipher(key: keys.phoneToDesktop)
        self.receiver = FrameCipher(key: keys.desktopToPhone)
    }

    public mutating func seal(_ message: LinkMessage) throws -> Data {
        try sender.seal(message.jsonData())
    }

    public mutating func open(_ frame: Data) throws -> LinkMessage {
        let plaintext = try receiver.open(frame)
        do { return try LinkMessage.decode(plaintext) } catch { throw HandshakeError.malformed }
    }
}

func constantTimeEqual(_ a: Data, _ b: Data) -> Bool {
    guard a.count == b.count else { return false }
    var diff: UInt8 = 0
    for (x, y) in zip(a, b) { diff |= x ^ y }
    return diff == 0
}

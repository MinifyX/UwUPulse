import CryptoKit
import Foundation

/// Constants of link protocol version 1.
public enum LinkProtocol {
    public static let version = 1
    public static let transcriptLabel = Data("UwUPulse-link-v1".utf8)
    public static let commitLabel = Data("UwUPulse-commit-v1".utf8)
    public static let hkdfInfo = Data("uwupulse link keys".utf8)
    public static let maxFrameBytes = 16 * 1024
    public static let pingInterval: TimeInterval = 10
    public static let zeroPSK = Data(count: 32)
    public static let bonjourType = "_uwupulse._tcp"
    public static let defaultPort = 8746
    public static let path = "/link"
}

public enum LinkCryptoError: Error, Equatable, Sendable {
    case invalidKey
    case invalidPSK
    case frameTooLarge
    case decryptionFailed
    case counterExhausted
}

/// Output of the key schedule (step 5 of the handshake).
public struct LinkSessionKeys: Sendable, Equatable {
    public let transcriptHash: Data
    public let phoneToDesktop: SymmetricKey
    public let desktopToPhone: SymmetricKey
    /// 6-digit comparison code with leading zeros.
    public let sas: String

    public static func == (a: Self, b: Self) -> Bool {
        a.transcriptHash == b.transcriptHash && a.sas == b.sas
            && a.phoneToDesktop.withUnsafeBytes { Data($0) } == b.phoneToDesktop.withUnsafeBytes { Data($0) }
            && a.desktopToPhone.withUnsafeBytes { Data($0) } == b.desktopToPhone.withUnsafeBytes { Data($0) }
    }
}

public enum LinkKeySchedule {
    /// th = SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2), in `sas` mode SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2 ‖ m3).
    public static func transcriptHash(m1: Data, m2: Data, m3: Data? = nil) -> Data {
        var hasher = SHA256()
        hasher.update(data: LinkProtocol.transcriptLabel)
        hasher.update(data: m1)
        hasher.update(data: m2)
        if let m3 { hasher.update(data: m3) }
        return Data(hasher.finalize())
    }

    /// SAS commitment: SHA-256("UwUPulse-commit-v1" ‖ epk_phone).
    public static func commitment(ephemeralPublicKey: Data) -> Data {
        var hasher = SHA256()
        hasher.update(data: LinkProtocol.commitLabel)
        hasher.update(data: ephemeralPublicKey)
        return Data(hasher.finalize())
    }

    /// Derives the session keys from the two DH results.
    /// `ephemeralShared` = X25519(e_phone, e_desk), `staticShared` = X25519(s_phone, s_desk).
    public static func derive(
        ephemeralShared: Data, staticShared: Data, psk: Data, m1: Data, m2: Data, m3: Data? = nil
    ) throws -> LinkSessionKeys {
        guard ephemeralShared.count == 32, staticShared.count == 32 else { throw LinkCryptoError.invalidKey }
        guard psk.count == 32 else { throw LinkCryptoError.invalidPSK }
        let th = transcriptHash(m1: m1, m2: m2, m3: m3)
        let ikm = ephemeralShared + staticShared + psk
        let okm = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: ikm),
            salt: th,
            info: LinkProtocol.hkdfInfo,
            outputByteCount: 68
        ).withUnsafeBytes { Data($0) }
        let code = okm[okm.startIndex + 64 ..< okm.startIndex + 68].reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
        return LinkSessionKeys(
            transcriptHash: th,
            phoneToDesktop: SymmetricKey(data: okm[okm.startIndex ..< okm.startIndex + 32]),
            desktopToPhone: SymmetricKey(data: okm[okm.startIndex + 32 ..< okm.startIndex + 64]),
            sas: formatSAS(code % 1_000_000)
        )
    }

    /// Convenience for either role: both DH values are symmetric, so the caller passes its own
    /// private keys and the peer's public keys.
    public static func derive(
        ownStatic: Curve25519.KeyAgreement.PrivateKey,
        ownEphemeral: Curve25519.KeyAgreement.PrivateKey,
        peerStatic: Curve25519.KeyAgreement.PublicKey,
        peerEphemeral: Curve25519.KeyAgreement.PublicKey,
        psk: Data, m1: Data, m2: Data, m3: Data? = nil
    ) throws -> LinkSessionKeys {
        let ee = try ownEphemeral.sharedSecretFromKeyAgreement(with: peerEphemeral).withUnsafeBytes { Data($0) }
        let ss = try ownStatic.sharedSecretFromKeyAgreement(with: peerStatic).withUnsafeBytes { Data($0) }
        // Reject all-zero DH output (low-order peer key).
        guard ee.contains(where: { $0 != 0 }), ss.contains(where: { $0 != 0 }) else { throw LinkCryptoError.invalidKey }
        return try derive(ephemeralShared: ee, staticShared: ss, psk: psk, m1: m1, m2: m2, m3: m3)
    }

    public static func formatSAS(_ value: UInt32) -> String {
        let s = String(value % 1_000_000)
        return String(repeating: "0", count: 6 - s.count) + s
    }
}

/// One direction of the encrypted frame stream. Nonce = 4 zero bytes ‖ u64_le(counter), empty AAD.
/// Wire format of a frame: ciphertext ‖ 16-byte tag.
public struct FrameCipher: Sendable {
    private let key: SymmetricKey
    public private(set) var counter: UInt64 = 0

    public init(key: SymmetricKey, counter: UInt64 = 0) {
        self.key = key
        self.counter = counter
    }

    public static func nonce(counter: UInt64) -> Data {
        var data = Data(count: 4)
        withUnsafeBytes(of: counter.littleEndian) { data.append(contentsOf: $0) }
        return data
    }

    public mutating func seal(_ plaintext: Data) throws -> Data {
        guard counter < .max else { throw LinkCryptoError.counterExhausted }
        guard plaintext.count + 16 <= LinkProtocol.maxFrameBytes else { throw LinkCryptoError.frameTooLarge }
        let nonce = try ChaChaPoly.Nonce(data: Self.nonce(counter: counter))
        let box = try ChaChaPoly.seal(plaintext, using: key, nonce: nonce)
        counter += 1
        return box.ciphertext + box.tag
    }

    public mutating func open(_ frame: Data) throws -> Data {
        guard counter < .max else { throw LinkCryptoError.counterExhausted }
        guard frame.count <= LinkProtocol.maxFrameBytes else { throw LinkCryptoError.frameTooLarge }
        guard frame.count >= 16 else { throw LinkCryptoError.decryptionFailed }
        do {
            let nonce = try ChaChaPoly.Nonce(data: Self.nonce(counter: counter))
            let box = try ChaChaPoly.SealedBox(
                nonce: nonce,
                ciphertext: frame.prefix(frame.count - 16),
                tag: frame.suffix(16)
            )
            let plaintext = try ChaChaPoly.open(box, using: key)
            counter += 1
            return plaintext
        } catch {
            throw LinkCryptoError.decryptionFailed
        }
    }
}

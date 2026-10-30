import CryptoKit
import Foundation
import PulseKit
import Security

/// Generic-password Keychain storage for the link identity and the paired desktops.
/// Items are available after first unlock so reconnects work in the background.
enum Keychain {
    private static let service = "app.uwupulse.ios.link"

    static func read(_ account: String) -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
        return result as? Data
    }

    @discardableResult
    static func write(_ data: Data, account: String) -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            return SecItemAdd(query.merging(attributes) { $1 } as CFDictionary, nil) == errSecSuccess
        }
        return status == errSecSuccess
    }

    // MARK: Identity

    private struct StoredIdentity: Codable {
        var id: String
        var key: Data
    }

    /// Loads the static X25519 identity, creating it on first use.
    static func loadOrCreateIdentity() -> LinkIdentity {
        if let data = read("identity"),
           let stored = try? JSONDecoder().decode(StoredIdentity.self, from: data),
           let key = try? Curve25519.KeyAgreement.PrivateKey(rawRepresentation: stored.key) {
            return LinkIdentity(id: stored.id, privateKey: key)
        }
        let identity = LinkIdentity.generate()
        let stored = StoredIdentity(id: identity.id, key: identity.privateKey.rawRepresentation)
        if let data = try? JSONEncoder().encode(stored) { write(data, account: "identity") }
        return identity
    }

    // MARK: Paired desktops

    static func loadDesktops() -> [PairedDesktop] {
        guard let data = read("desktops") else { return [] }
        return (try? JSONDecoder().decode([PairedDesktop].self, from: data)) ?? []
    }

    static func saveDesktops(_ desktops: [PairedDesktop]) {
        guard let data = try? JSONEncoder().encode(desktops) else { return }
        write(data, account: "desktops")
    }
}

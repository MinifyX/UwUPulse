import Foundation

/// `uwupulse://pair?v=1&id=<desk-id>&n=<name>&pk=<b64>&s=<b64>&p=<port>&h=<ip1>,<ip2>,…`
public struct PairingOffer: Equatable, Sendable {
    public var desktopId: String
    public var name: String
    public var publicKey: Data
    public var secret: Data
    public var port: Int
    public var hosts: [String]

    public static let maxHosts = 16

    public enum ParseError: Error, Equatable, Sendable {
        case notAPairingURI
        case unsupportedVersion
        case missing(String)
        case invalid(String)
    }

    public init(desktopId: String, name: String, publicKey: Data, secret: Data, port: Int, hosts: [String]) {
        self.desktopId = desktopId; self.name = name; self.publicKey = publicKey
        self.secret = secret; self.port = port; self.hosts = hosts
    }

    public init(uri: String) throws {
        let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.utf8.count <= 4096 else { throw ParseError.invalid("length") }
        let prefix = "uwupulse://pair?"
        guard trimmed.lowercased().hasPrefix(prefix) else { throw ParseError.notAPairingURI }
        let query = trimmed.dropFirst(prefix.count).split(separator: "#", maxSplits: 1).first ?? ""

        var params: [String: String] = [:]
        for pair in query.split(separator: "&", omittingEmptySubsequences: true) {
            let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            guard let rawKey = parts.first else { continue }
            let rawValue = parts.count > 1 ? parts[1] : ""
            guard let key = Self.formDecode(rawKey), let value = Self.formDecode(rawValue) else {
                throw ParseError.invalid(String(rawKey))
            }
            if params[key] == nil { params[key] = value } // first occurrence wins
        }

        func require(_ key: String) throws -> String {
            guard let value = params[key], !value.isEmpty else { throw ParseError.missing(key) }
            return value
        }

        guard try require("v") == "1" else { throw ParseError.unsupportedVersion }
        let id = try require("id")
        guard id.count <= LinkLimits.maxNameLength else { throw ParseError.invalid("id") }
        let name = (params["n"] ?? "").limited()
        guard let pk = B64.decode(try require("pk")), pk.count == 32 else { throw ParseError.invalid("pk") }
        guard let s = B64.decode(try require("s")), s.count == 32 else { throw ParseError.invalid("s") }
        guard let port = Int(try require("p")), (1...65535).contains(port) else { throw ParseError.invalid("p") }
        let hosts = (params["h"] ?? "")
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        // A pairing code must only point into the local network; anything else rejects the whole code.
        guard hosts.allSatisfy(Self.isAllowedHost) else { throw ParseError.invalid("h") }
        self.init(
            desktopId: id, name: name.isEmpty ? "Desktop" : name, publicKey: pk, secret: s,
            port: port, hosts: Array(hosts.prefix(Self.maxHosts))
        )
    }

    /// Percent-decoding with `+` as space (form encoding).
    private static func formDecode(_ s: Substring) -> String? {
        s.replacingOccurrences(of: "+", with: " ").removingPercentEncoding
    }

    /// Local-network targets only: private IPv4 (RFC 1918), link-local (169.254/16, fe80::/10),
    /// unique local IPv6 (fc00::/7), loopback, or an mDNS name `*.local`.
    public static func isAllowedHost(_ host: String) -> Bool {
        guard !host.isEmpty, host.utf8.count <= 253 else { return false }
        if let v4 = parseIPv4(host) { return isAllowedIPv4(v4) }
        if host.contains(":") {
            // Optional zone (`fe80::1%en0`), only for link-local addresses.
            let parts = host.split(separator: "%", maxSplits: 1, omittingEmptySubsequences: false)
            guard let v6 = parseIPv6(String(parts[0])) else { return false }
            if parts.count == 2 {
                let zone = parts[1]
                guard !zone.isEmpty, zone.count <= 16, zone.unicodeScalars.allSatisfy(isASCIIAlnum),
                      isLinkLocalIPv6(v6) else { return false }
                return true
            }
            return isAllowedIPv6(v6)
        }
        return isLocalHostname(host)
    }

    static func parseIPv4(_ s: String) -> [UInt8]? {
        var addr = in_addr()
        let octets = s.split(separator: ".", omittingEmptySubsequences: false)
        // Strict dotted quad: no leading zeros (some resolvers read them as octal).
        guard octets.count == 4,
              octets.allSatisfy({ !$0.isEmpty && $0.count <= 3 && $0.utf8.allSatisfy { (UInt8(ascii: "0")...UInt8(ascii: "9")).contains($0) } && ($0 == "0" || !$0.hasPrefix("0")) }),
              inet_pton(AF_INET, s, &addr) == 1 else { return nil }
        return withUnsafeBytes(of: &addr) { Array($0) }
    }

    static func parseIPv6(_ s: String) -> [UInt8]? {
        var addr = in6_addr()
        guard s.unicodeScalars.allSatisfy({ isASCIIHex($0) || $0 == ":" || $0 == "." }),
              inet_pton(AF_INET6, s, &addr) == 1 else { return nil }
        return withUnsafeBytes(of: &addr) { Array($0) }
    }

    static func isAllowedIPv4(_ b: [UInt8]) -> Bool {
        guard b.count == 4 else { return false }
        return b[0] == 10                                  // 10.0.0.0/8
            || (b[0] == 172 && (16...31).contains(b[1]))   // 172.16.0.0/12
            || (b[0] == 192 && b[1] == 168)                // 192.168.0.0/16
            || (b[0] == 169 && b[1] == 254)                // 169.254.0.0/16 link-local
            || b[0] == 127                                 // loopback
    }

    static func isLinkLocalIPv6(_ b: [UInt8]) -> Bool {
        b.count == 16 && b[0] == 0xFE && b[1] & 0xC0 == 0x80 // fe80::/10
    }

    static func isAllowedIPv6(_ b: [UInt8]) -> Bool {
        guard b.count == 16 else { return false }
        if isLinkLocalIPv6(b) { return true }
        if b[0] & 0xFE == 0xFC { return true }                       // fc00::/7 ULA
        if b[0..<15].allSatisfy({ $0 == 0 }) && b[15] == 1 { return true } // ::1
        // IPv4-mapped (::ffff:a.b.c.d): same rules as IPv4.
        if b[0..<10].allSatisfy({ $0 == 0 }) && b[10] == 0xFF && b[11] == 0xFF { return isAllowedIPv4(Array(b[12...])) }
        return false
    }

    /// `name.local` / `a.b.local` with LDH labels (no URL syntax that could alter the ws:// URL).
    static func isLocalHostname(_ host: String) -> Bool {
        let lower = host.lowercased()
        let name = lower.hasSuffix(".") ? String(lower.dropLast()) : lower
        let labels = name.split(separator: ".", omittingEmptySubsequences: false)
        guard labels.count >= 2, labels.last == "local" else { return false }
        return labels.allSatisfy { label in
            !label.isEmpty && label.count <= 63 && !label.hasPrefix("-") && !label.hasSuffix("-")
                && label.unicodeScalars.allSatisfy { isASCIIAlnum($0) || $0 == "-" }
        }
    }

    private static func isASCIIAlnum(_ c: Unicode.Scalar) -> Bool {
        ("a"..."z").contains(c) || ("A"..."Z").contains(c) || ("0"..."9").contains(c)
    }

    private static func isASCIIHex(_ c: Unicode.Scalar) -> Bool {
        ("0"..."9").contains(c) || ("a"..."f").contains(c) || ("A"..."F").contains(c)
    }
}

import Foundation

public enum HandshakeMode: String, Codable, Sendable { case known, qr, sas }
public enum HeartRateSource: String, Codable, Sendable { case watch, ble }
public enum WatchLinkState: String, Codable, Sendable { case unavailable, idle, running }
public enum WatchCommand: String, Codable, Sendable { case start, stop }

/// Error codes sent by the desktop as plain-text or encrypted `error` frames.
public enum LinkErrorCode: Equatable, Sendable {
    case version, unknownDevice, noOffer, busy, rejected, other(String)

    public init(raw: String) {
        switch raw {
        case "version": self = .version
        case "unknown_device": self = .unknownDevice
        case "no_offer": self = .noOffer
        case "busy": self = .busy
        case "rejected": self = .rejected
        default: self = .other(raw)
        }
    }

    public var raw: String {
        switch self {
        case .version: "version"
        case .unknownDevice: "unknown_device"
        case .noOffer: "no_offer"
        case .busy: "busy"
        case .rejected: "rejected"
        case .other(let s): s
        }
    }
}

public struct HelloMessage: Equatable, Sendable {
    public var v: Int
    public var id: String
    public var name: String
    public var spk: String
    /// Ephemeral key. Absent in the phone's hello in `sas` mode (sent later in `reveal`).
    public var epk: String?
    /// Only in the phone's hello in `sas` mode: b64(SHA-256("UwUPulse-commit-v1" ‖ epk_phone)).
    public var commit: String?
    /// Only set in the phone's hello (m1).
    public var mode: HandshakeMode?

    public init(
        v: Int = LinkProtocol.version, id: String, name: String, spk: String,
        epk: String?, commit: String? = nil, mode: HandshakeMode?
    ) {
        self.v = v; self.id = id; self.name = name; self.spk = spk
        self.epk = epk; self.commit = commit; self.mode = mode
    }
}

public struct HeartRateMessage: Equatable, Sendable {
    public var ts: Int64
    public var bpm: Int
    public var src: HeartRateSource
    public var dev: String
    public var rr: [Int]?

    public init(ts: Int64, bpm: Int, src: HeartRateSource, dev: String, rr: [Int]? = nil) {
        self.ts = ts; self.bpm = bpm; self.src = src; self.dev = dev; self.rr = rr
    }
}

public struct StatusMessage: Equatable, Sendable {
    public var watch: WatchLinkState
    public var ble: [String]
    public var battery: Double?

    public init(watch: WatchLinkState, ble: [String], battery: Double?) {
        self.watch = watch; self.ble = ble; self.battery = battery
    }
}

/// Every JSON message of the link protocol, discriminated by `"t"`.
public enum LinkMessage: Equatable, Sendable {
    case hello(HelloMessage)
    /// SAS mode only (m3): the phone reveals its ephemeral key after receiving m2.
    case reveal(epk: String)
    case error(LinkErrorCode)
    case auth
    case ready
    case paired(id: String, name: String)
    case hr(HeartRateMessage)
    case status(StatusMessage)
    case ping
    case pong
    case cmd(WatchCommand)
    /// Unknown message type (ignored for forward compatibility).
    case unknown(String)
}

extension LinkMessage: Codable {
    private enum K: String, CodingKey {
        case t, v, id, name, spk, epk, commit, mode, code, ts, bpm, src, dev, rr, watch, ble, battery, action
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: K.self)
        let t = try c.decode(String.self, forKey: .t)
        switch t {
        case "hello":
            self = .hello(HelloMessage(
                v: try c.decode(Int.self, forKey: .v),
                id: try c.decode(String.self, forKey: .id),
                name: try c.decode(String.self, forKey: .name),
                spk: try c.decode(String.self, forKey: .spk),
                epk: try c.decodeIfPresent(String.self, forKey: .epk),
                commit: try c.decodeIfPresent(String.self, forKey: .commit),
                mode: try c.decodeIfPresent(HandshakeMode.self, forKey: .mode)
            ))
        case "reveal": self = .reveal(epk: try c.decode(String.self, forKey: .epk))
        case "error": self = .error(LinkErrorCode(raw: try c.decode(String.self, forKey: .code)))
        case "auth": self = .auth
        case "ready": self = .ready
        case "paired": self = .paired(id: try c.decode(String.self, forKey: .id), name: try c.decode(String.self, forKey: .name))
        case "hr":
            self = .hr(HeartRateMessage(
                ts: try c.decode(Int64.self, forKey: .ts),
                bpm: try c.decode(Int.self, forKey: .bpm),
                src: try c.decode(HeartRateSource.self, forKey: .src),
                dev: try c.decode(String.self, forKey: .dev),
                rr: try c.decodeIfPresent([Int].self, forKey: .rr)
            ))
        case "status":
            self = .status(StatusMessage(
                watch: try c.decode(WatchLinkState.self, forKey: .watch),
                ble: try c.decode([String].self, forKey: .ble),
                battery: try c.decodeIfPresent(Double.self, forKey: .battery)
            ))
        case "ping": self = .ping
        case "pong": self = .pong
        case "cmd": self = .cmd(try c.decode(WatchCommand.self, forKey: .action))
        default: self = .unknown(t)
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: K.self)
        switch self {
        case .hello(let h):
            try c.encode("hello", forKey: .t)
            try c.encode(h.v, forKey: .v)
            try c.encode(h.id, forKey: .id)
            try c.encode(h.name, forKey: .name)
            try c.encode(h.spk, forKey: .spk)
            try c.encodeIfPresent(h.epk, forKey: .epk)
            try c.encodeIfPresent(h.commit, forKey: .commit)
            try c.encodeIfPresent(h.mode, forKey: .mode)
        case .reveal(let epk):
            try c.encode("reveal", forKey: .t)
            try c.encode(epk, forKey: .epk)
        case .error(let code):
            try c.encode("error", forKey: .t)
            try c.encode(code.raw, forKey: .code)
        case .auth: try c.encode("auth", forKey: .t)
        case .ready: try c.encode("ready", forKey: .t)
        case .paired(let id, let name):
            try c.encode("paired", forKey: .t)
            try c.encode(id, forKey: .id)
            try c.encode(name, forKey: .name)
        case .hr(let m):
            try c.encode("hr", forKey: .t)
            try c.encode(m.ts, forKey: .ts)
            try c.encode(m.bpm, forKey: .bpm)
            try c.encode(m.src, forKey: .src)
            try c.encode(m.dev, forKey: .dev)
            try c.encodeIfPresent(m.rr, forKey: .rr)
        case .status(let s):
            try c.encode("status", forKey: .t)
            try c.encode(s.watch, forKey: .watch)
            try c.encode(s.ble, forKey: .ble)
            if let battery = s.battery { try c.encode(battery, forKey: .battery) } else { try c.encodeNil(forKey: .battery) }
        case .ping: try c.encode("ping", forKey: .t)
        case .pong: try c.encode("pong", forKey: .t)
        case .cmd(let action):
            try c.encode("cmd", forKey: .t)
            try c.encode(action, forKey: .action)
        case .unknown(let t): try c.encode(t, forKey: .t)
        }
    }

    public func jsonData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        return try encoder.encode(self)
    }

    public static func decode(_ data: Data) throws -> LinkMessage {
        try JSONDecoder().decode(LinkMessage.self, from: data)
    }
}

public extension String {
    /// Truncates to at most `max` characters (protocol limit for `dev`/`name`).
    func limited(_ max: Int = LinkLimits.maxNameLength) -> String {
        count <= max ? self : String(prefix(max))
    }
}

public enum LinkLimits {
    public static let minBPM = 20
    public static let maxBPM = 250
    public static let maxNameLength = 64
    public static let maxMessagesPerSecond = 20

    public static func isValidBPM(_ bpm: Int) -> Bool { (minBPM...maxBPM).contains(bpm) }
}

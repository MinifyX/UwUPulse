import Foundation

/// Watch → phone payload sent via `sendToRemoteWorkoutSession(data:)`.
public struct WatchHeartRate: Codable, Equatable, Sendable {
    public var bpm: Int
    /// Unix milliseconds.
    public var ts: Int64

    public init(bpm: Int, ts: Int64) { self.bpm = bpm; self.ts = ts }
}

/// Small JSON envelope for the mirrored workout session data channel (both directions).
public enum WorkoutChannelMessage: Codable, Equatable, Sendable {
    case heartRate(WatchHeartRate)
    case command(WatchCommand)

    private enum K: String, CodingKey { case hr, cmd }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: K.self)
        if let hr = try c.decodeIfPresent(WatchHeartRate.self, forKey: .hr) {
            self = .heartRate(hr)
        } else {
            self = .command(try c.decode(WatchCommand.self, forKey: .cmd))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: K.self)
        switch self {
        case .heartRate(let hr): try c.encode(hr, forKey: .hr)
        case .command(let cmd): try c.encode(cmd, forKey: .cmd)
        }
    }

    public func encoded() -> Data { (try? JSONEncoder().encode(self)) ?? Data() }

    public static func decode(_ data: Data) -> WorkoutChannelMessage? {
        guard data.count <= 4096 else { return nil }
        return try? JSONDecoder().decode(WorkoutChannelMessage.self, from: data)
    }
}

/// A desktop this phone is paired with (persisted in the Keychain).
public struct PairedDesktop: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var publicKey: Data
    public var hosts: [String]
    public var port: Int
    public var pairedAt: Date

    public init(id: String, name: String, publicKey: Data, hosts: [String], port: Int, pairedAt: Date = .now) {
        self.id = id; self.name = name; self.publicKey = publicKey
        self.hosts = hosts; self.port = port; self.pairedAt = pairedAt
    }
}

/// Exponential reconnect backoff: 1, 2, 4, … capped at `max` seconds, with ±20 % jitter.
public struct Backoff: Sendable {
    public var attempt = 0
    public let base: Double
    public let max: Double

    public init(base: Double = 1, max: Double = 30) { self.base = base; self.max = max }

    public mutating func next(jitter: Double = Double.random(in: 0.8...1.2)) -> Double {
        let delay = Swift.min(max, base * pow(2, Double(Swift.min(attempt, 16))))
        attempt += 1
        return delay * jitter
    }

    public mutating func reset() { attempt = 0 }
}

/// Sliding one-second window limiter (protocol: at most 20 messages/s per connection).
public struct RateLimiter: Sendable {
    public let limit: Int
    private var stamps: [Double] = []

    public init(limit: Int = LinkLimits.maxMessagesPerSecond) { self.limit = limit }

    public mutating func allow(now: Double) -> Bool {
        stamps.removeAll { now - $0 >= 1 }
        guard stamps.count < limit else { return false }
        stamps.append(now)
        return true
    }
}

public extension Date {
    var unixMillis: Int64 { Int64((timeIntervalSince1970 * 1000).rounded()) }
}

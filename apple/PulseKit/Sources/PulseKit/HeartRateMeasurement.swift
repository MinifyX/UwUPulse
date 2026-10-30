import Foundation

/// Parsed GATT Heart Rate Measurement characteristic (0x2A37).
public struct HeartRateMeasurement: Equatable, Sendable {
    public var bpm: Int
    /// nil if the sensor does not support contact detection.
    public var sensorContact: Bool?
    public var energyExpended: Int?
    /// RR intervals in milliseconds.
    public var rrIntervals: [Int]

    public static let serviceUUID = "180D"
    public static let characteristicUUID = "2A37"

    /// Returns nil for malformed input; never traps.
    public init?(data: Data) {
        let bytes = [UInt8](data)
        guard let flags = bytes.first else { return nil }
        var i = 1

        func u8() -> Int? {
            guard i < bytes.count else { return nil }
            defer { i += 1 }
            return Int(bytes[i])
        }
        func u16() -> Int? {
            guard i + 1 < bytes.count else { return nil }
            defer { i += 2 }
            return Int(bytes[i]) | Int(bytes[i + 1]) << 8
        }

        guard let value = (flags & 0x01 != 0) ? u16() : u8() else { return nil }
        bpm = value
        sensorContact = (flags & 0x04 != 0) ? (flags & 0x02 != 0) : nil
        if flags & 0x08 != 0 {
            guard let energy = u16() else { return nil }
            energyExpended = energy
        } else {
            energyExpended = nil
        }
        var rr: [Int] = []
        if flags & 0x10 != 0 {
            while i + 1 < bytes.count, rr.count < 32, let raw = u16() {
                rr.append(Int((Double(raw) * 1000.0 / 1024.0).rounded()))
            }
        }
        rrIntervals = rr
    }
}

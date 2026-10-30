import SwiftUI

/// The suite palette on the watch, which is always dark.
enum WatchPalette {
    static let pink = Color(red: 0xFF / 255, green: 0x7F / 255, blue: 0xAC / 255)
    static let pinkInk = Color(red: 0x2A / 255, green: 0x0A / 255, blue: 0x18 / 255)
    static let success = Color(red: 0x3D / 255, green: 0xDC / 255, blue: 0x97 / 255)
    static let warning = Color(red: 0xF5 / 255, green: 0xB9 / 255, blue: 0x4A / 255)
    static let danger = Color(red: 0xFF / 255, green: 0x6B / 255, blue: 0x7E / 255)
    static let muted = Color.white.opacity(0.62)
}

/// UwU Sans on the watch, scaled with Dynamic Type.
enum WatchFont {
    static func style(_ style: Font.TextStyle, size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("UwU Sans", size: size, relativeTo: style).weight(weight)
    }
}

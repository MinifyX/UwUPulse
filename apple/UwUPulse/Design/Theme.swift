import SwiftUI
import UIKit

/// The UwUSuite tokens (UwUSuite-Design 1.8.1 `tokens.css`) as dynamic colours.
/// Pink marks selection, focus and the main action, never "ok".
enum Palette {
    static let canvas = dynamic(0xF8F4F6, 0x141016)
    static let surface = dynamic(0xFFFFFF, 0x1C171F)
    static let elevated = dynamic(0xFCF8FA, 0x241E28)
    static let ink = dynamic(0x1C1420, 0xF8F2F6)
    static let muted = dynamic(0x716672, 0xB3A8B3)
    static let faint = dynamic(0xA69BA5, 0x7D717D)
    static let hairline = dynamic(0xF2E8EE, 0x2C2430)
    static let border = dynamic(0xE9DDE4, 0x3A3040)
    static let pink = dynamic(0xFF4D8D, 0xFF7FAC)
    static let pinkSolid = dynamic(0xE11D74, 0xFF7FAC)
    static let pinkInk = dynamic(0xA3154F, 0xFFA3C4)
    static let pinkTint = dynamic(0xFFE4EF, 0x3A1A2A)
    static let success = dynamic(0x12A06D, 0x3DDC97)
    static let warning = dynamic(0xB86E00, 0xF5B94A)
    static let danger = dynamic(0xD92D44, 0xFF6B7E)
    static let dangerInk = dynamic(0xB4233A, 0xFF8A99)

    private static func dynamic(_ light: UInt32, _ dark: UInt32) -> Color {
        Color(UIColor { traits in
            UIColor(hex: traits.userInterfaceStyle == .dark ? dark : light)
        })
    }
}

extension UIColor {
    convenience init(hex: UInt32) {
        self.init(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }
}

/// UwU Sans, the suite's interface font (variable, 200–800), scaled with
/// Dynamic Type like the system styles it stands in for.
enum UwUFont {
    static let family = "UwU Sans"

    static func style(_ style: Font.TextStyle, weight: Font.Weight = .regular) -> Font {
        Font.custom(family, size: size(of: style), relativeTo: style).weight(weight)
    }

    /// A fixed size that still follows Dynamic Type, e.g. the big BPM number.
    static func fixed(_ size: CGFloat, weight: Font.Weight, relativeTo style: Font.TextStyle = .largeTitle) -> Font {
        Font.custom(family, size: size, relativeTo: style).weight(weight)
    }

    private static func size(of style: Font.TextStyle) -> CGFloat {
        switch style {
        case .largeTitle: 34
        case .title: 28
        case .title2: 22
        case .title3: 20
        case .headline: 17
        case .body: 17
        case .callout: 16
        case .subheadline: 15
        case .footnote: 13
        case .caption: 12
        case .caption2: 11
        default: 17
        }
    }

    /// Navigation bar titles and tab bar labels in UwU Sans too, the tab bar in the accent.
    @MainActor static func applyToBars() {
        let nav = UINavigationBarAppearance()
        nav.configureWithDefaultBackground()
        if let large = scaled("UwUSans-Regular", 34, .largeTitle, .heavy) {
            nav.largeTitleTextAttributes = [.font: large]
        }
        if let inline = scaled("UwUSans-Regular", 17, .headline, .bold) {
            nav.titleTextAttributes = [.font: inline]
        }
        let edge = nav.copy()
        edge.configureWithTransparentBackground()
        edge.largeTitleTextAttributes = nav.largeTitleTextAttributes
        edge.titleTextAttributes = nav.titleTextAttributes
        let bar = UINavigationBar.appearance()
        bar.standardAppearance = nav
        bar.compactAppearance = nav
        bar.scrollEdgeAppearance = edge

        let accent = UIColor(named: "AccentColor") ?? .systemPink
        let tab = UITabBarAppearance()
        tab.configureWithDefaultBackground()
        let label = UIFont(name: "UwUSans-Regular", size: 10)?.withWeight(.semibold)
        for item in [tab.stackedLayoutAppearance, tab.inlineLayoutAppearance, tab.compactInlineLayoutAppearance] {
            if let label {
                item.normal.titleTextAttributes = [.font: label]
                item.selected.titleTextAttributes = [.font: label, .foregroundColor: accent]
            }
            item.selected.iconColor = accent
        }
        UITabBar.appearance().standardAppearance = tab
        UITabBar.appearance().scrollEdgeAppearance = tab
        UITabBar.appearance().tintColor = accent
    }

    private static func scaled(_ name: String, _ size: CGFloat, _ style: UIFont.TextStyle, _ weight: UIFont.Weight) -> UIFont? {
        UIFont(name: name, size: size).map { UIFontMetrics(forTextStyle: style).scaledFont(for: $0.withWeight(weight)) }
    }
}

private extension UIFont {
    /// A weight of a variable font, through its `wght` axis.
    func withWeight(_ weight: UIFont.Weight) -> UIFont {
        let wght: CGFloat = switch weight {
        case .heavy: 760
        case .bold: 700
        case .semibold: 600
        case .medium: 500
        default: 400
        }
        // 'wght' as the four-character code the variation dictionary is keyed by.
        let axis = 0x7767_6874
        let descriptor = fontDescriptor.addingAttributes([
            UIFontDescriptor.AttributeName(rawValue: kCTFontVariationAttribute as String): [axis: wght],
        ])
        return UIFont(descriptor: descriptor, size: pointSize)
    }
}

extension View {
    /// The suite's canvas behind a list or a page.
    func canvasBackground() -> some View {
        scrollContentBackground(.hidden).background(Palette.canvas)
    }

    /// A grouped inset list on the suite's canvas (docs/mobile.md: Lists).
    func suiteList() -> some View {
        listStyle(.insetGrouped)
            .listSectionSpacing(.compact)
            .canvasBackground()
    }
}

/// A section header in UwU Sans, sentence case.
struct SectionHeader: View {
    let title: LocalizedStringKey

    init(_ title: LocalizedStringKey) { self.title = title }

    var body: some View {
        Text(title)
            .font(UwUFont.style(.footnote, weight: .semibold))
            .foregroundStyle(Palette.muted)
            .textCase(nil)
    }
}

/// A round tinted badge with a symbol, at the start of a row.
struct SymbolBadge: View {
    let symbol: String
    var color: Color = Palette.pink
    var size: CGFloat = 30

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: size * 0.48, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color.gradient, in: RoundedRectangle(cornerRadius: size * 0.3, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// The small status dot at the end of a row.
struct StatusDot: View {
    let color: Color

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 10, height: 10)
            .accessibilityHidden(true)
    }
}

extension View {
    /// The main action: Liquid Glass on iOS 26, a filled button before.
    @ViewBuilder func primaryButton() -> some View {
        if #available(iOS 26.0, *) {
            buttonStyle(.glassProminent)
        } else {
            buttonStyle(.borderedProminent)
        }
    }

    /// A secondary action next to the main one.
    @ViewBuilder func secondaryButton() -> some View {
        if #available(iOS 26.0, *) {
            buttonStyle(.glass)
        } else {
            buttonStyle(.bordered)
        }
    }
}

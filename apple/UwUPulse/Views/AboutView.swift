import SwiftUI

struct AboutView: View {
    @Environment(AppModel.self) private var model

    private var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(short) (\(build))"
    }

    var body: some View {
        List {
            Section {
                VStack(spacing: 6) {
                    Image("Nyu-happy")
                        .resizable()
                        .scaledToFit()
                        .frame(width: 128, height: 128)
                        .accessibilityHidden(true)
                    Text(verbatim: "UwUPulse")
                        .font(UwUFont.fixed(34, weight: .heavy, relativeTo: .largeTitle))
                        .foregroundStyle(Palette.ink)
                    Text("Your heart rate, live on your desktop.")
                        .font(UwUFont.style(.subheadline))
                        .foregroundStyle(Palette.muted)
                    Text("Version \(version)")
                        .font(UwUFont.style(.footnote))
                        .foregroundStyle(Palette.faint)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
            }
            .listRowBackground(Color.clear)

            Section {
                AboutRow("Your heart rate only goes to computers you paired, directly over your local network. No cloud, no account, no tracking.", symbol: "lock.shield.fill", color: Palette.success)
                AboutRow("Every connection is end-to-end encrypted.", symbol: "key.fill", color: Palette.pink)
                AboutRow("Workouts are discarded at the end and not saved to Health.", symbol: "heart.text.square.fill", color: Palette.pink)
            } header: {
                SectionHeader("Privacy")
            }

            Section {
                LabeledContent("Device ID") {
                    Text(model.links.identity.id)
                        .font(.footnote.monospaced())
                        .foregroundStyle(Palette.muted)
                        .textSelection(.enabled)
                }
            } header: {
                SectionHeader("This iPhone")
            }

            Section {
                LabeledContent("License", value: "MIT")
            }
        }
        .suiteList()
        .navigationTitle("About")
    }
}

private struct AboutRow: View {
    let text: LocalizedStringKey
    let symbol: String
    let color: Color

    init(_ text: LocalizedStringKey, symbol: String, color: Color) {
        self.text = text
        self.symbol = symbol
        self.color = color
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            SymbolBadge(symbol: symbol, color: color)
            Text(text)
                .font(UwUFont.style(.subheadline))
                .foregroundStyle(Palette.ink)
        }
    }
}

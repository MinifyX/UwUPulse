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
                VStack(spacing: 8) {
                    PulsingHeart(bpm: nil, size: 56)
                    Text("UwUPulse").font(.title2.bold())
                    Text("Version \(version)").font(.footnote).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
            }
            .listRowBackground(Color.clear)

            Section("Privacy") {
                Label("Your heart rate only goes to computers you paired, directly over your local network. No cloud, no account, no tracking.", systemImage: "lock.shield")
                Label("Every connection is end-to-end encrypted.", systemImage: "key")
                Label("Workouts are discarded at the end and not saved to Health.", systemImage: "heart.text.square")
            }

            Section("This iPhone") {
                LabeledContent("Device ID") {
                    Text(model.links.identity.id)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                }
            }

            Section {
                LabeledContent("License", value: "MIT")
            }
        }
        .navigationTitle("About")
    }
}

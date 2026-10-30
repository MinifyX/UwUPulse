import SwiftUI

struct WatchContentView: View {
    @State private var manager = WatchWorkoutManager.shared

    var body: some View {
        VStack(spacing: 6) {
            WatchHeart(bpm: manager.bpm, active: manager.phase == .running)
                .frame(height: 44)

            Text(manager.bpm.map(String.init) ?? "--")
                .font(.system(size: 64, weight: .bold, design: .rounded))
                .monospacedDigit()
                .contentTransition(.numericText())
                .animation(.snappy, value: manager.bpm)
                .minimumScaleFactor(0.6)
            Text("BPM")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)

            statusLine

            Button {
                if manager.phase == .running {
                    manager.stop()
                } else {
                    Task { await manager.start() }
                }
            } label: {
                Label(
                    manager.phase == .running ? "Stop" : "Start",
                    systemImage: manager.phase == .running ? "stop.fill" : "play.fill"
                )
            }
            .tint(manager.phase == .running ? .gray : .accentColor)
            .disabled(manager.phase == .starting || manager.phase == .ending)
        }
        .padding(.horizontal)
    }

    @ViewBuilder private var statusLine: some View {
        Group {
            switch manager.phase {
            case .idle:
                if let error = manager.errorMessage { Text(error).lineLimit(2) } else { Text("Not running") }
            case .starting: Text("Starting…")
            case .ending: Text("Ending…")
            case .running:
                if manager.isMirrored {
                    Label("Mirrored to iPhone", systemImage: "iphone.radiowaves.left.and.right")
                } else {
                    Label("iPhone not connected", systemImage: "iphone.slash")
                }
            }
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
        .multilineTextAlignment(.center)
    }
}

/// Heart that beats at the current rate.
struct WatchHeart: View {
    let bpm: Int?
    let active: Bool

    var body: some View {
        let period = 60.0 / Double(max(40, min(bpm ?? 60, 220)))
        Image(systemName: "heart.fill")
            .resizable()
            .scaledToFit()
            .foregroundStyle(Color.accentColor.gradient)
            .phaseAnimator(active && bpm != nil ? [1.0, 1.18, 1.0] : [1.0]) { view, scale in
                view.scaleEffect(scale)
            } animation: { _ in
                .easeInOut(duration: period / 3)
            }
            .accessibilityHidden(true)
    }
}

#Preview {
    WatchContentView()
}

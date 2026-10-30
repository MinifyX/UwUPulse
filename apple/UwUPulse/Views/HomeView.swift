import PulseKit
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let bpm = model.currentBPM(at: context.date)
                    VStack(spacing: 4) {
                        PulsingHeart(bpm: bpm, size: 96)
                            .frame(height: 120)
                        Text(bpm.map(String.init) ?? "--")
                            .font(UwUFont.fixed(84, weight: .heavy))
                            .foregroundStyle(Palette.ink)
                            .contentTransition(.numericText())
                            .animation(.snappy, value: bpm)
                        Text("BPM")
                            .font(UwUFont.style(.headline, weight: .bold))
                            .foregroundStyle(Palette.muted)
                        if bpm != nil, let source = model.source {
                            Label(
                                source == .watch ? String(localized: "From Apple Watch") : String(localized: "From chest strap"),
                                systemImage: source == .watch ? "applewatch" : "dot.radiowaves.left.and.right"
                            )
                            .font(UwUFont.style(.footnote, weight: .medium))
                            .foregroundStyle(Palette.muted)
                            .padding(.top, 4)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                }
            }
            .listRowBackground(Color.clear)

            Section {
                watchRow
                if let error = model.workout.lastError {
                    Text(error)
                        .font(UwUFont.style(.footnote))
                        .foregroundStyle(Palette.danger)
                }
            } header: {
                SectionHeader("Apple Watch")
            } footer: {
                Button(action: model.toggleWorkout) {
                    Label(workoutButtonTitle, systemImage: isWorkoutActive ? "stop.fill" : "play.fill")
                        .font(UwUFont.style(.body, weight: .semibold))
                        .frame(maxWidth: .infinity)
                }
                .primaryButton()
                .controlSize(.large)
                .tint(isWorkoutActive ? Palette.danger : Palette.pinkSolid)
                .disabled(!canToggleWorkout)
                .padding(.top, 12)
            }

            Section {
                if model.links.desktops.isEmpty {
                    Button("Pair a desktop", systemImage: "plus.circle.fill") { model.selectedTab = .desktops }
                        .foregroundStyle(Palette.pinkSolid)
                } else {
                    ForEach(model.links.desktops) { desktop in
                        DesktopStatusRow(name: desktop.name, connection: model.links.connections[desktop.id])
                    }
                }
            } header: {
                SectionHeader("Desktops")
            }

            if model.straps.savedStrapID != nil {
                Section {
                    StrapStatusRow()
                } header: {
                    SectionHeader("Chest strap")
                }
            }
        }
        .suiteList()
        .navigationTitle("UwUPulse")
    }

    private var isWorkoutActive: Bool {
        model.workout.phase == .running || model.workout.phase == .starting
    }

    private var canToggleWorkout: Bool {
        model.workout.healthAvailable && model.workout.watchPaired && model.workout.phase != .stopping
    }

    private var workoutButtonTitle: LocalizedStringKey {
        switch model.workout.phase {
        case .idle: "Start on Apple Watch"
        case .starting: "Starting…"
        case .running: "Stop workout"
        case .stopping: "Stopping…"
        }
    }

    @ViewBuilder private var watchRow: some View {
        let workout = model.workout
        if !workout.healthAvailable {
            StatusRow("Health data is not available on this device.", symbol: "exclamationmark.triangle.fill", color: Palette.warning)
        } else if !workout.watchPaired {
            StatusRow("No Apple Watch paired", symbol: "applewatch.slash", color: Palette.faint)
        } else if !workout.watchAppInstalled {
            StatusRow("Install UwUPulse on your Apple Watch.", symbol: "applewatch", color: Palette.warning)
        } else {
            switch workout.phase {
            case .running: StatusRow("Workout running", symbol: "applewatch.radiowaves.left.and.right", color: Palette.success)
            case .starting: StatusRow("Starting on the watch…", symbol: "applewatch", color: Palette.warning)
            case .stopping: StatusRow("Stopping…", symbol: "applewatch", color: Palette.warning)
            case .idle: StatusRow("Ready", symbol: "applewatch", color: Palette.faint)
            }
        }
    }
}

/// A row with a tinted badge and one line of status text.
struct StatusRow: View {
    let title: LocalizedStringKey
    let symbol: String
    let color: Color

    init(_ title: LocalizedStringKey, symbol: String, color: Color) {
        self.title = title
        self.symbol = symbol
        self.color = color
    }

    var body: some View {
        HStack(spacing: 12) {
            SymbolBadge(symbol: symbol, color: color)
            Text(title)
                .font(UwUFont.style(.body))
                .foregroundStyle(Palette.ink)
        }
    }
}

struct DesktopStatusRow: View {
    var name: String
    var connection: DesktopConnection?

    var body: some View {
        HStack(spacing: 12) {
            SymbolBadge(symbol: "desktopcomputer", color: Palette.pink)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(UwUFont.style(.body, weight: .medium))
                    .foregroundStyle(Palette.ink)
                Text(statusText)
                    .font(UwUFont.style(.footnote))
                    .foregroundStyle(Palette.muted)
            }
            Spacer()
            StatusDot(color: statusColor)
        }
    }

    private var statusText: String {
        guard let phase = connection?.phase else { return String(localized: "Not connected") }
        return phase.localizedDescription
    }

    private var statusColor: Color {
        switch connection?.phase {
        case .ready: Palette.success
        case .connecting, .handshaking, .confirming: Palette.warning
        case .failed: Palette.danger
        default: Palette.faint
        }
    }
}

extension DesktopConnection.Phase {
    var localizedDescription: String {
        switch self {
        case .idle: String(localized: "Not connected")
        case .connecting: String(localized: "Connecting…")
        case .handshaking: String(localized: "Securing connection…")
        case .confirming(_, let confirmed):
            confirmed ? String(localized: "Waiting for confirmation…") : String(localized: "Compare the codes")
        case .ready(let host): String(localized: "Connected (\(host))")
        case .waiting: String(localized: "Offline, retrying…")
        case .failed(let failure): failure.localizedDescription
        }
    }
}

extension DesktopConnection.Failure {
    var localizedDescription: String {
        switch self {
        case .rejected: String(localized: "Pairing was rejected on the desktop.")
        case .removedOnDesktop: String(localized: "This iPhone was removed on the desktop. Pair again.")
        case .noOffer: String(localized: "The desktop is not in pairing mode.")
        case .busy: String(localized: "The desktop is busy with another pairing.")
        case .version: String(localized: "The desktop app uses a different protocol version. Update both apps.")
        case .keyMismatch: String(localized: "The desktop key does not match. Pair again.")
        case .identityMismatch: String(localized: "The computer does not match the pairing code. Pair again.")
        case .replaceDeclined: String(localized: "Pairing cancelled. The stored computer was kept.")
        case .unreachable: String(localized: "The desktop could not be reached.")
        case .protocolError: String(localized: "The connection failed.")
        }
    }
}

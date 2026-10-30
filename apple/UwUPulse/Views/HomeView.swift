import PulseKit
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let bpm = model.currentBPM(at: context.date)
                    VStack(spacing: 8) {
                        PulsingHeart(bpm: bpm)
                            .frame(height: 140)
                        Text(bpm.map(String.init) ?? "--")
                            .font(.system(size: 72, weight: .bold, design: .rounded))
                            .monospacedDigit()
                            .contentTransition(.numericText())
                            .animation(.snappy, value: bpm)
                        Text("BPM")
                            .font(.headline)
                            .foregroundStyle(.secondary)
                        if bpm != nil, let source = model.source {
                            Label(
                                source == .watch ? String(localized: "From Apple Watch") : String(localized: "From chest strap"),
                                systemImage: source == .watch ? "applewatch" : "dot.radiowaves.left.and.right"
                            )
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical)
                }
            }
            .listRowBackground(Color.clear)

            Section("Apple Watch") {
                watchRow
                Button(action: model.toggleWorkout) {
                    Label(workoutButtonTitle, systemImage: isWorkoutActive ? "stop.fill" : "play.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(isWorkoutActive ? .gray : .accentColor)
                .disabled(!canToggleWorkout)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
                if let error = model.workout.lastError {
                    Text(error).font(.footnote).foregroundStyle(.red)
                }
            }

            Section("Desktops") {
                if model.links.desktops.isEmpty {
                    Button("Pair a desktop") { model.selectedTab = .desktops }
                } else {
                    ForEach(model.links.desktops) { desktop in
                        DesktopStatusRow(name: desktop.name, connection: model.links.connections[desktop.id])
                    }
                }
            }

            if model.straps.savedStrapID != nil {
                Section("Chest strap") {
                    StrapStatusRow()
                }
            }
        }
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
            Label("Health data is not available on this device.", systemImage: "exclamationmark.triangle")
        } else if !workout.watchPaired {
            Label("No Apple Watch paired", systemImage: "applewatch.slash")
        } else if !workout.watchAppInstalled {
            Label("Install UwUPulse on your Apple Watch.", systemImage: "applewatch")
        } else {
            switch workout.phase {
            case .running: Label("Workout running", systemImage: "applewatch.radiowaves.left.and.right")
            case .starting: Label("Starting on the watch…", systemImage: "applewatch")
            case .stopping: Label("Stopping…", systemImage: "applewatch")
            case .idle: Label("Ready", systemImage: "applewatch")
            }
        }
    }
}

struct DesktopStatusRow: View {
    var name: String
    var connection: DesktopConnection?

    var body: some View {
        HStack {
            Image(systemName: "desktopcomputer")
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading) {
                Text(name)
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Circle()
                .fill(statusColor)
                .frame(width: 10, height: 10)
                .accessibilityHidden(true)
        }
    }

    private var statusText: String {
        guard let phase = connection?.phase else { return String(localized: "Not connected") }
        return phase.localizedDescription
    }

    private var statusColor: Color {
        switch connection?.phase {
        case .ready: .green
        case .connecting, .handshaking, .confirming: .orange
        case .failed: .red
        default: .gray
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

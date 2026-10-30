import CoreBluetooth
import SwiftUI

struct StrapsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let straps = model.straps
        List {
            if straps.savedStrapID != nil {
                Section("Your strap") {
                    StrapStatusRow()
                    Button("Forget strap", role: .destructive) { straps.forget() }
                }
            }

            Section {
                switch straps.bluetoothState {
                case .unauthorized:
                    Label("Allow Bluetooth for UwUPulse in Settings.", systemImage: "exclamationmark.triangle")
                case .poweredOff:
                    Label("Bluetooth is turned off.", systemImage: "exclamationmark.triangle")
                case .unsupported:
                    Label("Bluetooth is not supported on this device.", systemImage: "exclamationmark.triangle")
                default:
                    let others = straps.found.filter { $0.id != straps.savedStrapID }
                    if others.isEmpty {
                        HStack(spacing: 10) {
                            if straps.isScanning { ProgressView() }
                            Text(straps.isScanning ? "Searching for chest straps…" : "Not searching")
                                .foregroundStyle(.secondary)
                        }
                    }
                    ForEach(others) { item in
                        Button {
                            straps.connect(item)
                        } label: {
                            HStack {
                                Label(item.name, systemImage: "heart.circle")
                                Spacer()
                                SignalBars(rssi: item.rssi)
                            }
                        }
                    }
                }
            } header: {
                Text("Nearby")
            } footer: {
                Text("Any Bluetooth chest strap with the standard heart rate profile works. Wear it so it wakes up.")
            }
        }
        .navigationTitle("Chest straps")
        .onAppear { straps.startScan() }
        .onDisappear { straps.stopScan() }
    }
}

struct StrapStatusRow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let straps = model.straps
        HStack {
            Image(systemName: "dot.radiowaves.left.and.right")
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading) {
                Text(straps.strapName ?? String(localized: "Chest strap"))
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if straps.connectionState == .connected, let bpm = straps.lastMeasurement?.bpm {
                Text("\(bpm) BPM")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var statusText: String {
        let straps = model.straps
        switch straps.connectionState {
        case .connected:
            return straps.lastMeasurement?.sensorContact == false
                ? String(localized: "Connected, no skin contact")
                : String(localized: "Connected")
        case .connecting: return String(localized: "Waiting for strap…")
        case .disconnected: return String(localized: "Not connected")
        }
    }
}

private struct SignalBars: View {
    var rssi: Int

    var body: some View {
        let level = rssi >= -60 ? 1.0 : rssi >= -75 ? 0.66 : 0.33
        Image(systemName: "cellularbars", variableValue: level)
            .foregroundStyle(.secondary)
            .accessibilityHidden(true)
    }
}

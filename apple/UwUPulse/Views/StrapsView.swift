import CoreBluetooth
import SwiftUI

struct StrapsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let straps = model.straps
        List {
            if straps.savedStrapID != nil {
                Section {
                    StrapStatusRow()
                    Button("Forget strap", role: .destructive) { straps.forget() }
                        .foregroundStyle(Palette.danger)
                } header: {
                    SectionHeader("Your strap")
                }
            }

            if straps.savedStrapID == nil, straps.found.isEmpty,
               ![.unauthorized, .poweredOff, .unsupported].contains(straps.bluetoothState) {
                Section {
                    EmptyState(
                        title: "No chest strap yet",
                        message: "Put on your strap and wet the electrodes. It shows up below as soon as it wakes up.",
                        mood: .sleepy
                    )
                }
                .listRowBackground(Color.clear)
            }

            Section {
                switch straps.bluetoothState {
                case .unauthorized:
                    StatusRow("Allow Bluetooth for UwUPulse in Settings.", symbol: "exclamationmark.triangle.fill", color: Palette.warning)
                case .poweredOff:
                    StatusRow("Bluetooth is turned off.", symbol: "exclamationmark.triangle.fill", color: Palette.warning)
                case .unsupported:
                    StatusRow("Bluetooth is not supported on this device.", symbol: "exclamationmark.triangle.fill", color: Palette.warning)
                default:
                    let others = straps.found.filter { $0.id != straps.savedStrapID }
                    if others.isEmpty {
                        HStack(spacing: 10) {
                            if straps.isScanning { ProgressView() }
                            Text(straps.isScanning ? "Searching for chest straps…" : "Not searching")
                                .font(UwUFont.style(.body))
                                .foregroundStyle(Palette.muted)
                        }
                    }
                    ForEach(others) { item in
                        Button {
                            straps.connect(item)
                        } label: {
                            HStack(spacing: 12) {
                                SymbolBadge(symbol: "heart.circle.fill", color: Palette.pink)
                                Text(item.name)
                                    .font(UwUFont.style(.body, weight: .medium))
                                    .foregroundStyle(Palette.ink)
                                Spacer()
                                SignalBars(rssi: item.rssi)
                            }
                        }
                    }
                }
            } header: {
                SectionHeader("Nearby")
            } footer: {
                Text("Any Bluetooth chest strap with the standard heart rate profile works. Wear it so it wakes up.")
            }
        }
        .suiteList()
        .navigationTitle("Chest straps")
        .onAppear { straps.startScan() }
        .onDisappear { straps.stopScan() }
    }
}

struct StrapStatusRow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let straps = model.straps
        HStack(spacing: 12) {
            SymbolBadge(symbol: "dot.radiowaves.left.and.right", color: Palette.pink)
            VStack(alignment: .leading, spacing: 2) {
                Text(straps.strapName ?? String(localized: "Chest strap"))
                    .font(UwUFont.style(.body, weight: .medium))
                    .foregroundStyle(Palette.ink)
                Text(statusText)
                    .font(UwUFont.style(.footnote))
                    .foregroundStyle(Palette.muted)
            }
            Spacer()
            if straps.connectionState == .connected, let bpm = straps.lastMeasurement?.bpm {
                Text("\(bpm) BPM")
                    .font(UwUFont.style(.subheadline, weight: .semibold))
                    .monospacedDigit()
                    .foregroundStyle(Palette.ink)
            }
            StatusDot(color: statusColor)
        }
    }

    private var statusColor: Color {
        let straps = model.straps
        switch straps.connectionState {
        case .connected: return straps.lastMeasurement?.sensorContact == false ? Palette.warning : Palette.success
        case .connecting: return Palette.warning
        case .disconnected: return Palette.faint
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
            .foregroundStyle(Palette.muted)
            .accessibilityHidden(true)
    }
}

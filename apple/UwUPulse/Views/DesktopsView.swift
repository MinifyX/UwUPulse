import PulseKit
import SwiftUI
import VisionKit

struct DesktopsView: View {
    @Environment(AppModel.self) private var model
    @State private var scannerShown = false
    @State private var scanError: String?

    var body: some View {
        List {
            if model.links.desktops.isEmpty {
                Section {
                    EmptyState(
                        title: "No desktop paired yet",
                        message: "Open UwUPulse on your computer and choose “Pair iPhone”. Then scan the QR code or pick the computer below.",
                        mood: .sleepy,
                        action: QRScannerView.isAvailable ? ("Scan QR code", "qrcode.viewfinder", { scannerShown = true }) : nil
                    )
                    .frame(maxWidth: .infinity)
                    if let scanError {
                        Text(scanError)
                            .font(UwUFont.style(.footnote))
                            .foregroundStyle(Palette.danger)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: .infinity)
                    }
                }
                .listRowBackground(Color.clear)
            } else {
                Section {
                ForEach(model.links.desktops) { desktop in
                    DesktopStatusRow(name: desktop.name, connection: model.links.connections[desktop.id])
                        .swipeActions {
                            Button("Remove", role: .destructive) { model.links.remove(desktop) }
                        }
                        .contextMenu {
                            Button("Reconnect", systemImage: "arrow.clockwise") {
                                model.links.connections[desktop.id]?.reconnectNow()
                            }
                            Button("Remove", systemImage: "trash", role: .destructive) { model.links.remove(desktop) }
                        }
                }
            } header: {
                SectionHeader("Paired")
            }

            Section {
                Button("Scan QR code", systemImage: "qrcode.viewfinder") { scannerShown = true }
                    .foregroundStyle(Palette.pinkSolid)
                    .disabled(!QRScannerView.isAvailable)
                if let scanError {
                    Text(scanError).font(UwUFont.style(.footnote)).foregroundStyle(Palette.danger)
                }
            }
            }

            Section {
                if model.links.browser.permissionDenied {
                    Label("Allow local network access for UwUPulse in Settings to find your computer.", systemImage: "wifi.exclamationmark")
                        .foregroundStyle(Palette.muted)
                } else if model.links.unpairedNearby.isEmpty {
                    HStack(spacing: 10) {
                        ProgressView()
                        Text("Searching for computers…").foregroundStyle(Palette.muted)
                    }
                } else {
                    ForEach(model.links.unpairedNearby) { desktop in
                        Button {
                            model.startPairing(desktop)
                        } label: {
                            HStack(spacing: 12) {
                                SymbolBadge(symbol: "desktopcomputer", color: Palette.pink)
                                Text(desktop.name)
                                    .font(UwUFont.style(.body, weight: .medium))
                                    .foregroundStyle(Palette.ink)
                                Spacer()
                                Image(systemName: "chevron.right")
                                    .font(.footnote.weight(.semibold))
                                    .foregroundStyle(Palette.faint)
                            }
                        }
                    }
                }
            } header: {
                SectionHeader("Nearby")
            } footer: {
                Text("Pairing with a nearby computer shows a 6-digit code on both devices. Confirm it on both devices if it matches.")
            }
        }
        .suiteList()
        .navigationTitle("Desktops")
        .sheet(isPresented: $scannerShown) {
            NavigationStack {
                QRScannerView { payload in
                    scannerShown = false
                    handleScan(payload)
                }
                .ignoresSafeArea()
                .navigationTitle("Scan QR code")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { scannerShown = false }
                    }
                }
            }
        }
    }

    private func handleScan(_ payload: String) {
        do {
            let offer = try PairingOffer(uri: payload)
            scanError = nil
            // Let the scanner sheet close before presenting the pairing sheet.
            Task {
                try? await Task.sleep(for: .milliseconds(400))
                model.startPairing(offer)
            }
        } catch PairingOffer.ParseError.invalid("h") {
            scanError = String(localized: "This pairing code points outside your local network and was rejected.")
        } catch PairingOffer.ParseError.unsupportedVersion {
            scanError = String(localized: "This QR code is from a newer UwUPulse version. Update the app.")
        } catch {
            scanError = String(localized: "This is not a UwUPulse pairing code.")
        }
    }
}

/// Shows the progress of the current pairing (QR or SAS code).
struct PairingSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                if let pairing = model.links.pairing {
                    content(for: pairing)
                } else {
                    ProgressView()
                }
            }
            .padding()
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .canvasBackground()
            .navigationTitle(model.links.pairing?.name ?? String(localized: "Pairing"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(isDone ? "Done" : "Cancel") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
        .alert(
            "Replace paired computer?",
            isPresented: Binding(get: { model.links.pendingReplacement != nil }, set: { _ in }),
            presenting: model.links.pendingReplacement
        ) { _ in
            Button("Replace", role: .destructive) { model.links.resolveReplacement(replace: true) }
            Button("Keep old", role: .cancel) { model.links.resolveReplacement(replace: false) }
        } message: { pending in
            Text("“\(pending.existingName)” is already paired but now presents a different key. Replace it only if you reinstalled UwUPulse on that computer.")
        }
    }

    private var isDone: Bool {
        switch model.links.pairing?.phase {
        case .ready, .failed: true
        default: false
        }
    }

    @ViewBuilder private func content(for pairing: DesktopConnection) -> some View {
        switch pairing.phase {
        case .idle, .connecting, .handshaking, .waiting:
            ProgressView()
                .controlSize(.large)
            Text("Connecting to \(pairing.name)…")
                .font(UwUFont.style(.body))
                .foregroundStyle(Palette.muted)
        case .confirming(let code, let confirmed):
            Text(confirmed
                ? "Now confirm the code on the computer."
                : "Check that the computer shows the same code.")
                .font(UwUFont.style(.body))
                .foregroundStyle(Palette.ink)
                .multilineTextAlignment(.center)
            Text(code)
                .font(UwUFont.fixed(52, weight: .heavy))
                .monospacedDigit()
                .foregroundStyle(Palette.ink)
                .kerning(6)
                .textSelection(.enabled)
                .accessibilityLabel(code.map(String.init).joined(separator: " "))
            if confirmed {
                ProgressView()
            } else {
                VStack(spacing: 12) {
                    Button {
                        pairing.confirmSAS()
                    } label: {
                        Text("Codes match").frame(maxWidth: .infinity)
                    }
                    .primaryButton()
                    .tint(Palette.pinkSolid)
                    Button(role: .cancel) {
                        dismiss()
                    } label: {
                        Text("Cancel").frame(maxWidth: .infinity)
                    }
                    .secondaryButton()
                }
                .font(UwUFont.style(.body, weight: .semibold))
                .controlSize(.large)
            }
        case .ready:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 64))
                .foregroundStyle(Palette.success)
            Text("Paired with \(pairing.name)")
                .font(UwUFont.style(.headline, weight: .bold))
        case .failed(let failure):
            Image(systemName: "xmark.circle.fill")
                .font(.system(size: 64))
                .foregroundStyle(Palette.danger)
            Text(failure.localizedDescription)
                .font(UwUFont.style(.body))
                .multilineTextAlignment(.center)
        }
    }
}

/// Confirmation for a pairing link opened from outside the app: nothing connects before "Pair".
struct ConfirmOfferSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let confirmation: AppModel.OfferConfirmation

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Computer", value: confirmation.offer.name)
                    if confirmation.offer.hosts.isEmpty {
                        LabeledContent("Address", value: String(localized: "Found on the local network"))
                    } else {
                        ForEach(confirmation.offer.hosts, id: \.self) { host in
                            LabeledContent("Address", value: "\(host):\(confirmation.offer.port)")
                        }
                    }
                } footer: {
                    Text("Only pair if you just opened “Pair iPhone” on this computer yourself.")
                }
                Section {
                    Button("Pair", systemImage: "link") { model.confirmOffer(confirmation) }
                        .foregroundStyle(Palette.pinkSolid)
                }
            }
            .suiteList()
            .navigationTitle("Pair with computer?")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

/// VisionKit live scanner restricted to QR codes.
struct QRScannerView: UIViewControllerRepresentable {
    var onScan: (String) -> Void

    static var isAvailable: Bool {
        DataScannerViewController.isSupported && DataScannerViewController.isAvailable
    }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ controller: DataScannerViewController, context: Context) {
        context.coordinator.onScan = onScan
    }

    static func dismantleUIViewController(_ controller: DataScannerViewController, coordinator: Coordinator) {
        controller.stopScanning()
    }

    func makeCoordinator() -> Coordinator { Coordinator(onScan: onScan) }

    @MainActor
    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        var onScan: (String) -> Void
        private var delivered = false

        init(onScan: @escaping (String) -> Void) { self.onScan = onScan }

        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            for item in addedItems {
                guard !delivered, case .barcode(let barcode) = item, let payload = barcode.payloadStringValue,
                      payload.hasPrefix("uwupulse://") else { continue }
                delivered = true
                dataScanner.stopScanning()
                onScan(payload)
            }
        }
    }
}

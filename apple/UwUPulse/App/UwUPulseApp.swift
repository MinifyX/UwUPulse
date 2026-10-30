import PulseKit
import SwiftUI
import UIKit

@main
struct UwUPulseApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase

    init() {
        UwUFont.applyToBars()
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(AppModel.shared)
                .tint(Palette.pinkSolid)
                .font(UwUFont.style(.body))
                .onOpenURL { AppModel.shared.handle(url: $0) }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active { AppModel.shared.becameActive() }
                }
        }
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // The mirroring handler and the BLE central must exist before launch completes:
        // a background launch (mirrored workout / BLE restoration) delivers its events right away.
        AppModel.shared.launch()
        return true
    }
}

/// Wires the heart-rate sources (watch, BLE) to the desktop links.
@MainActor @Observable
final class AppModel {
    static let shared = AppModel()

    let workout = PhoneWorkoutManager()
    let straps = StrapManager()
    let links = LinkManager()

    private(set) var bpm: Int?
    private(set) var source: HeartRateSource?
    private(set) var lastSampleAt: Date?
    /// Pairing offer from an opened `uwupulse://pair` link: shown for confirmation, never used directly.
    var offerToConfirm: OfferConfirmation?
    var selectedTab: AppTab = .home
    var pairingSheetShown = false

    enum AppTab: Hashable { case home, desktops, straps, about }

    private var launched = false

    private init() {}

    func launch() {
        guard !launched else { return }
        launched = true
        #if DEBUG
        if Self.isDemo {
            launchDemo()
            return
        }
        #endif
        UIDevice.current.isBatteryMonitoringEnabled = true

        workout.onHeartRate = { [weak self] hr in
            self?.receive(bpm: hr.bpm, ts: hr.ts, src: .watch, dev: String(localized: "Apple Watch"), rr: nil)
        }
        workout.onStateChange = { [weak self] in self?.pushStatus() }
        workout.setUp()

        straps.onMeasurement = { [weak self] measurement, name in
            let rr = measurement.rrIntervals.isEmpty ? nil : measurement.rrIntervals
            self?.receive(bpm: measurement.bpm, ts: Date.now.unixMillis, src: .ble, dev: name, rr: rr)
        }
        straps.onStateChange = { [weak self] in self?.pushStatus() }
        straps.activateIfStrapSaved()

        links.onCommand = { [weak self] command in self?.handle(command) }
        links.start()

        NotificationCenter.default.addObserver(
            forName: UIDevice.batteryLevelDidChangeNotification, object: nil, queue: .main
        ) { _ in
            MainActor.assumeIsolated { AppModel.shared.pushStatus() }
        }
        pushStatus()
    }

    func becameActive() {
        #if DEBUG
        if Self.isDemo { return }
        #endif
        links.resume()
        pushStatus()
    }

    struct OfferConfirmation: Identifiable {
        let id = UUID()
        let offer: PairingOffer
    }

    /// A deep link only pre-fills: the user sees name and hosts and has to confirm.
    func handle(url: URL) {
        guard let offer = try? PairingOffer(uri: url.absoluteString) else { return }
        selectedTab = .desktops
        guard pairingSheetShown else {
            offerToConfirm = OfferConfirmation(offer: offer)
            return
        }
        // Close a running pairing first; presenting while dismissing would drop the sheet.
        pairingSheetShown = false
        Task {
            try? await Task.sleep(for: .milliseconds(400))
            offerToConfirm = OfferConfirmation(offer: offer)
        }
    }

    func confirmOffer(_ confirmation: OfferConfirmation) {
        offerToConfirm = nil
        Task {
            try? await Task.sleep(for: .milliseconds(400))
            startPairing(confirmation.offer)
        }
    }

    func startPairing(_ offer: PairingOffer) {
        links.pair(with: offer)
        pairingSheetShown = true
    }

    func startPairing(_ discovered: DiscoveredDesktop) {
        links.pair(with: discovered)
        pairingSheetShown = true
    }

    func toggleWorkout() {
        Task {
            switch workout.phase {
            case .idle: await workout.startWorkout()
            case .running, .starting: await workout.stopWorkout()
            case .stopping: break
            }
        }
    }

    private func handle(_ command: WatchCommand) {
        Task {
            switch command {
            case .start: if workout.phase == .idle { await workout.startWorkout() }
            case .stop: await workout.stopWorkout()
            }
        }
    }

    private func receive(bpm: Int, ts: Int64, src: HeartRateSource, dev: String, rr: [Int]?) {
        guard LinkLimits.isValidBPM(bpm) else { return }
        self.bpm = bpm
        source = src
        lastSampleAt = .now
        links.forward(HeartRateMessage(ts: ts, bpm: bpm, src: src, dev: dev, rr: rr))
    }

    /// Recent sample (within 10 s), otherwise nil, for the UI.
    func currentBPM(at date: Date) -> Int? {
        guard let lastSampleAt, date.timeIntervalSince(lastSampleAt) < 10 else { return nil }
        return bpm
    }

    func pushStatus() {
        let level = UIDevice.current.batteryLevel
        let battery: Double? = level < 0 ? nil : (Double(level) * 100).rounded() / 100
        links.updateStatus(StatusMessage(watch: workout.linkState, ble: straps.connectedNames, battery: battery))
    }
}

#if DEBUG
extension AppModel {
    /// `-demo` launch argument (simulator screenshots, App Store): a running watch workout with a
    /// heart rate between 72 and 140 BPM, the paired desktop "Studio-PC" and the strap "Polar H10".
    /// Nothing connects, nothing is stored.
    static let isDemo = ProcessInfo.processInfo.arguments.contains("-demo")

    fileprivate func launchDemo() {
        let args = ProcessInfo.processInfo.arguments
        // `-demoTab desktops|straps|about` opens that tab; `-demoEmpty` leaves out desktop and strap.
        if let i = args.firstIndex(of: "-demoTab"), i + 1 < args.count {
            let tab: AppTab = switch args[i + 1] {
            case "desktops": .desktops
            case "straps": .straps
            case "about": .about
            default: .home
            }
            // After the tab view exists, or it starts on its first tab anyway.
            Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(300))
                self?.selectedTab = tab
            }
        }
        workout.showDemo()
        if !args.contains("-demoEmpty") {
            straps.showDemo(name: "Polar H10")
            let desktop = PairedDesktop(
                id: "demo-studio-pc", name: "Studio-PC", publicKey: Data(repeating: 0, count: 32),
                hosts: ["192.0.2.10"], port: 47_320
            )
            links.showDemo(desktop: desktop, host: "192.0.2.10")
        }
        Task { [weak self] in
            var t = 28.0
            while let self {
                // A slow climb from rest to a hard effort and back, with a little jitter.
                let wave = (1 - cos(t / 18)) / 2
                let value = Int((72 + wave * 68 + Double.random(in: -2...2)).rounded())
                bpm = value
                source = .watch
                lastSampleAt = .now
                if straps.savedStrapID != nil { straps.showDemo(bpm: value - 1) }
                t += 1
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }
}
#endif

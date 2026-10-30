import HealthKit
import SwiftUI
import WatchKit

@main
struct UwUPulseWatchApp: App {
    @WKApplicationDelegateAdaptor private var delegate: WatchAppDelegate

    var body: some Scene {
        WindowGroup {
            WatchContentView()
        }
    }
}

final class WatchAppDelegate: NSObject, WKApplicationDelegate {
    /// Called when the iPhone starts the app via `startWatchApp(toHandle:)`.
    func handle(_ workoutConfiguration: HKWorkoutConfiguration) {
        Task { await WatchWorkoutManager.shared.start(configuration: workoutConfiguration) }
    }

    func handleActiveWorkoutRecovery() {
        Task { await WatchWorkoutManager.shared.recover() }
    }
}

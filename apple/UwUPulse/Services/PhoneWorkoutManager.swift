import Foundation
import HealthKit
import Observation
import PulseKit
import WatchConnectivity

/// iPhone side of the watch workout: starts the watch app, receives the mirrored session
/// (which keeps this app running in the background) and its heart-rate data.
@MainActor @Observable
final class PhoneWorkoutManager: NSObject {
    enum Phase: Equatable { case idle, starting, running, stopping }

    private(set) var phase: Phase = .idle
    private(set) var lastError: String?
    private(set) var watchPaired = false
    private(set) var watchAppInstalled = false
    private(set) var authorizationRequested = false

    @ObservationIgnored var onHeartRate: ((WatchHeartRate) -> Void)?
    @ObservationIgnored var onStateChange: (() -> Void)?

    private let healthStore = HKHealthStore()
    private var mirroredSession: HKWorkoutSession?
    private var startTimeout: Task<Void, Never>?

    var healthAvailable: Bool { HKHealthStore.isHealthDataAvailable() }

    var linkState: WatchLinkState {
        guard healthAvailable, watchPaired else { return .unavailable }
        return phase == .running || phase == .stopping ? .running : .idle
    }

    /// Must run early at launch: a background launch for a mirrored session delivers it here.
    func setUp() {
        healthStore.workoutSessionMirroringStartHandler = { [weak self] session in
            Task { @MainActor in self?.adopt(session) }
        }
        if WCSession.isSupported() {
            WCSession.default.delegate = self
            WCSession.default.activate()
        }
    }

    func requestAuthorizationIfNeeded() async {
        guard healthAvailable else { return }
        let read: Set<HKObjectType> = [HKQuantityType(.heartRate)]
        let share: Set<HKSampleType> = [HKObjectType.workoutType()]
        do {
            let status = try await healthStore.statusForAuthorizationRequest(toShare: share, read: read)
            if status == .shouldRequest {
                try await healthStore.requestAuthorization(toShare: share, read: read)
            }
            authorizationRequested = true
        } catch {
            lastError = error.localizedDescription
        }
    }

    /// Launches the watch app with a workout configuration; the watch then mirrors back.
    func startWorkout() async {
        guard phase == .idle else { return }
        lastError = nil
        phase = .starting
        notify()
        await requestAuthorizationIfNeeded()
        let config = HKWorkoutConfiguration()
        config.activityType = .other
        config.locationType = .indoor
        do {
            try await healthStore.startWatchApp(toHandle: config)
            // If no mirrored session arrives, fall back to idle.
            startTimeout?.cancel()
            startTimeout = Task { [weak self] in
                try? await Task.sleep(for: .seconds(20))
                guard !Task.isCancelled, let self, self.phase == .starting else { return }
                self.phase = .idle
                self.lastError = String(localized: "The watch did not start the workout.")
                self.notify()
            }
        } catch {
            phase = .idle
            lastError = error.localizedDescription
            notify()
        }
    }

    /// Asks the watch to end (and discard) the workout.
    func stopWorkout() async {
        guard let session = mirroredSession else {
            phase = .idle
            notify()
            return
        }
        phase = .stopping
        notify()
        do {
            try await session.sendToRemoteWorkoutSession(data: WorkoutChannelMessage.command(.stop).encoded())
        } catch {
            // Fallback: stop through the mirrored session itself.
            session.stopActivity(with: .now)
        }
    }

    private func adopt(_ session: HKWorkoutSession) {
        startTimeout?.cancel()
        mirroredSession = session
        session.delegate = self
        phase = .running
        lastError = nil
        notify()
    }

    private func sessionEnded() {
        mirroredSession = nil
        phase = .idle
        notify()
    }

    private func notify() { onStateChange?() }

    fileprivate func handle(_ messages: [WorkoutChannelMessage]) {
        for message in messages {
            if case .heartRate(let hr) = message, LinkLimits.isValidBPM(hr.bpm) {
                onHeartRate?(hr)
            }
        }
    }

    fileprivate func updateWatch(paired: Bool, installed: Bool) {
        watchPaired = paired
        watchAppInstalled = installed
        notify()
    }
}

extension PhoneWorkoutManager: HKWorkoutSessionDelegate {
    nonisolated func workoutSession(
        _ workoutSession: HKWorkoutSession, didChangeTo toState: HKWorkoutSessionState,
        from fromState: HKWorkoutSessionState, date: Date
    ) {
        guard toState == .ended || toState == .stopped else { return }
        Task { @MainActor in
            if toState == .ended { self.sessionEnded() } else { self.phase = .stopping; self.notify() }
        }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didFailWithError error: Error) {
        let message = error.localizedDescription
        Task { @MainActor in self.lastError = message }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didReceiveDataFromRemoteWorkoutSession data: [Data]) {
        let messages = data.compactMap(WorkoutChannelMessage.decode)
        Task { @MainActor in self.handle(messages) }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didDisconnectFromRemoteDeviceWithError error: Error?) {
        Task { @MainActor in self.sessionEnded() }
    }
}

extension PhoneWorkoutManager: WCSessionDelegate {
    nonisolated func session(_ session: WCSession, activationDidCompleteWith activationState: WCSessionActivationState, error: Error?) {
        let paired = session.isPaired, installed = session.isWatchAppInstalled
        Task { @MainActor in self.updateWatch(paired: paired, installed: installed) }
    }

    nonisolated func sessionWatchStateDidChange(_ session: WCSession) {
        let paired = session.isPaired, installed = session.isWatchAppInstalled
        Task { @MainActor in self.updateWatch(paired: paired, installed: installed) }
    }

    nonisolated func sessionDidBecomeInactive(_ session: WCSession) {}

    nonisolated func sessionDidDeactivate(_ session: WCSession) {
        session.activate()
    }
}

#if DEBUG
extension PhoneWorkoutManager {
    /// `-demo` launch argument (screenshots): a paired watch with a running workout.
    func showDemo() {
        watchPaired = true
        watchAppInstalled = true
        phase = .running
    }
}
#endif

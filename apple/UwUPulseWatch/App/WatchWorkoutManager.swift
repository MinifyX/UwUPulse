import Foundation
import HealthKit
import Observation
import PulseKit

/// Runs the heart-rate workout on the watch and mirrors it to the iPhone.
/// Nothing is saved to Health: the workout is discarded when it ends.
@MainActor @Observable
final class WatchWorkoutManager: NSObject {
    static let shared = WatchWorkoutManager()

    enum Phase: Equatable { case idle, starting, running, ending }

    private(set) var phase: Phase = .idle
    private(set) var bpm: Int?
    private(set) var lastSampleAt: Date?
    private(set) var isMirrored = false
    private(set) var errorMessage: String?

    private let healthStore = HKHealthStore()
    private var session: HKWorkoutSession?
    private var builder: HKLiveWorkoutBuilder?

    private nonisolated static let heartRateType = HKQuantityType(.heartRate)
    private nonisolated static let bpmUnit = HKUnit.count().unitDivided(by: .minute())

    static func configuration() -> HKWorkoutConfiguration {
        let config = HKWorkoutConfiguration()
        config.activityType = .other
        config.locationType = .indoor
        return config
    }

    func requestAuthorization() async -> Bool {
        guard HKHealthStore.isHealthDataAvailable() else { return false }
        do {
            try await healthStore.requestAuthorization(toShare: [HKObjectType.workoutType()], read: [Self.heartRateType])
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func start(configuration: HKWorkoutConfiguration = WatchWorkoutManager.configuration()) async {
        guard phase == .idle else { return }
        phase = .starting
        errorMessage = nil
        guard await requestAuthorization() else {
            phase = .idle
            return
        }
        do {
            let session = try HKWorkoutSession(healthStore: healthStore, configuration: configuration)
            let builder = session.associatedWorkoutBuilder()
            builder.dataSource = HKLiveWorkoutDataSource(healthStore: healthStore, workoutConfiguration: configuration)
            adopt(session: session, builder: builder)
            let now = Date()
            session.startActivity(with: now)
            try await builder.beginCollection(at: now)
            phase = .running
            await mirror()
        } catch {
            errorMessage = error.localizedDescription
            reset()
        }
    }

    /// Re-attaches a session that survived an app relaunch.
    func recover() async {
        do {
            guard let session = try await healthStore.recoverActiveWorkoutSession() else { return }
            adopt(session: session, builder: session.associatedWorkoutBuilder())
            phase = .running
            await mirror()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func stop() {
        guard let session, phase == .running || phase == .starting else { return }
        phase = .ending
        session.end()
    }

    private func adopt(session: HKWorkoutSession, builder: HKLiveWorkoutBuilder) {
        session.delegate = self
        builder.delegate = self
        self.session = session
        self.builder = builder
    }

    private func mirror() async {
        guard let session else { return }
        do {
            try await session.startMirroringToCompanionDevice()
            isMirrored = true
        } catch {
            // The watch keeps measuring; the phone just won't receive values.
            isMirrored = false
            errorMessage = error.localizedDescription
        }
    }

    private func handleEnded(at date: Date) async {
        if let builder {
            try? await builder.endCollection(at: date)
            builder.discardWorkout()
        }
        reset()
    }

    private func reset() {
        session = nil
        builder = nil
        phase = .idle
        isMirrored = false
        bpm = nil
    }

    private func handleHeartRate(_ value: Int, at date: Date) {
        bpm = value
        lastSampleAt = date
        guard let session, isMirrored else { return }
        let data = WorkoutChannelMessage.heartRate(WatchHeartRate(bpm: value, ts: date.unixMillis)).encoded()
        Task {
            do { try await session.sendToRemoteWorkoutSession(data: data) } catch { /* phone not reachable; next sample retries */ }
        }
    }

    private func handleRemote(_ messages: [WorkoutChannelMessage]) {
        for message in messages {
            if case .command(.stop) = message { stop() }
        }
    }
}

extension WatchWorkoutManager: HKWorkoutSessionDelegate {
    nonisolated func workoutSession(
        _ workoutSession: HKWorkoutSession, didChangeTo toState: HKWorkoutSessionState,
        from fromState: HKWorkoutSessionState, date: Date
    ) {
        Task { @MainActor in
            switch toState {
            case .stopped: self.session?.end()
            case .ended: await self.handleEnded(at: date)
            default: break
            }
        }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didFailWithError error: Error) {
        let message = error.localizedDescription
        Task { @MainActor in self.errorMessage = message }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didReceiveDataFromRemoteWorkoutSession data: [Data]) {
        let messages = data.compactMap(WorkoutChannelMessage.decode)
        Task { @MainActor in self.handleRemote(messages) }
    }

    nonisolated func workoutSession(_ workoutSession: HKWorkoutSession, didDisconnectFromRemoteDeviceWithError error: Error?) {
        Task { @MainActor in self.isMirrored = false }
    }
}

extension WatchWorkoutManager: HKLiveWorkoutBuilderDelegate {
    nonisolated func workoutBuilderDidCollectEvent(_ workoutBuilder: HKLiveWorkoutBuilder) {}

    nonisolated func workoutBuilder(_ workoutBuilder: HKLiveWorkoutBuilder, didCollectDataOf collectedTypes: Set<HKSampleType>) {
        guard collectedTypes.contains(Self.heartRateType),
              let stats = workoutBuilder.statistics(for: Self.heartRateType),
              let quantity = stats.mostRecentQuantity()
        else { return }
        let value = Int(quantity.doubleValue(for: Self.bpmUnit).rounded())
        let date = stats.mostRecentQuantityDateInterval()?.end ?? Date()
        guard LinkLimits.isValidBPM(value) else { return }
        Task { @MainActor in self.handleHeartRate(value, at: date) }
    }
}

#if DEBUG
extension WatchWorkoutManager {
    /// `-demo`: a running workout with a made-up rate, for screenshots. Touches no HealthKit.
    static let isDemo = ProcessInfo.processInfo.arguments.contains("-demo")

    func showDemo() {
        phase = .running
        isMirrored = true
        Task {
            var t = 28.0
            while !Task.isCancelled {
                bpm = Int(72 + (1 - cos(t / 18)) / 2 * 68)
                lastSampleAt = .now
                t += 1
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }
}
#endif

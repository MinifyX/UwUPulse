import Foundation
import Observation
import PulseKit

struct HostPort: Hashable, Sendable {
    var host: String
    var port: Int
}

/// One WebSocket link to a desktop: handshake, encrypted frames, ping, reconnect with backoff.
@MainActor @Observable
final class DesktopConnection: Identifiable {
    enum Mode {
        case known(PairedDesktop)
        case qr(PairingOffer)
        case sas(DiscoveredDesktop)
    }

    enum Failure: Equatable {
        case rejected, removedOnDesktop, noOffer, busy, version, keyMismatch, identityMismatch, replaceDeclined, unreachable, protocolError
    }

    enum Phase: Equatable {
        case idle
        case connecting
        case handshaking
        /// SAS pairing: code shown on both devices. `confirmed` once the user tapped "Codes match"
        /// here (then waiting for the desktop).
        case confirming(code: String, confirmed: Bool)
        case ready(host: String)
        case waiting(until: Date)
        case failed(Failure)
    }

    private enum Outcome: Equatable {
        case unreachable
        case closedAfterReady
        case retry
        /// `known`: the desktop answered `unknown_device` before `ready` (not fatal on its own).
        case unknownDevice
        case fatal(Failure)
    }

    /// `known`: rounds in a row in which every reachable address answered `unknown_device`.
    private static let unknownDeviceRoundsLimit = 3

    let id: String
    /// Desktop id; for a pairing it becomes the id the desktop confirmed in `paired`.
    private(set) var desktopId: String
    private(set) var name: String
    private(set) var mode: Mode
    private(set) var phase: Phase = .idle

    /// Candidate addresses, best first. Set by the owner.
    var hostsProvider: (@MainActor (Mode) async -> [HostPort])?
    var onPaired: ((PairedDesktop) -> Void)?
    var onReady: ((DesktopConnection, HostPort) -> Void)?
    var onCommand: ((WatchCommand) -> Void)?

    private let identity: LinkIdentity
    private let deviceName: String
    private var loop: Task<Void, Never>?
    private var socket: URLSessionWebSocketTask?
    private var session: LinkSession?
    private var limiter = RateLimiter()
    private var backoff = Backoff()
    private var wakeUp: CheckedContinuation<Void, Never>?
    private var sleepGeneration = 0
    private var unknownDeviceRounds = 0
    /// SAS: the user confirmed the code on this phone during the current attempt.
    private var sasConfirmed = false

    private static let urlSession: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 10
        config.waitsForConnectivity = false
        return URLSession(configuration: config)
    }()

    init(mode: Mode, identity: LinkIdentity, deviceName: String) {
        self.mode = mode
        self.identity = identity
        self.deviceName = deviceName
        switch mode {
        case .known(let d): id = d.id; name = d.name
        case .qr(let offer): id = offer.desktopId; name = offer.name
        case .sas(let found): id = found.id; name = found.name
        }
        desktopId = id
    }

    var isReady: Bool { if case .ready = phase { true } else { false } }

    /// Pairing attempts run once; known desktops reconnect forever.
    private var isPersistent: Bool { if case .known = mode { true } else { false } }

    func start() {
        guard loop == nil else { return }
        loop = Task { [weak self] in await self?.run() }
    }

    func stop() {
        loop?.cancel()
        loop = nil
        wakeUp?.resume()
        wakeUp = nil
        socket?.cancel(with: .goingAway, reason: nil)
        socket = nil
        session = nil
        phase = .idle
    }

    /// Skips the remaining backoff delay (e.g. desktop just appeared on Bonjour, app came to foreground).
    func reconnectNow() {
        if case .failed = phase, isPersistent {
            backoff.reset()
            loop = nil
            start()
            return
        }
        guard case .waiting = phase else { return }
        backoff.reset()
        wakeUp?.resume()
        wakeUp = nil
    }

    /// SAS: the user tapped "Codes match". Only now the phone sends `auth`.
    func confirmSAS() {
        guard isSAS, !sasConfirmed, case .confirming(let code, false) = phase, session != nil else { return }
        sasConfirmed = true
        phase = .confirming(code: code, confirmed: true)
        sendSealed(.auth)
    }

    /// Ends this attempt with a failure shown to the user (e.g. replacing a stored desktop was declined).
    func abort(_ failure: Failure) {
        stop()
        phase = .failed(failure)
    }

    /// Sends a message if the link is ready (drops it otherwise or when over the rate limit).
    func send(_ message: LinkMessage) {
        guard isReady else { return }
        guard limiter.allow(now: ProcessInfo.processInfo.systemUptime) else { return }
        sendSealed(message)
    }

    // MARK: - Loop

    private func run() async {
        while !Task.isCancelled {
            phase = .connecting
            let candidates = await hostsProvider?(mode) ?? []
            var outcomes: [Outcome] = []
            for candidate in candidates {
                let outcome = await attempt(candidate)
                if Task.isCancelled { return }
                outcomes.append(outcome)
                if outcome == .closedAfterReady { break }
                if case .fatal = outcome { break }
                // Pairing: one real attempt. Known desktops: a rejection may come from a stale
                // address or a spoofer, so try the next address.
                if !isPersistent, outcome != .unreachable { break }
            }
            let outcome = outcomes.last ?? .unreachable
            if case .fatal(let failure) = outcome {
                phase = .failed(failure)
                loop = nil
                return
            }
            if !isPersistent {
                phase = .failed(outcome == .unreachable ? .unreachable : .protocolError)
                loop = nil
                return
            }
            let reachable = outcomes.filter { $0 != .unreachable }
            if !reachable.isEmpty, reachable.allSatisfy({ $0 == .unknownDevice }) {
                unknownDeviceRounds += 1
            } else {
                unknownDeviceRounds = 0
            }
            if unknownDeviceRounds >= Self.unknownDeviceRoundsLimit {
                unknownDeviceRounds = 0
                phase = .failed(.removedOnDesktop)
                loop = nil
                return
            }
            if outcomes.contains(.closedAfterReady) { backoff.reset() }
            let delay = backoff.next()
            phase = .waiting(until: Date().addingTimeInterval(delay))
            await sleep(seconds: delay)
        }
    }

    private func sleep(seconds: Double) async {
        sleepGeneration += 1
        let generation = sleepGeneration
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            wakeUp = continuation
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(seconds))
                self?.wake(generation)
            }
        }
    }

    private func wake(_ generation: Int) {
        guard generation == sleepGeneration else { return }
        wakeUp?.resume()
        wakeUp = nil
    }

    private func attempt(_ target: HostPort) async -> Outcome {
        guard let url = linkURL(host: target.host, port: target.port) else { return .unreachable }
        let task = Self.urlSession.webSocketTask(with: url)
        task.maximumMessageSize = LinkProtocol.maxFrameBytes * 2
        socket = task
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
            if socket === task { socket = nil }
            session = nil
            limiter = RateLimiter()
        }

        var reachedReady = false
        sasConfirmed = false
        let pinger = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(LinkProtocol.pingInterval))
                self?.send(.ping)
            }
        }
        defer { pinger.cancel() }

        do {
            let handshake = try makeHandshake()
            phase = .handshaking
            try await send(.string(handshake.m1String), on: task)
            guard case .string(let m2) = try await receive(task, timeout: 10) else { return .retry }
            let linkSession = try handshake.finish(m2: Data(m2.utf8))
            if let m3 = handshake.m3String {
                // SAS: reveal the committed ephemeral key only after m2 is fixed.
                try await send(.string(m3), on: task)
            }
            session = linkSession
            if isSAS {
                // Nothing is sent or stored until the user confirms the code here (confirmSAS()).
                phase = .confirming(code: linkSession.sas, confirmed: false)
            } else {
                sendSealed(.auth)
            }

            while !Task.isCancelled {
                let timeout: Double = reachedReady ? 30 : (isSAS ? 150 : 20)
                guard case .data(let frame) = try await receive(task, timeout: timeout) else {
                    return reachedReady ? .closedAfterReady : .retry // text frame after handshake: close
                }
                guard frame.count <= LinkProtocol.maxFrameBytes, var current = session else { return .retry }
                let message = try current.open(frame)
                session = current

                switch message {
                case .paired(let desktopId, let desktopName):
                    // Only valid for a pairing attempt that the user confirmed (SAS) and only once.
                    guard !reachedReady, isPairing, !isSAS || sasConfirmed else { return .fatal(.protocolError) }
                    guard completePairing(id: desktopId, name: desktopName, session: linkSession, via: target) else {
                        return .fatal(.identityMismatch)
                    }
                case .ready:
                    if isPairing { return .fatal(.protocolError) } // `ready` before `paired`
                    reachedReady = true
                    unknownDeviceRounds = 0
                    backoff.reset()
                    phase = .ready(host: target.host)
                    onReady?(self, target)
                case .cmd(let command):
                    if reachedReady { onCommand?(command) }
                case .error(let code):
                    if reachedReady { return .closedAfterReady }
                    return rejection(code)
                case .pong, .ping, .hello, .reveal, .auth, .hr, .status, .unknown:
                    break
                }
            }
            return .closedAfterReady
        } catch HandshakeError.desktop(let code) {
            return rejection(code)
        } catch HandshakeError.keyMismatch {
            return isPersistent ? .retry : .fatal(.keyMismatch)
        } catch HandshakeError.idMismatch {
            return .fatal(.identityMismatch)
        } catch is HandshakeError {
            return .retry
        } catch is LinkCryptoError {
            return .retry
        } catch {
            if reachedReady { return .closedAfterReady }
            // Lost during SAS confirmation: the desktop closed (rejected or timed out).
            if case .confirming = phase { return .fatal(.rejected) }
            return session == nil ? .unreachable : .retry
        }
    }

    private var isSAS: Bool { if case .sas = mode { true } else { false } }
    /// Still pairing (QR/SAS) and not yet stored as a known desktop.
    private var isPairing: Bool { !isPersistent }

    /// Plaintext or encrypted `error` before `ready`. Known desktops retry (next address, then
    /// backoff); pairing attempts end with the error.
    private func rejection(_ code: LinkErrorCode) -> Outcome {
        guard isPersistent else { return .fatal(failure(for: code)) }
        return code == .unknownDevice ? .unknownDevice : .retry
    }

    private func makeHandshake() throws -> PhoneHandshake {
        switch mode {
        case .known(let desktop):
            return try PhoneHandshake(identity: identity, deviceName: deviceName, mode: .known, expectedDesktopKey: desktop.publicKey)
        case .qr(let offer):
            return try PhoneHandshake(
                identity: identity, deviceName: deviceName, mode: .qr, expectedDesktopKey: offer.publicKey,
                expectedDesktopId: offer.desktopId, psk: offer.secret
            )
        case .sas:
            return try PhoneHandshake(identity: identity, deviceName: deviceName, mode: .sas, expectedDesktopKey: nil)
        }
    }

    /// Hands the new desktop to the owner. `paired.id` must be the id from m2 (authenticated by the
    /// transcript) and, for QR, the id from the QR code.
    private func completePairing(id desktopId: String, name desktopName: String, session: LinkSession, via target: HostPort) -> Bool {
        guard desktopId.limited() == session.desktopId else { return false }
        var hosts = [target.host]
        if case .qr(let offer) = mode {
            guard desktopId == offer.desktopId else { return false }
            hosts += offer.hosts.filter { $0 != target.host }
        }
        let desktop = PairedDesktop(
            id: desktopId.limited(), name: desktopName.limited(), publicKey: session.desktopKey,
            hosts: Array(hosts.prefix(8)), port: target.port
        )
        self.desktopId = desktop.id
        name = desktop.name
        mode = .known(desktop)
        onPaired?(desktop)
        return true
    }

    /// Updates the stored desktop data used for reconnects (after a successful connect).
    func update(desktop: PairedDesktop) {
        mode = .known(desktop)
        name = desktop.name
    }

    private func failure(for code: LinkErrorCode) -> Failure {
        switch code {
        case .version: .version
        case .unknownDevice: .removedOnDesktop
        case .noOffer: .noOffer
        case .busy: .busy
        case .rejected: .rejected
        case .other: .protocolError
        }
    }

    // MARK: - I/O helpers

    private func sendSealed(_ message: LinkMessage) {
        guard var current = session, let task = socket else { return }
        do {
            let frame = try current.seal(message)
            session = current
            // Completion-handler variant enqueues synchronously, so frames keep counter order.
            task.send(.data(frame)) { error in
                if error != nil { task.cancel(with: .abnormalClosure, reason: nil) }
            }
        } catch {
            task.cancel(with: .abnormalClosure, reason: nil)
        }
    }

    private func send(_ message: URLSessionWebSocketTask.Message, on task: URLSessionWebSocketTask) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            task.send(message) { error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume() }
            }
        }
    }

    private func receive(_ task: URLSessionWebSocketTask, timeout: Double) async throws -> URLSessionWebSocketTask.Message {
        let timer = Task {
            try await Task.sleep(for: .seconds(timeout))
            task.cancel(with: .goingAway, reason: nil)
        }
        defer { timer.cancel() }
        return try await task.receive()
    }
}

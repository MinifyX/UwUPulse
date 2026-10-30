import Foundation
import Observation
import PulseKit
import UIKit

/// Owns the paired desktops, Bonjour discovery and one persistent connection per desktop.
@MainActor @Observable
final class LinkManager {
    let identity: LinkIdentity
    let browser = BonjourBrowser()
    private(set) var desktops: [PairedDesktop]
    private(set) var connections: [String: DesktopConnection] = [:]
    /// The pairing attempt shown in the pairing sheet.
    private(set) var pairing: DesktopConnection?
    /// A pairing produced a desktop whose id is already stored with a different key: the user decides.
    private(set) var pendingReplacement: PendingReplacement?
    var onCommand: ((WatchCommand) -> Void)?

    private var status = StatusMessage(watch: .unavailable, ble: [], battery: nil)
    private let deviceName = UIDevice.current.name

    init() {
        identity = Keychain.loadOrCreateIdentity()
        desktops = Keychain.loadDesktops()
        browser.onChange = { [weak self] in self?.bonjourChanged() }
    }

    func start() {
        browser.start()
        for desktop in desktops { connection(for: desktop).start() }
    }

    /// App returned to the foreground: refresh discovery, skip pending backoff delays.
    func resume() {
        browser.restart()
        for connection in connections.values { connection.reconnectNow() }
    }

    var readyCount: Int { connections.values.filter(\.isReady).count }

    /// Desktops on the network that are not paired yet.
    var unpairedNearby: [DiscoveredDesktop] {
        let paired = Set(desktops.map(\.id))
        return browser.desktops.filter { !paired.contains($0.id) }
    }

    // MARK: Forwarding

    func forward(_ hr: HeartRateMessage) {
        guard LinkLimits.isValidBPM(hr.bpm) else { return }
        var message = hr
        message.dev = hr.dev.limited()
        for connection in connections.values { connection.send(.hr(message)) }
    }

    func updateStatus(_ newStatus: StatusMessage) {
        guard newStatus != status else { return }
        status = newStatus
        for connection in connections.values { connection.send(.status(newStatus)) }
    }

    // MARK: Pairing

    func pair(with offer: PairingOffer) {
        startPairing(DesktopConnection(mode: .qr(offer), identity: identity, deviceName: deviceName))
    }

    func pair(with discovered: DiscoveredDesktop) {
        startPairing(DesktopConnection(mode: .sas(discovered), identity: identity, deviceName: deviceName))
    }

    struct PendingReplacement {
        let desktop: PairedDesktop
        let existingName: String
        let connection: DesktopConnection
    }

    /// Answer to the "replace the stored desktop?" alert.
    func resolveReplacement(replace: Bool) {
        guard let pending = pendingReplacement else { return }
        pendingReplacement = nil
        if replace {
            adopt(pending.desktop, connection: pending.connection)
        } else {
            pending.connection.abort(.replaceDeclined)
        }
    }

    func cancelPairing() {
        if let pending = pendingReplacement, pending.connection === pairing {
            pendingReplacement = nil
        }
        guard let pairing else { return }
        // Keep it if pairing already succeeded (it is now a regular connection).
        if connections[pairing.desktopId] !== pairing { pairing.stop() }
        self.pairing = nil
    }

    func remove(_ desktop: PairedDesktop) {
        connections.removeValue(forKey: desktop.id)?.stop()
        desktops.removeAll { $0.id == desktop.id }
        Keychain.saveDesktops(desktops)
    }

    private func startPairing(_ connection: DesktopConnection) {
        cancelPairing()
        configure(connection)
        pairing = connection
        connection.start()
    }

    // MARK: Connections

    private func connection(for desktop: PairedDesktop) -> DesktopConnection {
        if let existing = connections[desktop.id] { return existing }
        let connection = DesktopConnection(mode: .known(desktop), identity: identity, deviceName: deviceName)
        configure(connection)
        connections[desktop.id] = connection
        return connection
    }

    private func configure(_ connection: DesktopConnection) {
        connection.hostsProvider = { [weak self] mode in await self?.candidates(for: mode) ?? [] }
        connection.onPaired = { [weak self, weak connection] desktop in
            guard let self, let connection else { return }
            // Never replace a stored desktop silently: same id with another key needs the user's OK.
            if let existing = self.desktops.first(where: { $0.id == desktop.id }), existing.publicKey != desktop.publicKey {
                self.pendingReplacement = PendingReplacement(desktop: desktop, existingName: existing.name, connection: connection)
                return
            }
            self.adopt(desktop, connection: connection)
        }
        connection.onReady = { [weak self] connection, target in
            // Only adopted connections (not one waiting for the replace decision) get data.
            guard let self, self.connections[connection.desktopId] === connection else { return }
            connection.send(.status(self.status))
            self.remember(target, for: connection.desktopId)
        }
        connection.onCommand = { [weak self] command in self?.onCommand?(command) }
    }

    private func adopt(_ desktop: PairedDesktop, connection: DesktopConnection) {
        store(desktop)
        if let old = connections[desktop.id], old !== connection { old.stop() }
        connections[desktop.id] = connection
        // The pairing may have reached `ready` while the user was deciding.
        if connection.isReady { connection.send(.status(status)) }
    }

    private func store(_ desktop: PairedDesktop) {
        if let index = desktops.firstIndex(where: { $0.id == desktop.id }) {
            desktops[index] = desktop
        } else {
            desktops.append(desktop)
        }
        Keychain.saveDesktops(desktops)
    }

    /// Puts the address that worked first in the stored host list.
    private func remember(_ target: HostPort, for id: String) {
        guard let index = desktops.firstIndex(where: { $0.id == id }) else { return }
        var desktop = desktops[index]
        let hosts = Array(([target.host] + desktop.hosts.filter { $0 != target.host }).prefix(8))
        guard hosts != desktop.hosts || desktop.port != target.port else { return }
        desktop.hosts = hosts
        desktop.port = target.port
        desktops[index] = desktop
        Keychain.saveDesktops(desktops)
        connections[id]?.update(desktop: desktop)
    }

    /// Bonjour-resolved address first, then stored/QR addresses.
    private func candidates(for mode: DesktopConnection.Mode) async -> [HostPort] {
        var result: [HostPort] = []
        let id: String
        var fallback: [HostPort] = []
        switch mode {
        case .known(let desktop):
            id = desktop.id
            fallback = desktop.hosts.map { HostPort(host: $0, port: desktop.port) }
        case .qr(let offer):
            id = offer.desktopId
            fallback = offer.hosts.map { HostPort(host: $0, port: offer.port) }
        case .sas(let found):
            id = found.id
        }
        if let found = browser.desktops.first(where: { $0.id == id }),
           let resolved = await BonjourBrowser.resolve(found.endpoint) {
            result.append(HostPort(host: resolved.host, port: resolved.port))
        }
        for candidate in fallback where !result.contains(candidate) { result.append(candidate) }
        return result
    }

    private func bonjourChanged() {
        let visible = Set(browser.desktops.map(\.id))
        for (id, connection) in connections where visible.contains(id) {
            connection.reconnectNow()
        }
    }
}

#if DEBUG
extension LinkManager {
    /// `-demo` launch argument (screenshots): one paired, connected desktop, nothing stored.
    func showDemo(desktop: PairedDesktop, host: String) {
        desktops = [desktop]
        let connection = DesktopConnection(mode: .known(desktop), identity: identity, deviceName: deviceName)
        connection.showDemoPhase(.ready(host: host))
        connections = [desktop.id: connection]
    }
}
#endif

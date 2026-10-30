import Foundation
import Network
import Observation
import PulseKit
import Synchronization

/// A desktop advertising `_uwupulse._tcp` on the local network.
struct DiscoveredDesktop: Identifiable, Equatable, Sendable {
    /// Desktop id from the TXT record (falls back to the instance name).
    var id: String
    var name: String
    var publicKey: Data?
    var endpoint: NWEndpoint
}

/// Browses for desktops with NWBrowser.
@MainActor @Observable
final class BonjourBrowser {
    private(set) var desktops: [DiscoveredDesktop] = []
    private(set) var isBrowsing = false
    private(set) var permissionDenied = false
    @ObservationIgnored var onChange: (() -> Void)?
    private var browser: NWBrowser?

    func start() {
        guard browser == nil else { return }
        let parameters = NWParameters.tcp
        parameters.includePeerToPeer = false
        let browser = NWBrowser(for: .bonjourWithTXTRecord(type: LinkProtocol.bonjourType, domain: nil), using: parameters)
        browser.stateUpdateHandler = { [weak self] state in
            MainActor.assumeIsolated { self?.handle(state) }
        }
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            MainActor.assumeIsolated { self?.update(results) }
        }
        browser.start(queue: .main)
        self.browser = browser
        isBrowsing = true
    }

    func stop() {
        browser?.cancel()
        browser = nil
        isBrowsing = false
    }

    /// Restarts browsing (e.g. after the app returns to the foreground).
    func restart() {
        stop()
        start()
    }

    private func handle(_ state: NWBrowser.State) {
        switch state {
        case .failed:
            stop()
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(3))
                self?.start()
            }
        case .waiting(let error):
            if case .dns(let code) = error, code == -65570 /* kDNSServiceErr_PolicyDenied */ { permissionDenied = true }
        case .ready:
            permissionDenied = false
        default:
            break
        }
    }

    private func update(_ results: Set<NWBrowser.Result>) {
        desktops = results.compactMap { result -> DiscoveredDesktop? in
            guard case .service(let name, _, _, _) = result.endpoint else { return nil }
            var id = name
            var key: Data?
            if case .bonjour(let txt) = result.metadata {
                if let v = txt["v"], v != String(LinkProtocol.version) { return nil }
                if let txtId = txt["id"], !txtId.isEmpty { id = txtId.limited() }
                key = txt["pk"].flatMap(B64.decode).flatMap { $0.count == 32 ? $0 : nil }
            }
            return DiscoveredDesktop(id: id, name: name.limited(), publicKey: key, endpoint: result.endpoint)
        }
        .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        onChange?()
    }

    /// Resolves a Bonjour endpoint to a host and port by briefly opening a TCP connection.
    /// Prefers IPv4 (simpler ws:// URLs); falls back to any address family.
    nonisolated static func resolve(_ endpoint: NWEndpoint, timeout: Duration = .seconds(5)) async -> (host: String, port: Int)? {
        if let v4 = await resolveOnce(endpoint, ipv4Only: true, timeout: timeout) { return v4 }
        return await resolveOnce(endpoint, ipv4Only: false, timeout: timeout)
    }

    private nonisolated static func resolveOnce(_ endpoint: NWEndpoint, ipv4Only: Bool, timeout: Duration) async -> (host: String, port: Int)? {
        let parameters = NWParameters.tcp
        if ipv4Only, let ip = parameters.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
            ip.version = .v4
        }
        let connection = NWConnection(to: endpoint, using: parameters)
        let finished = Mutex(false)

        return await withCheckedContinuation { (continuation: CheckedContinuation<(host: String, port: Int)?, Never>) in
            let finish: @Sendable ((host: String, port: Int)?) -> Void = { value in
                let first = finished.withLock { done -> Bool in
                    defer { done = true }
                    return !done
                }
                guard first else { return }
                connection.cancel()
                continuation.resume(returning: value)
            }
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    if case .hostPort(let host, let port) = connection.currentPath?.remoteEndpoint {
                        finish((hostString(host), Int(port.rawValue)))
                    } else {
                        finish(nil)
                    }
                case .failed, .cancelled:
                    finish(nil)
                default:
                    break
                }
            }
            connection.start(queue: .global(qos: .userInitiated))
            Task { @Sendable in
                try? await Task.sleep(for: timeout)
                finish(nil)
            }
        }
    }

    private nonisolated static func hostString(_ host: NWEndpoint.Host) -> String {
        switch host {
        case .ipv4(let address):
            // Drop an interface suffix like "%en0" for IPv4.
            return "\(address)".split(separator: "%").first.map(String.init) ?? "\(address)"
        case .ipv6(let address):
            return "\(address)"
        case .name(let name, _):
            return name
        @unknown default:
            return "\(host)"
        }
    }
}

/// Builds `ws://host:port/link`, bracketing IPv6 literals and escaping the zone id.
func linkURL(host: String, port: Int) -> URL? {
    var h = host
    if h.contains(":") {
        h = "[" + h.replacingOccurrences(of: "%", with: "%25") + "]"
    }
    return URL(string: "ws://\(h):\(port)\(LinkProtocol.path)")
}

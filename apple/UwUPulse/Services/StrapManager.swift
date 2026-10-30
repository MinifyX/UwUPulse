import CoreBluetooth
import Foundation
import Observation
import PulseKit

/// BLE heart-rate chest straps (service 0x180D, characteristic 0x2A37).
/// Remembers the chosen strap, reconnects to it automatically and restores state after a relaunch.
@MainActor @Observable
final class StrapManager: NSObject {
    struct Found: Identifiable, Equatable {
        let id: UUID
        var name: String
        var rssi: Int
    }

    enum ConnectionState: Equatable { case disconnected, connecting, connected }

    private(set) var bluetoothState: CBManagerState = .unknown
    private(set) var isScanning = false
    private(set) var found: [Found] = []
    private(set) var strapName: String?
    private(set) var connectionState: ConnectionState = .disconnected
    private(set) var lastMeasurement: HeartRateMeasurement?
    private(set) var savedStrapID: UUID?

    @ObservationIgnored var onMeasurement: ((HeartRateMeasurement, String) -> Void)?
    @ObservationIgnored var onStateChange: (() -> Void)?

    private var central: CBCentralManager?
    private var peripheral: CBPeripheral?
    private var reconnectTask: Task<Void, Never>?

    private static let savedKey = "strap.id"
    private static let restoreID = "app.uwupulse.ios.central"
    private static let hrService = CBUUID(string: HeartRateMeasurement.serviceUUID)
    private static let hrMeasurement = CBUUID(string: HeartRateMeasurement.characteristicUUID)

    override init() {
        super.init()
        savedStrapID = UserDefaults.standard.string(forKey: Self.savedKey).flatMap(UUID.init(uuidString:))
    }

    /// Creates the central. Only when a strap is saved or the user opens the strap screen,
    /// so the Bluetooth permission prompt does not appear on first launch.
    func activate() {
        guard central == nil else { return }
        central = CBCentralManager(delegate: self, queue: .main, options: [
            CBCentralManagerOptionRestoreIdentifierKey: Self.restoreID,
        ])
    }

    func activateIfStrapSaved() {
        if savedStrapID != nil { activate() }
    }

    var connectedNames: [String] {
        connectionState == .connected ? [strapName ?? String(localized: "Chest strap")] : []
    }

    func startScan() {
        activate()
        guard let central, central.state == .poweredOn else { isScanning = true; return }
        found.removeAll()
        central.scanForPeripherals(withServices: [Self.hrService], options: [CBCentralManagerScanOptionAllowDuplicatesKey: false])
        isScanning = true
    }

    func stopScan() {
        central?.stopScan()
        isScanning = false
    }

    func connect(_ item: Found) {
        guard let central, let target = central.retrievePeripherals(withIdentifiers: [item.id]).first else { return }
        stopScan()
        if let peripheral, peripheral.identifier != target.identifier { central.cancelPeripheralConnection(peripheral) }
        savedStrapID = target.identifier
        UserDefaults.standard.set(target.identifier.uuidString, forKey: Self.savedKey)
        connect(target)
    }

    /// Disconnects and forgets the saved strap.
    func forget() {
        reconnectTask?.cancel()
        if let peripheral { central?.cancelPeripheralConnection(peripheral) }
        peripheral = nil
        savedStrapID = nil
        strapName = nil
        lastMeasurement = nil
        UserDefaults.standard.removeObject(forKey: Self.savedKey)
        connectionState = .disconnected
        onStateChange?()
    }

    private func connect(_ target: CBPeripheral) {
        peripheral = target
        target.delegate = self
        strapName = target.name ?? strapName
        connectionState = .connecting
        // Pending connects never time out, so this also covers a strap that is out of range.
        central?.connect(target, options: nil)
        onStateChange?()
    }

    private func reconnectSaved() {
        guard let central, central.state == .poweredOn, let id = savedStrapID, connectionState == .disconnected else { return }
        if let known = central.retrievePeripherals(withIdentifiers: [id]).first {
            connect(known)
        }
    }

    private func handleValue(_ data: Data, from peripheral: CBPeripheral) {
        guard let measurement = HeartRateMeasurement(data: data) else { return }
        lastMeasurement = measurement
        if measurement.sensorContact == false { return }
        guard LinkLimits.isValidBPM(measurement.bpm) else { return }
        onMeasurement?(measurement, (peripheral.name ?? strapName ?? "BLE").limited())
    }
}

// Delegate callbacks arrive on the main queue (see `activate`), so main-actor isolation holds.
extension StrapManager: @preconcurrency CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        bluetoothState = central.state
        if central.state == .poweredOn {
            if isScanning { startScan() }
            reconnectSaved()
        } else {
            connectionState = .disconnected
            onStateChange?()
        }
    }

    func centralManager(_ central: CBCentralManager, willRestoreState dict: [String: Any]) {
        guard let restored = (dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral])?
            .first(where: { $0.identifier == savedStrapID })
        else { return }
        peripheral = restored
        restored.delegate = self
        strapName = restored.name
        connectionState = restored.state == .connected ? .connected : .connecting
    }

    func centralManager(
        _ central: CBCentralManager, didDiscover peripheral: CBPeripheral,
        advertisementData: [String: Any], rssi RSSI: NSNumber
    ) {
        let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? peripheral.name ?? String(localized: "Chest strap")
        let item = Found(id: peripheral.identifier, name: name.limited(), rssi: RSSI.intValue)
        if let index = found.firstIndex(where: { $0.id == item.id }) {
            found[index] = item
        } else if found.count < 50 {
            found.append(item)
        }
    }

    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        guard peripheral.identifier == self.peripheral?.identifier else { return }
        connectionState = .connected
        strapName = peripheral.name ?? strapName
        peripheral.discoverServices([Self.hrService])
        onStateChange?()
    }

    func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral, error: Error?) {
        scheduleReconnect(peripheral)
    }

    func centralManager(
        _ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral, error: Error?
    ) {
        scheduleReconnect(peripheral)
    }

    private func scheduleReconnect(_ lost: CBPeripheral) {
        guard lost.identifier == peripheral?.identifier else { return }
        connectionState = .disconnected
        onStateChange?()
        guard savedStrapID == lost.identifier else { return }
        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            self?.reconnectSaved()
        }
    }
}

extension StrapManager: @preconcurrency CBPeripheralDelegate {
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard let service = peripheral.services?.first(where: { $0.uuid == Self.hrService }) else { return }
        peripheral.discoverCharacteristics([Self.hrMeasurement], for: service)
    }

    func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        guard let characteristic = service.characteristics?.first(where: { $0.uuid == Self.hrMeasurement }) else { return }
        peripheral.setNotifyValue(true, for: characteristic)
    }

    func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        guard error == nil, characteristic.uuid == Self.hrMeasurement, let data = characteristic.value else { return }
        handleValue(data, from: peripheral)
    }
}

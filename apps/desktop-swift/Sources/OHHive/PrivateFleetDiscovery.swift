import Foundation
import Combine
import Darwin

/// Bonjour locates candidates only. Pairing codes and signed enrollment still establish trust.
@MainActor
final class PrivateFleetDiscovery: NSObject, ObservableObject, @preconcurrency NetServiceBrowserDelegate, @preconcurrency NetServiceDelegate {
    struct Computer: Identifiable, Equatable {
        let id: String
        let name: String
        let endpoint: String
        let approvalEndpoint: String?
    }
    static let serviceType = "_lokisden._tcp."
    @Published private(set) var computers: [Computer] = []
    @Published private(set) var message: String?
    private var browser: NetServiceBrowser?
    private var resolving: [String: NetService] = [:]

    func start() {
        guard browser == nil else { return }
        message = nil
        let browser = NetServiceBrowser()
        self.browser = browser
        browser.delegate = self
        browser.searchForServices(ofType: Self.serviceType, inDomain: "local.")
    }
    func stop() {
        browser?.stop(); browser?.delegate = nil; browser = nil
        for service in resolving.values { service.stop(); service.delegate = nil }
        resolving = [:]; computers = []
    }
    private func identifier(_ service: NetService) -> String { service.domain + service.type + service.name }
    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        guard self.browser === browser else { return }
        let id = identifier(service)
        resolving[id]?.stop()
        resolving[id] = service; service.delegate = self; service.resolve(withTimeout: 5)
    }
    func netServiceBrowser(_ browser: NetServiceBrowser, didRemove service: NetService, moreComing: Bool) {
        guard self.browser === browser else { return }
        let id = identifier(service)
        resolving.removeValue(forKey: id)?.stop()
        computers.removeAll { $0.id == id }
    }
    func netServiceBrowser(_ browser: NetServiceBrowser, didNotSearch errorDict: [String: NSNumber]) {
        guard self.browser === browser else { return }
        message = "Nearby computers couldn’t be found. Allow Local Network access for Loki’s Den in System Settings, then try again."
        stop()
    }
    func netServiceDidResolveAddress(_ sender: NetService) {
        let id = identifier(sender)
        guard resolving[id] === sender, (1...65535).contains(sender.port) else { return }
        let local = Set(FleetNetwork.localAddresses())
        let addresses = (sender.addresses ?? []).compactMap { data -> String? in
            data.withUnsafeBytes { bytes in
                guard let pointer = bytes.baseAddress, bytes.count >= MemoryLayout<sockaddr_in>.size else { return nil }
                let address = pointer.assumingMemoryBound(to: sockaddr.self)
                guard address.pointee.sa_family == sa_family_t(AF_INET) else { return nil }
                var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                guard getnameinfo(address, socklen_t(MemoryLayout<sockaddr_in>.size), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { return nil }
                let ip = String(cString: host)
                return FleetNetwork.isPrivateIPv4(ip) ? ip : nil
            }
        }
        // Do not offer this computer as its own primary.
        guard let data = sender.txtRecordData(),
              let bound = NetService.dictionary(fromTXTRecord: data)["address"],
              let ip = String(data: bound, encoding: .utf8),
              let endpoint = FleetNetwork.discoveredEndpoint(address: ip, port: sender.port, resolved: addresses, local: local) else { return }
        let record = NetService.dictionary(fromTXTRecord: data)
        let approvalPort = record["approval-port"].flatMap { String(data: $0, encoding: .utf8) }.flatMap(Int.init)
        let approvalEndpoint = approvalPort.flatMap { FleetNetwork.discoveredEndpoint(address: ip, port: $0, resolved: addresses, local: local) }
        let computer = Computer(id: id, name: sender.name, endpoint: endpoint, approvalEndpoint: approvalEndpoint)
        computers.removeAll { $0.id == id }; computers.append(computer)
        computers.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

@MainActor
final class PrivateFleetAdvertisement: NSObject, ObservableObject, @preconcurrency NetServiceDelegate {
    @Published private(set) var message: String?
    let pairing = FleetPairingApproval()
    private var service: NetService?
    func start(address: String, port: Int32, issueCode: @escaping () async throws -> String) async throws {
        stop()
        let approvalPort = try await pairing.start(address: address, issueCode: issueCode)
        let service = NetService(domain: "local.", type: PrivateFleetDiscovery.serviceType,
                                 name: Host.current().localizedName ?? "Loki’s Den", port: port)
        self.service = service; service.delegate = self
        service.setTXTRecord(NetService.data(fromTXTRecord: ["address": Data(address.utf8), "approval-port": Data(String(approvalPort).utf8)]))
        service.publish()
    }
    func stop() { service?.stop(); service?.delegate = nil; service = nil; pairing.stop(); message = nil }
    func netService(_ sender: NetService, didNotPublish errorDict: [String: NSNumber]) {
        guard service === sender else { return }
        message = "Sharing is running, but nearby discovery is unavailable. Check Local Network access in System Settings."
    }
}

enum FleetNetwork {
    static func discoveredEndpoint(address: String, port: Int, resolved: [String], local: Set<String>) -> String? {
        guard (1...65535).contains(port), isPrivateIPv4(address), resolved.contains(address),
              !resolved.contains(where: local.contains) else { return nil }
        return "http://\(address):\(port)"
    }
    static func isPrivateIPv4(_ value: String) -> Bool {
        let parts = value.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return false }
        let bytes = parts.compactMap { UInt8($0) }
        guard bytes.count == 4, zip(parts, bytes).allSatisfy({ String($0.1) == $0.0 }) else { return false }
        return bytes[0] == 10 || (bytes[0] == 172 && (16...31).contains(bytes[1])) ||
            (bytes[0] == 192 && bytes[1] == 168) || (bytes[0] == 169 && bytes[1] == 254)
    }
    static func localAddresses() -> [String] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else { return [] }
        defer { freeifaddrs(head) }
        var result: [(String, String)] = []
        var current: UnsafeMutablePointer<ifaddrs>? = first
        while let entry = current {
            defer { current = entry.pointee.ifa_next }
            guard let address = entry.pointee.ifa_addr,
                  address.pointee.sa_family == sa_family_t(AF_INET),
                  entry.pointee.ifa_flags & UInt32(IFF_UP) != 0,
                  entry.pointee.ifa_flags & UInt32(IFF_LOOPBACK) == 0 else { continue }
            let name = String(cString: entry.pointee.ifa_name)
            // LAN only; don't silently expose the listener on a VPN or virtual tunnel.
            guard name.hasPrefix("en") || name.hasPrefix("bridge") else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(address, socklen_t(MemoryLayout<sockaddr_in>.size), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { continue }
            let ip = String(cString: host)
            if isPrivateIPv4(ip) { result.append((name, ip)) }
        }
        return result.sorted {
            let left = ($0.1.hasPrefix("169.254.") ? 2 : 0) + ($0.0.hasPrefix("en") ? 0 : 1)
            let right = ($1.1.hasPrefix("169.254.") ? 2 : 0) + ($1.0.hasPrefix("en") ? 0 : 1)
            return left == right ? $0.0.localizedStandardCompare($1.0) == .orderedAscending : left < right
        }.map(\.1)
    }
}

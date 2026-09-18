import Foundation
import Combine
import Network

/// The LAN request channel only exchanges a one-use pairing code after a local click.
/// The existing signed fleet enrollment still gates access to private data.
@MainActor
final class FleetPairingApproval: ObservableObject {
    struct Request: Identifiable {
        let id: String
        let name: String
        let expires: Date
        var issuing = false
    }
    @Published private(set) var requests: [Request] = []
    @Published private(set) var error: String?
    private struct Ticket { let expires: Date; var state: String; var code: String? }
    private var tickets: [String: Ticket] = [:]
    private var listener: NWListener?
    private var connections: [UUID: NWConnection] = [:]
    private var ready: CheckedContinuation<UInt16, Error>?
    private var expiryTask: Task<Void, Never>?
    private var issueCode: (() async throws -> String)?

    func start(address: String, issueCode: @escaping () async throws -> String) async throws -> UInt16 {
        stop(); self.issueCode = issueCode
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: NWEndpoint.Host(address), port: .any)
        let socket = try NWListener(using: parameters, on: .any)
        listener = socket
        socket.newConnectionHandler = { [weak self] connection in
            Task { @MainActor in self?.accept(connection) }
        }
        socket.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in
                guard let self, self.listener === socket else { return }
                switch state {
                case .ready:
                    if let port = socket.port?.rawValue { self.ready?.resume(returning: port); self.ready = nil }
                case .failed, .cancelled:
                    self.ready?.resume(throwing: FleetPairingError.unavailable); self.ready = nil
                    self.error = "Pairing requests are unavailable. Stop sharing and try again."
                default: break
                }
            }
        }
        expiryTask = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                self?.prune()
            }
        }
        let port = try await withCheckedThrowingContinuation { continuation in
            ready = continuation
            socket.start(queue: .main)
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(5))
                guard let self, self.listener === socket, self.ready != nil else { return }
                self.ready?.resume(throwing: FleetPairingError.unavailable); self.ready = nil; self.stop()
            }
        }
        return port
    }
    func stop() {
        ready?.resume(throwing: CancellationError()); ready = nil
        listener?.cancel(); listener = nil; expiryTask?.cancel(); expiryTask = nil
        for connection in connections.values { connection.cancel() }
        connections = [:]; requests = []; tickets = [:]; issueCode = nil; error = nil
    }
    func approve(_ id: String) async {
        prune()
        guard let index = requests.firstIndex(where: { $0.id == id }), !requests[index].issuing, let issueCode else { return }
        requests[index].issuing = true
        do {
            let code = try await issueCode()
            prune()
            guard var ticket = tickets[id], ticket.state == "pending" else { return }
            ticket.state = "approved"; ticket.code = code; tickets[id] = ticket
            requests.removeAll { $0.id == id }
        } catch {
            self.error = "Approval did not finish. Please try again."
            if let index = requests.firstIndex(where: { $0.id == id }) { requests[index].issuing = false }
        }
    }
    func decline(_ id: String) {
        guard var ticket = tickets[id], ticket.state == "pending" else { return }
        ticket.state = "declined"; tickets[id] = ticket
        requests.removeAll { $0.id == id }
    }
    private func prune() {
        let now = Date()
        tickets = tickets.filter { $0.value.expires > now }
        requests.removeAll { $0.expires <= now }
    }
    private func accept(_ connection: NWConnection) {
        guard listener != nil, connections.count < 16 else { connection.cancel(); return }
        let id = UUID(); connections[id] = connection
        connection.start(queue: .main)
        read(connection, id: id, buffer: Data())
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(10))
            self?.connections.removeValue(forKey: id)?.cancel()
        }
    }
    private func read(_ connection: NWConnection, id: UUID, buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] data, _, done, error in
            Task { @MainActor in
                guard let self, self.connections[id] != nil else { connection.cancel(); return }
                var buffer = buffer
                if let data { buffer.append(data) }
                do {
                    guard error == nil else { throw FleetPairingError.unavailable }
                    if let request = try FleetPairingHTTP.parse(buffer) {
                        self.handle(request, connection: connection, id: id)
                    } else if !done { self.read(connection, id: id, buffer: buffer) }
                    else { self.close(connection, id: id) }
                } catch { self.respond(["error": "invalid_request"], status: 400, connection: connection, id: id) }
            }
        }
    }
    private func handle(_ request: FleetPairingHTTP.Request, connection: NWConnection, id: UUID) {
        prune()
        guard let body = try? JSONSerialization.jsonObject(with: request.body) as? [String: String] else {
            respond(["error": "invalid_request"], status: 400, connection: connection, id: id); return
        }
        if request.path == "/request", body.count == 1, let name = body["name"], FleetPairingHTTP.validName(name) {
            guard tickets.count < 8 else { respond(["error": "busy"], status: 429, connection: connection, id: id); return }
            let ticket = UUID().uuidString + UUID().uuidString
            let expires = Date().addingTimeInterval(180)
            tickets[ticket] = Ticket(expires: expires, state: "pending")
            requests.append(Request(id: ticket, name: name, expires: expires))
            respond(["ticket": ticket], status: 200, connection: connection, id: id)
        } else if request.path == "/poll", body.count == 1, let idValue = body["ticket"], let ticket = tickets[idValue] {
            var result = ["state": ticket.state]
            if let code = ticket.code { result["code"] = code }
            respond(result, status: 200, connection: connection, id: id)
        } else { respond(["error": "request_expired"], status: 404, connection: connection, id: id) }
    }
    private func close(_ connection: NWConnection, id: UUID) { connections.removeValue(forKey: id); connection.cancel() }
    private func respond(_ body: [String: String], status: Int, connection: NWConnection, id: UUID) {
        let data = (try? JSONSerialization.data(withJSONObject: body)) ?? Data()
        let header = "HTTP/1.1 \(status) Response\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: \(data.count)\r\nConnection: close\r\n\r\n"
        connection.send(content: Data(header.utf8) + data, completion: .contentProcessed { [weak self] _ in
            Task { @MainActor in self?.close(connection, id: id) }
        })
    }
}

enum FleetPairingHTTP {
    struct Request { let path: String; let body: Data }
    static func validName(_ name: String) -> Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 100 &&
        !name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
    }
    static func parse(_ data: Data) throws -> Request? {
        guard data.count <= 8192 else { throw FleetPairingError.unavailable }
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        guard let head = String(data: data[..<end.lowerBound], encoding: .utf8) else { throw FleetPairingError.unavailable }
        let lines = head.components(separatedBy: "\r\n")
        let first = lines[0].split(separator: " ")
        guard first.count == 3, first[0] == "POST", first[2] == "HTTP/1.1", ["/request", "/poll"].contains(String(first[1])) else { throw FleetPairingError.unavailable }
        var headers: [String: String] = [:]
        for line in lines.dropFirst() {
            let pieces = line.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
            guard pieces.count == 2 else { throw FleetPairingError.unavailable }
            let key = pieces[0].lowercased()
            guard headers[key] == nil else { throw FleetPairingError.unavailable }
            headers[key] = pieces[1].trimmingCharacters(in: .whitespaces)
        }
        guard headers["transfer-encoding"] == nil, headers["origin"] == nil,
              headers["content-type"]?.lowercased() == "application/json",
              let raw = headers["content-length"], let length = Int(raw), (1...2048).contains(length) else { throw FleetPairingError.unavailable }
        let body = data[end.upperBound...]
        guard body.count >= length else { return nil }
        guard body.count == length else { throw FleetPairingError.unavailable }
        return Request(path: String(first[1]), body: Data(body))
    }
}

enum FleetPairingError: LocalizedError {
    case unavailable, declined, expired
    var errorDescription: String? {
        switch self {
        case .unavailable: return "Could not reach pairing on your primary. Check that it is available and try again."
        case .declined: return "Your primary declined this connection."
        case .expired: return "The connection request expired. Please try again."
        }
    }
}

/// A separate short-lived session, with redirects disabled and no ambient cookies/credentials.
private final class FleetPairingSessionDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) { completionHandler(nil) }
}
enum FleetPairingClient {
    static func requestCode(endpoint: String, name: String) async throws -> String {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 8; config.httpShouldSetCookies = false; config.urlCredentialStorage = nil
        let session = URLSession(configuration: config, delegate: FleetPairingSessionDelegate(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        func post(_ path: String, _ body: [String: String]) async throws -> [String: String] {
            guard let url = URL(string: endpoint + path) else { throw FleetPairingError.unavailable }
            var request = URLRequest(url: url); request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            let (bytes, response) = try await session.bytes(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw FleetPairingError.unavailable }
            var data = Data()
            for try await byte in bytes { data.append(byte); if data.count > 4096 { throw FleetPairingError.unavailable } }
            guard let result = try JSONSerialization.jsonObject(with: data) as? [String: String] else { throw FleetPairingError.unavailable }
            return result
        }
        let result = try await post("/request", ["name": name])
        guard let ticket = result["ticket"], ticket.count <= 128 else { throw FleetPairingError.unavailable }
        let until = Date().addingTimeInterval(175)
        while Date() < until {
            try Task.checkCancellation()
            let result = try await post("/poll", ["ticket": ticket])
            switch result["state"] {
            case "approved":
                guard let code = result["code"], !code.isEmpty, code.count <= 256 else { throw FleetPairingError.unavailable }
                return code
            case "declined": throw FleetPairingError.declined
            case "pending": try await Task.sleep(for: .seconds(1))
            default: throw FleetPairingError.unavailable
            }
        }
        throw FleetPairingError.expired
    }
}

import AppKit
import Network
import SwiftUI

/// Browser transports a signed enrollment assertion; Rust remains the identity authority.
@MainActor
final class PrivateFleetSignIn: ObservableObject {
    @Published private(set) var busy = false
    @Published private(set) var message: String?
    @Published private(set) var completed = false
    @Published private(set) var progress = "Finish signing in in your browser…"
    private var activeState: String?
    private var task: Task<Void, Never>?
    private var listener: NWListener?
    private var pending: CheckedContinuation<String, Error>?
    private var timeout: Task<Void, Never>?

    func start(store: HiveStore, primaryEndpoint: String? = nil, pairingCode: String = "", approvalEndpoint: String? = nil) {
        guard !busy else { return }
        busy = true; message = nil; completed = false; progress = "Finish signing in in your browser…"
        task = Task {
            defer { activeState = nil; busy = false; listener?.cancel(); listener = nil; timeout?.cancel(); timeout = nil }
            do {
                let request: String
                if let primaryEndpoint {
                    var code = pairingCode
                    if let approvalEndpoint {
                        progress = "Approve this computer on your primary…"
                        code = try await FleetPairingClient.requestCode(endpoint: approvalEndpoint, name: Host.current().localizedName ?? "This Mac")
                    }
                    try Task.checkCancellation()
                    progress = "Finish signing in in your browser…"
                    request = try await store.privatePrimaryJoinBegin(endpoint: primaryEndpoint, code: code)
                } else {
                    request = try await store.privateFleetEnrollmentBegin()
                }
                try Task.checkCancellation()
                let (port, socket) = try await GoogleAuthManager.startLoopbackListener()
                listener = socket
                try Task.checkCancellation()
                let state = UUID().uuidString
                activeState = state
                let payload: [String: Any] = ["request":request, "state":state, "port":Int(port)]
                var url = URLComponents(string:"https://lokisden.app/private-fleet/enroll")!
                url.fragment = "desktop=" + (try JSONSerialization.data(withJSONObject: payload)).base64EncodedString()
                let approval: String = try await withCheckedThrowingContinuation { continuation in
                    pending = continuation
                    socket.newConnectionHandler = { connection in
                        connection.start(queue: .main)
                        Self.read(connection, buffer: Data()) { raw in
                            Task { @MainActor in
                                guard self.activeState == state, let approval = FleetSignInCallback.approval(raw, state:state) else {
                                    Self.respond(connection, status:"400 Bad Request", text:"This sign-in response was not accepted.")
                                    return
                                }
                                Self.respond(connection, status:"200 OK", text:"Sign-in response received. Return to Loki's Den; the app will confirm registration.")
                                self.finish(.success(approval))
                            }
                        }
                    }
                    timeout = Task {
                        do {
                            try await Task.sleep(for: .seconds(240))
                            if activeState == state { finish(.failure(FleetSignInError.expired)) }
                        } catch { }
                    }
                    if !NSWorkspace.shared.open(url.url!) { finish(.failure(FleetSignInError.browser)) }
                }
                try Task.checkCancellation()
                if primaryEndpoint != nil {
                    try await store.privatePrimaryJoinComplete(approval: approval)
                } else {
                    try await store.privateFleetEnrollmentComplete(approval: approval)
                }
                completed = true; store.refresh()
                message = primaryEndpoint == nil ? "This Mac is registered. Connect to an existing primary to see its agents, or start a new fleet here." : "Connected. Your primary’s agents are now available in Bots."
            } catch is CancellationError { message = nil }
            catch { message = "Registration did not finish. Please try again. \(error.localizedDescription)" }
        }
    }
    func cancel() {
        activeState = nil; task?.cancel(); listener?.cancel(); timeout?.cancel()
        finish(.failure(CancellationError()))
    }
    private func finish(_ result: Result<String, Error>) {
        guard let continuation = pending else { return }
        pending = nil; listener?.cancel(); timeout?.cancel()
        continuation.resume(with: result)
    }
    private nonisolated static func read(_ connection: NWConnection, buffer: Data, completion: @escaping (String) -> Void) {
        connection.receive(minimumIncompleteLength:1, maximumLength:4096) { data, _, done, error in
            var buffer = buffer
            if let data { buffer.append(data) }
            guard error == nil, buffer.count <= 16384 else { connection.cancel(); return }
            if let raw = String(data:buffer, encoding:.utf8), raw.contains("\r\n\r\n") { completion(raw) }
            else if !done { read(connection, buffer:buffer, completion:completion) }
            else { connection.cancel() }
        }
        // Bound incomplete local connections independently of the overall sign-in timeout.
        if buffer.isEmpty {
            DispatchQueue.main.asyncAfter(deadline:.now() + 10) { connection.cancel() }
        }
    }
    private nonisolated static func respond(_ connection: NWConnection, status: String, text: String) {
        let response = "HTTP/1.1 \(status)\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: \(text.utf8.count)\r\nConnection: close\r\n\r\n\(text)"
        connection.send(content:Data(response.utf8), completion:.contentProcessed { _ in connection.cancel() })
    }
}
private enum FleetSignInError: LocalizedError {
    case expired, browser
    var errorDescription: String? { self == .expired ? "The sign-in request expired." : "Could not open your browser." }
}

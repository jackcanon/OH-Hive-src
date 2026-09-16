import SwiftUI
import AppKit
import Network
import Security

// C-2: GitHub.com OAuth App, browser + loopback + PKCE, separate per-Mac Keychain.
// Deliberate provider-specific implementation alongside Google, per Claude's handoff.
// Uses the existing user-supplied client setup; shared-client rollout remains undecided.
// No repo write scope, Git credentials, model tools or production OAuth verification implied.
@MainActor
final class GitHubAuthManager: ObservableObject {
    @Published var clientID = GitHubKeychain.get("clientID") ?? ""
    @Published var clientSecret = GitHubKeychain.get("clientSecret") ?? ""
    @Published private(set) var isConnected = GitHubKeychain.get("session") != nil
    @Published private(set) var busy = false
    @Published private(set) var login: String?
    @Published private(set) var repositories: [GitHubRepository] = []
    @Published private(set) var lastError: String?
    private var generation = UUID()

    func disconnect() {
        generation = UUID()
        GitHubKeychain.removeAll()
        isConnected = false; login = nil; repositories = []
        clientID = ""; clientSecret = ""
        lastError = GitHubKeychain.get("session") == nil ? nil : "Could not remove GitHub credentials. Try disconnecting again."
    }
    func connect() async {
        guard !busy else { return }
        busy = true; lastError = nil
        defer { busy = false }
        let attempt = generation
        do {
            let id = clientID.trimmingCharacters(in: .whitespacesAndNewlines)
            let secret = clientSecret.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !id.isEmpty, !secret.isEmpty else { throw GitHubConnectorError.denied("Enter the OAuth client ID and secret.") }
            try GitHubKeychain.set("clientID", id)
            try GitHubKeychain.set("clientSecret", secret)
            clientID = id; clientSecret = secret
            let pkce = PKCE.generate(), state = PKCE.generate().verifier
            let (port, listener) = try await Self.startLoopbackListener()
            defer { listener.cancel() }
            let redirect = "http://127.0.0.1:\(port)/oauth2callback"
            var url = URLComponents(string: "https://github.com/login/oauth/authorize")!
            url.queryItems = ["client_id": id, "redirect_uri": redirect, "scope": "read:user", "state": state,
                              "code_challenge": pkce.challenge, "code_challenge_method": "S256"].map { URLQueryItem(name: $0.key, value: $0.value) }
            guard NSWorkspace.shared.open(url.url!) else { throw GitHubConnectorError.badURL }
            let callback = try await Self.waitForCallback(listener: listener, expectedState: state)
            guard let code = callback["code"], !code.isEmpty else { throw GitHubConnectorError.denied("Sign-in was not completed.") }
            let token = try await exchange(["client_id": id, "client_secret": secret, "code": code, "redirect_uri": redirect, "code_verifier": pkce.verifier])
            let user: GitHubUser = try await request("https://api.github.com/user", token: token.access_token)
            guard generation == attempt else { return }
            try save(token)
            login = user.login; isConnected = true; repositories = []
        } catch { lastError = describe(error); isConnected = false }
    }
    func loadPublicRepositories() async {
        guard !busy, isConnected else { return }
        busy = true; lastError = nil
        defer { busy = false }
        let attempt = generation
        do {
            let token = try await accessToken()
            let repos: [GitHubRepository] = try await request("https://api.github.com/user/repos?visibility=public&per_page=100&sort=updated", token: token)
            guard generation == attempt else { return }
            repositories = repos
        } catch { lastError = describe(error) }
    }
    private func accessToken() async throws -> String {
        guard let raw = GitHubKeychain.get("session"), let data = raw.data(using: .utf8) else { throw GitHubConnectorError.notConnected }
        let session = try JSONDecoder().decode(GitHubSession.self, from: data)
        if let expiry = session.expiresAt, expiry <= Date().timeIntervalSince1970 + 60 {
            guard let refresh = session.refreshToken else { throw GitHubConnectorError.notConnected }
            let token = try await exchange(["client_id": clientID, "client_secret": clientSecret, "grant_type": "refresh_token", "refresh_token": refresh])
            let user: GitHubUser = try await request("https://api.github.com/user", token: token.access_token)
            try save(token); login = user.login
            return token.access_token
        }
        return session.accessToken
    }
    private func save(_ token: GitHubToken) throws {
        let session = GitHubSession(accessToken: token.access_token, refreshToken: token.refresh_token,
            expiresAt: token.expires_in.map { Date().timeIntervalSince1970 + $0 })
        let data = try JSONEncoder().encode(session)
        try GitHubKeychain.set("session", String(decoding: data, as: UTF8.self))
    }
    private func exchange(_ values: [String: String]) async throws -> GitHubToken {
        var req = URLRequest(url: URL(string: "https://github.com/login/oauth/access_token")!)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: values)
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.check(response)
        let token = try JSONDecoder().decode(GitHubToken.self, from: data)
        guard !token.access_token.isEmpty, token.token_type.lowercased() == "bearer" else { throw GitHubConnectorError.notConnected }
        return token
    }
    private func request<T: Decodable>(_ url: String, token: String) async throws -> T {
        var req = URLRequest(url: URL(string: url)!)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue("Loki-Den", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.check(response)
        return try JSONDecoder().decode(T.self, from: data)
    }
    private static func check(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw GitHubConnectorError.denied("GitHub request failed (HTTP \((response as? HTTPURLResponse)?.statusCode ?? 0)). Reconnect if access expired or was revoked.")
        }
    }
    private func describe(_ error: Error) -> String {
        (error as? GitHubConnectorError)?.description ?? "GitHub connection failed. Check your app setup and try again."
    }
    private static func startLoopbackListener() async throws -> (UInt16, NWListener) {
        let params = NWParameters.tcp
        params.requiredInterfaceType = .loopback // never bind anything but 127.0.0.1/::1
        let listener = try NWListener(using: params, on: .any)
        return try await withCheckedThrowingContinuation { continuation in
            let box = GitHubResumeOnce(continuation)
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    if let port = listener.port?.rawValue {
                        box.succeed((port, listener))
                    } else {
                        box.fail(GitHubConnectorError.listener("ready with no assigned port"))
                    }
                case .failed(let error):
                    box.fail(GitHubConnectorError.listener(error.localizedDescription))
                default:
                    break
                }
            }
            listener.start(queue: .main)
        }
    }

    private static func waitForCallback(listener: NWListener, expectedState: String) async throws -> [String: String] {
        try await withCheckedThrowingContinuation { continuation in
            let box = GitHubResumeOnce(continuation)

            listener.newConnectionHandler = { connection in
                connection.start(queue: .main)
                Self.readRequest(connection) { paramsOrNil in
                    listener.cancel()
                    guard let params = paramsOrNil else {
                        box.fail(GitHubConnectorError.listener("couldn't read the browser's response"))
                        return
                    }
                    guard params["state"] == expectedState else {
                        box.fail(GitHubConnectorError.listener("state mismatch -- discarded a response that didn't match this attempt"))
                        return
                    }
                    box.succeed(params)
                }
            }
            listener.stateUpdateHandler = { state in
                if case .failed(let error) = state {
                    box.fail(GitHubConnectorError.listener(error.localizedDescription))
                }
            }
            // Abandon the attempt if the member never finishes the browser consent screen.
            DispatchQueue.main.asyncAfter(deadline: .now() + 180) {
                listener.cancel()
                box.fail(GitHubConnectorError.timedOut)
            }
        }
    }

    // `nonisolated`: called from `NWListener.newConnectionHandler`, a plain non-actor-isolated
    // closure per Network.framework's own API -- this method touches no actor state (just parses
    // bytes off the connection and hands the result to a completion callback), so it doesn't need
    // (and, as of this toolchain's stricter actor-isolation checking, can't have) the surrounding
    // class's @MainActor isolation.
    private nonisolated static func readRequest(_ connection: NWConnection, completion: @escaping ([String: String]?) -> Void) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { data, _, _, error in
            guard error == nil, let data, let text = String(data: data, encoding: .utf8),
                  let result = GitHubCallback.parse(text) else {
                connection.cancel()
                completion(nil)
                return
            }
            let html = "<html><body style=\"font-family:-apple-system;padding:40px;text-align:center\"><h2>Authorization response received.</h2><p>You can close this tab and go back to the app to finish connecting.</p></body></html>"
            let responseText = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: \(html.utf8.count)\r\nConnection: close\r\n\r\n\(html)"
            connection.send(content: responseText.data(using: .utf8), completion: .contentProcessed { _ in
                connection.cancel()
            })
            completion(result)
        }
    }
}


struct GitHubRepository: Decodable, Identifiable {
    let id: Int
    let full_name: String
    let html_url: String
    var safeURL: URL? {
        guard let url = URL(string: html_url), url.scheme == "https", url.host == "github.com", url.user == nil, url.password == nil else { return nil }
        return url
    }
}
private struct GitHubUser: Decodable { let login: String }
struct GitHubToken: Decodable { let access_token: String; let token_type: String; let refresh_token: String?; let expires_in: Double? }
private struct GitHubSession: Codable { let accessToken: String; let refreshToken: String?; let expiresAt: Double? }
enum GitHubConnectorError: Error, CustomStringConvertible {
    case keychain(OSStatus), badURL, notConnected, denied(String), listener(String), timedOut
    var description: String {
        switch self {
        case .keychain(let status): return "Could not store GitHub credentials (\(status))."
        case .badURL: return "Could not open GitHub sign-in."
        case .notConnected: return "Connect GitHub again to continue."
        case .denied(let reason), .listener(let reason): return reason
        case .timedOut: return "GitHub sign-in timed out. Try again."
        }
    }
}
private final class GitHubResumeOnce<T> {
    private var continuation: CheckedContinuation<T, Error>?
    private let lock = NSLock()

    init(_ continuation: CheckedContinuation<T, Error>) {
        self.continuation = continuation
    }

    func succeed(_ value: T) {
        lock.lock(); defer { lock.unlock() }
        continuation?.resume(returning: value)
        continuation = nil
    }

    func fail(_ error: Error) {
        lock.lock(); defer { lock.unlock() }
        continuation?.resume(throwing: error)
        continuation = nil
    }
}

enum GitHubKeychain {
    private static let service = "media.happyjack.hive.github"

    static func set(_ key: String, _ value: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key
        ]
        var attrs = query
        attrs[kSecValueData as String] = Data(value.utf8)
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let updated = SecItemUpdate(query as CFDictionary, [kSecValueData as String: Data(value.utf8)] as CFDictionary)
        let status = updated == errSecItemNotFound ? SecItemAdd(attrs as CFDictionary, nil) : updated
        try verifyWrite(status: status, stored: status == errSecSuccess ? get(key) : nil, expected: value)
    }

    static func verifyWrite(status: OSStatus, stored: String?, expected: String) throws {
        guard status == errSecSuccess else { throw GitHubConnectorError.keychain(status) }
        guard stored == expected else { throw GitHubConnectorError.keychain(errSecDecode) }
    }

    static func get(_ key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    static func remove(_ key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key
        ]
        SecItemDelete(query as CFDictionary)
    }

    static func removeAll() {
        for key in ["clientID", "clientSecret", "session"] {
            remove(key)
        }
    }
}



// Validate the exact callback route and reject ambiguous parameters before state checking.
enum GitHubCallback {
    static func parse(_ request: String) -> [String: String]? {
        guard let line = request.components(separatedBy: "\r\n").first else { return nil }
        let fields = line.split(separator: " ")
        guard fields.count == 3, fields[0] == "GET", fields[2].hasPrefix("HTTP/"),
              fields[1].hasPrefix("/oauth2callback?"),
              let url = URLComponents(string: "http://127.0.0.1" + fields[1]),
              url.path == "/oauth2callback" else { return nil }
        var result: [String: String] = [:]
        for item in url.queryItems ?? [] {
            guard result[item.name] == nil, let value = item.value else { return nil }
            result[item.name] = value
        }
        guard let state = result["state"], !state.isEmpty else { return nil }
        return result
    }
}

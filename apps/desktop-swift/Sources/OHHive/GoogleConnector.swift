import Foundation
import Combine
import Network
import AppKit
import CryptoKit
import Security

/// ADR-026 v1: Google Drive + Gmail for the Swift app, "let's build out Google Workspace to
/// start" (2026-09-14, Jack). Pure Swift + Keychain, no backend -- matches the ADR's "Swift app
/// only" scope and Decision 1's "no backend proxy needed for the OAuth flow itself."
///
/// **Redirect method note (2026-09-14):** ADR-026 originally planned `ASWebAuthenticationSession`
/// with a custom URL scheme. Checked against Google's current native-app OAuth guidance while
/// building this: Google has since deprecated custom URI-scheme redirects for installed-app OAuth
/// clients (app-impersonation risk) and now only supports the loopback-IP redirect
/// (`http://127.0.0.1:<port>`) for the Desktop-app client type. This file implements that instead:
/// open the system browser directly (still satisfies Google's embedded-WebView ban, same as
/// `ASWebAuthenticationSession` would have) and run a one-shot local HTTP listener, restricted to
/// the loopback interface only, to catch the single redirect and shut itself down immediately
/// after. This has not been build-verified against a real Google Cloud OAuth client yet -- Jack
/// still needs to create one and paste its Client ID/secret into the new Connectors settings tab
/// before the round trip can be tested for real.
///
/// Scope stays fixed at `drive.file` (app-created/picked files only) + `gmail.send` (send-only) --
/// see ADR-026 Decision 3. Do not widen this without a new ADR decision; broader scopes push Hive
/// into Google's CASA security-assessment tier.
@MainActor
final class GoogleAuthManager: ObservableObject {
    @Published var isConnected = false
    @Published var isConnecting = false
    @Published var lastError: String?
    @Published var clientID: String = GoogleKeychain.get("clientID") ?? ""
    @Published var clientSecret: String = GoogleKeychain.get("clientSecret") ?? ""

    static let scopes = "https://www.googleapis.com/auth/drive.file https://www.googleapis.com/auth/gmail.send"

    init() {
        isConnected = GoogleKeychain.get("refreshToken") != nil
    }

    func saveCredentials(id: String, secret: String) {
        let trimmedID = id.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedSecret = secret.trimmingCharacters(in: .whitespacesAndNewlines)
        clientID = trimmedID
        clientSecret = trimmedSecret
        GoogleKeychain.set("clientID", trimmedID)
        GoogleKeychain.set("clientSecret", trimmedSecret)
    }

    func disconnect() {
        GoogleKeychain.removeAll()
        clientID = ""
        clientSecret = ""
        isConnected = false
        lastError = nil
    }

    func connect() async {
        guard !clientID.isEmpty, !clientSecret.isEmpty else {
            lastError = "Enter this Hive Google OAuth client's ID and secret first (from Google Cloud Console)."
            return
        }
        isConnecting = true
        lastError = nil
        defer { isConnecting = false }
        do {
            let pkce = PKCE.generate()
            let state = PKCE.generate().verifier // reused only as a random per-attempt nonce

            let (port, listener) = try await Self.startLoopbackListener()
            let redirectURI = "http://127.0.0.1:\(port)/oauth2callback"

            guard var comps = URLComponents(string: "https://accounts.google.com/o/oauth2/v2/auth") else {
                throw GoogleConnectorError.badURL
            }
            comps.queryItems = [
                URLQueryItem(name: "client_id", value: clientID),
                URLQueryItem(name: "redirect_uri", value: redirectURI),
                URLQueryItem(name: "response_type", value: "code"),
                URLQueryItem(name: "scope", value: Self.scopes),
                URLQueryItem(name: "access_type", value: "offline"),
                URLQueryItem(name: "prompt", value: "consent"),
                URLQueryItem(name: "code_challenge", value: pkce.challenge),
                URLQueryItem(name: "code_challenge_method", value: "S256"),
                URLQueryItem(name: "state", value: state)
            ]
            guard let authURL = comps.url else { throw GoogleConnectorError.badURL }

            // System browser, not an embedded WebView -- required by Google's OAuth policy and
            // the correct call for a loopback-redirect flow (there's no app-side URL scheme to
            // intercept here; the local listener below is what catches the redirect).
            NSWorkspace.shared.open(authURL)

            let params = try await Self.waitForCallback(listener: listener, expectedState: state)
            guard let code = params["code"] else {
                throw GoogleConnectorError.denied(params["error"] ?? "no code returned")
            }

            try await exchangeCode(code: code, redirectURI: redirectURI, verifier: pkce.verifier)
            isConnected = true
        } catch {
            lastError = (error as? GoogleConnectorError)?.description ?? error.localizedDescription
        }
    }

    private func exchangeCode(code: String, redirectURI: String, verifier: String) async throws {
        var req = URLRequest(url: URL(string: "https://oauth2.googleapis.com/token")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Self.formEncode([
            "client_id": clientID,
            "client_secret": clientSecret,
            "code": code,
            "code_verifier": verifier,
            "grant_type": "authorization_code",
            "redirect_uri": redirectURI
        ])
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.checkOK(response, data)
        let token = try JSONDecoder().decode(GoogleTokenResponse.self, from: data)
        GoogleKeychain.set("accessToken", token.access_token)
        if let refresh = token.refresh_token {
            GoogleKeychain.set("refreshToken", refresh)
        }
        GoogleKeychain.set("tokenExpiry", String(Date().addingTimeInterval(TimeInterval(token.expires_in)).timeIntervalSince1970))
    }

    /// Every Drive/Gmail call routes through this -- never read the Keychain's accessToken
    /// directly, it may be stale. Refreshes automatically using the stored refresh token.
    func validAccessToken() async throws -> String {
        guard let refreshToken = GoogleKeychain.get("refreshToken") else {
            throw GoogleConnectorError.notConnected
        }
        let expiry = Double(GoogleKeychain.get("tokenExpiry") ?? "0") ?? 0
        if let token = GoogleKeychain.get("accessToken"), Date().timeIntervalSince1970 < expiry - 60 {
            return token
        }
        var req = URLRequest(url: URL(string: "https://oauth2.googleapis.com/token")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Self.formEncode([
            "client_id": clientID,
            "client_secret": clientSecret,
            "refresh_token": refreshToken,
            "grant_type": "refresh_token"
        ])
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.checkOK(response, data)
        let token = try JSONDecoder().decode(GoogleTokenResponse.self, from: data)
        GoogleKeychain.set("accessToken", token.access_token)
        GoogleKeychain.set("tokenExpiry", String(Date().addingTimeInterval(TimeInterval(token.expires_in)).timeIntervalSince1970))
        return token.access_token
    }

    private static func formEncode(_ dict: [String: String]) -> Data {
        let allowed = CharacterSet.urlQueryAllowed.subtracting(CharacterSet(charactersIn: "+&="))
        return dict.map { key, value -> String in
            let k = key.addingPercentEncoding(withAllowedCharacters: allowed) ?? key
            let v = value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
            return "\(k)=\(v)"
        }.joined(separator: "&").data(using: .utf8)!
    }

    private static func checkOK(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw GoogleConnectorError.http(String(data: data, encoding: .utf8) ?? "(no body)")
        }
    }

    // MARK: - Loopback redirect listener

    private static func startLoopbackListener() async throws -> (UInt16, NWListener) {
        let params = NWParameters.tcp
        params.requiredInterfaceType = .loopback // never bind anything but 127.0.0.1/::1
        let listener = try NWListener(using: params, on: .any)
        return try await withCheckedThrowingContinuation { continuation in
            let box = ResumeOnce(continuation)
            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    if let port = listener.port?.rawValue {
                        box.succeed((port, listener))
                    } else {
                        box.fail(GoogleConnectorError.listener("ready with no assigned port"))
                    }
                case .failed(let error):
                    box.fail(GoogleConnectorError.listener(error.localizedDescription))
                default:
                    break
                }
            }
            listener.start(queue: .main)
        }
    }

    private static func waitForCallback(listener: NWListener, expectedState: String) async throws -> [String: String] {
        try await withCheckedThrowingContinuation { continuation in
            let box = ResumeOnce(continuation)

            listener.newConnectionHandler = { connection in
                connection.start(queue: .main)
                Self.readRequest(connection) { paramsOrNil in
                    listener.cancel()
                    guard let params = paramsOrNil else {
                        box.fail(GoogleConnectorError.listener("couldn't read the browser's response"))
                        return
                    }
                    guard params["state"] == expectedState else {
                        box.fail(GoogleConnectorError.listener("state mismatch -- discarded a response that didn't match this attempt"))
                        return
                    }
                    box.succeed(params)
                }
            }
            listener.stateUpdateHandler = { state in
                if case .failed(let error) = state {
                    box.fail(GoogleConnectorError.listener(error.localizedDescription))
                }
            }
            // Abandon the attempt if the member never finishes the browser consent screen.
            DispatchQueue.main.asyncAfter(deadline: .now() + 180) {
                listener.cancel()
                box.fail(GoogleConnectorError.timedOut)
            }
        }
    }

    private static func readRequest(_ connection: NWConnection, completion: @escaping ([String: String]?) -> Void) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { data, _, _, error in
            guard error == nil, let data, let text = String(data: data, encoding: .utf8),
                  let requestLine = text.split(separator: "\r\n").first,
                  let pathPart = requestLine.split(separator: " ").dropFirst().first,
                  let url = URL(string: "http://127.0.0.1\(pathPart)"),
                  let comps = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
                completion(nil)
                return
            }
            var result: [String: String] = [:]
            for item in comps.queryItems ?? [] {
                result[item.name] = item.value ?? ""
            }
            let html = "<html><body style=\"font-family:-apple-system;padding:40px;text-align:center\"><h2>Hive is connected.</h2><p>You can close this tab and go back to the app.</p></body></html>"
            let responseText = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: \(html.utf8.count)\r\nConnection: close\r\n\r\n\(html)"
            connection.send(content: responseText.data(using: .utf8), completion: .contentProcessed { _ in
                connection.cancel()
            })
            completion(result)
        }
    }
}

// MARK: - Drive / Gmail actions (v1 scope per ADR-026 Decision 4)

extension GoogleAuthManager {
    /// v1 Drive action: the app creates a new file directly -- `drive.file` scope means this can
    /// only ever see files it created itself, never browse the member's existing Drive. A member
    /// file-picker flow is explicitly deferred (ADR-026 Decision 4).
    func createDriveFile(name: String, mimeType: String = "text/plain", content: String) async throws -> String {
        let token = try await validAccessToken()
        let boundary = "hive-\(UUID().uuidString)"
        var req = URLRequest(url: URL(string: "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart")!)
        req.httpMethod = "POST"
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("multipart/related; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        var body = Data()
        let metadata = try JSONSerialization.data(withJSONObject: ["name": name])
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        body.append("Content-Type: application/json; charset=UTF-8\r\n\r\n".data(using: .utf8)!)
        body.append(metadata)
        body.append("\r\n--\(boundary)\r\n".data(using: .utf8)!)
        body.append("Content-Type: \(mimeType)\r\n\r\n".data(using: .utf8)!)
        body.append(content.data(using: .utf8)!)
        body.append("\r\n--\(boundary)--".data(using: .utf8)!)
        req.httpBody = body

        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.checkOK(response, data)
        struct CreatedFile: Decodable { let id: String }
        return try JSONDecoder().decode(CreatedFile.self, from: data).id
    }

    /// v1 Gmail action: send-only -- cannot read, list, or search anything (ADR-026 Decision 4).
    func sendGmail(to: String, subject: String, body text: String) async throws {
        let token = try await validAccessToken()
        let raw = "To: \(to)\r\nSubject: \(subject)\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n\(text)"
        let encoded = Data(raw.utf8).base64URLEncodedString()
        var req = URLRequest(url: URL(string: "https://gmail.googleapis.com/gmail/v1/users/me/messages/send")!)
        req.httpMethod = "POST"
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: ["raw": encoded])
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.checkOK(response, data)
    }
}

// MARK: - Supporting types

private struct GoogleTokenResponse: Decodable {
    let access_token: String
    let expires_in: Int
    let refresh_token: String?
}

enum GoogleConnectorError: Error, CustomStringConvertible {
    case badURL
    case notConnected
    case denied(String)
    case listener(String)
    case timedOut
    case http(String)

    var description: String {
        switch self {
        case .badURL: return "Couldn't build the Google authorization URL."
        case .notConnected: return "Google isn't connected yet."
        case .denied(let reason): return "Google didn't return an authorization code: \(reason)"
        case .listener(let reason): return "Local sign-in listener failed: \(reason)"
        case .timedOut: return "Timed out waiting for Google sign-in \u{2014} try again."
        case .http(let body): return "Google returned an error: \(body.prefix(200))"
        }
    }
}

/// Guards a `CheckedContinuation` against being resumed twice -- the timeout and the connection
/// handler both race to resolve the same continuation, and only one may win.
private final class ResumeOnce<T> {
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

struct PKCE {
    let verifier: String
    let challenge: String

    static func generate() -> PKCE {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        let verifier = Data(bytes).base64URLEncodedString()
        let challenge = Data(SHA256.hash(data: Data(verifier.utf8))).base64URLEncodedString()
        return PKCE(verifier: verifier, challenge: challenge)
    }
}

extension Data {
    func base64URLEncodedString() -> String {
        base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

/// Local-only, per-machine token storage -- ADR-026 Decision 2: connecting Google on one paired
/// Mac does not connect it on another. Nothing here is ever sent to or through a Hive server.
enum GoogleKeychain {
    private static let service = "media.happyjack.hive.google"

    static func set(_ key: String, _ value: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key
        ]
        SecItemDelete(query as CFDictionary)
        var attrs = query
        attrs[kSecValueData as String] = Data(value.utf8)
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(attrs as CFDictionary, nil)
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
        for key in ["clientID", "clientSecret", "accessToken", "refreshToken", "tokenExpiry"] {
            remove(key)
        }
    }
}

import SwiftUI
import Security

@MainActor
final class GitHubAuthManager: ObservableObject {
    private let clientID = SharedConnectorConfiguration.githubClientID
    var isConfigured: Bool { !clientID.isEmpty }
    @Published private(set) var isConnected = false
    @Published private(set) var busy = false
    @Published private(set) var userCode: String?
    @Published private(set) var login: String?
    @Published private(set) var repositories: [GitHubRepository] = []
    @Published private(set) var lastError: String?
    @Published private(set) var copilotResult: CopilotCheckResult?
    private var generation = UUID()
    private var connectionTask: Task<Void, Never>?

    init() {
        isConnected = GitHubSession.read(GitHubKeychain.get("session"), clientID: clientID) != nil
    }
    func cancelConnection() {
        generation = UUID()
        connectionTask?.cancel(); connectionTask = nil
        busy = false; userCode = nil; copilotResult = nil
    }
    func disconnect() {
        cancelConnection()
        GitHubKeychain.removeAll()
        isConnected = false; login = nil; repositories = []
        lastError = GitHubKeychain.get("session") == nil ? nil : "Could not remove GitHub credentials. Try disconnecting again."
    }
    func connect() {
        guard !busy, isConfigured else { return }
        let attempt = UUID(); generation = attempt
        busy = true; lastError = nil
        connectionTask = Task { await performConnection(attempt) }
    }
    private func performConnection(_ attempt: UUID) async {
        defer {
            if generation == attempt { busy = false; userCode = nil; connectionTask = nil }
        }
        do {
            let data = try await post("https://github.com/login/device/code", ["client_id": clientID])
            try ensureCurrent(attempt)
            let device = try JSONDecoder().decode(GitHubDeviceCode.self, from: data)
            try device.validate()
            userCode = device.user_code
            let token = try await GitHubDeviceFlow.poll(device: device) {
                try await self.post("https://github.com/login/oauth/access_token", [
                    "client_id": self.clientID, "device_code": device.device_code,
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code"])
            }
            try ensureCurrent(attempt)
            let user: GitHubUser = try await request("https://api.github.com/user", token: token.access_token)
            try ensureCurrent(attempt)
            try save(token)
            login = user.login; isConnected = true; repositories = []
        } catch {
            if generation == attempt, !(error is CancellationError) { lastError = describe(error) }
        }
    }
    private func ensureCurrent(_ attempt: UUID) throws {
        try Task.checkCancellation()
        guard generation == attempt else { throw CancellationError() }
    }
    func loadRepositories() async {
        guard !busy, isConnected else { return }
        busy = true; lastError = nil
        let attempt = generation
        defer { if generation == attempt { busy = false } }
        do {
            let token = try await accessToken(attempt: attempt)
            let repos = try await GitHubRepositoryLoader.load { url in
                try await self.requestData(url, token: token)
            }
            try ensureCurrent(attempt)
            repositories = repos
        } catch {
            if generation == attempt, !(error is CancellationError) { lastError = describe(error) }
        }
    }
    func checkCopilot(model: String? = nil) async {
        guard !busy, isConnected else { return }
        busy = true; lastError = nil
        let attempt = generation
        defer { if generation == attempt { busy = false } }
        do {
            let token = try await accessToken(attempt: attempt)
            let user: GitHubUser = try await request("https://api.github.com/user", token: token)
            try ensureCurrent(attempt)
            let result = try await CopilotConnectionCheck.run(token: token, login: user.login, model: model)
            try ensureCurrent(attempt)
            if let error = result.error { throw GitHubConnectorError.denied(error) }
            guard result.login?.lowercased() == user.login.lowercased() else {
                throw GitHubConnectorError.denied("Copilot did not confirm the selected GitHub account.")
            }
            login = user.login
            copilotResult = result
        } catch {
            if generation == attempt, !(error is CancellationError) {
                copilotResult = nil
                lastError = describe(error)
            }
        }
    }
    private func accessToken(attempt: UUID) async throws -> String {
        guard let session = GitHubSession.read(GitHubKeychain.get("session"), clientID: clientID) else { throw GitHubConnectorError.notConnected }
        if let expiry = session.expiresAt, expiry <= Date().timeIntervalSince1970 + 60 {
            guard let refresh = session.refreshToken else { throw GitHubConnectorError.notConnected }
            let data = try await post("https://github.com/login/oauth/access_token", ["client_id": clientID, "grant_type": "refresh_token", "refresh_token": refresh])
            let token = try GitHubDeviceFlow.token(from: data)
            try ensureCurrent(attempt)
            // One Keychain value replaces both rotated tokens together.
            try save(token)
            return token.access_token
        }
        return session.accessToken
    }
    private func save(_ token: GitHubToken) throws {
        let session = GitHubSession(accessToken: token.access_token, refreshToken: token.refresh_token,
            expiresAt: token.expires_in.map { Date().timeIntervalSince1970 + $0 }, oauthClientID: clientID)
        try GitHubKeychain.set("session", String(decoding: JSONEncoder().encode(session), as: UTF8.self))
        GitHubKeychain.remove("clientID"); GitHubKeychain.remove("clientSecret")
    }
    private func post(_ url: String, _ values: [String: String]) async throws -> Data {
        var req = URLRequest(url: URL(string: url)!)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: values)
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.check(response)
        return data
    }
    private func request<T: Decodable>(_ url: String, token: String) async throws -> T {
        try JSONDecoder().decode(T.self, from: await requestData(url, token: token))
    }
    private func requestData(_ url: String, token: String) async throws -> Data {
        var req = URLRequest(url: URL(string: url)!)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue("Loki-Den", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await URLSession.shared.data(for: req)
        try Self.check(response)
        return data
    }
    private static func check(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw GitHubConnectorError.denied("GitHub request failed (HTTP \((response as? HTTPURLResponse)?.statusCode ?? 0)). Reconnect if access expired or was revoked.")
        }
    }
    private func describe(_ error: Error) -> String {
        (error as? GitHubConnectorError)?.description ?? "GitHub connection failed. Check your connection and try again."
    }
}

struct GitHubDeviceCode: Decodable {
    let device_code: String
    let user_code: String
    let verification_uri: String
    let expires_in: Double
    let interval: Double
    func validate() throws {
        guard !device_code.isEmpty, !user_code.isEmpty, verification_uri == "https://github.com/login/device",
              expires_in.isFinite, expires_in > 0, interval.isFinite, interval > 0 else { throw GitHubConnectorError.notConnected }
    }
}

// Injected time and transport keep polling tests independent of live accounts and Keychain.
enum GitHubDeviceFlow {
    private struct Failure: Decodable { let error: String; let interval: Double? }
    static func token(from data: Data) throws -> GitHubToken {
        let token = try JSONDecoder().decode(GitHubToken.self, from: data)
        guard !token.access_token.isEmpty, token.token_type.lowercased() == "bearer" else { throw GitHubConnectorError.notConnected }
        return token
    }
    static func poll(device: GitHubDeviceCode,
                     now: () -> Date = { Date() },
                     sleep: (Double) async throws -> Void = { try await Task.sleep(for: .seconds($0)) },
                     request: () async throws -> Data) async throws -> GitHubToken {
        try device.validate()
        let deadline = now().addingTimeInterval(device.expires_in)
        var interval = device.interval
        while true {
            try Task.checkCancellation()
            let remaining = deadline.timeIntervalSince(now())
            guard remaining > 0 else { throw GitHubConnectorError.timedOut }
            try await sleep(min(interval, remaining))
            try Task.checkCancellation()
            guard now() < deadline else { throw GitHubConnectorError.timedOut }
            let data = try await request()
            try Task.checkCancellation()
            guard now() < deadline else { throw GitHubConnectorError.timedOut }
            if let failure = try? JSONDecoder().decode(Failure.self, from: data) {
                switch failure.error {
                case "authorization_pending": continue
                case "slow_down": interval = max(interval + 5, failure.interval ?? 0)
                case "expired_token": throw GitHubConnectorError.timedOut
                case "access_denied": throw GitHubConnectorError.denied("GitHub sign-in was declined. You can try again.")
                default: throw GitHubConnectorError.denied("GitHub could not authorize this app. Check that device sign-in is enabled for this build’s GitHub App.")
                }
            } else { return try token(from: data) }
        }
    }
}
struct GitHubRepository: Decodable, Identifiable {
    let id: Int
    let full_name: String
    let html_url: String
    var `private`: Bool? = nil
    var safeURL: URL? {
        guard let url = URL(string: html_url), url.scheme == "https", url.host == "github.com", url.user == nil, url.password == nil else { return nil }
        return url
    }
}
private struct GitHubUser: Decodable { let login: String }
struct GitHubToken: Decodable { let access_token: String; let token_type: String; let refresh_token: String?; let expires_in: Double? }
struct GitHubSession: Codable {
    let accessToken: String
    let refreshToken: String?
    let expiresAt: Double?
    let oauthClientID: String
    static func read(_ raw: String?, clientID: String) -> GitHubSession? {
        guard !clientID.isEmpty, let raw, let session = try? JSONDecoder().decode(Self.self, from: Data(raw.utf8)),
              session.oauthClientID == clientID, !session.accessToken.isEmpty else { return nil }
        return session
    }
}
enum GitHubConnectorError: Error, CustomStringConvertible {
    case keychain(OSStatus), notConnected, denied(String), timedOut
    var description: String {
        switch self {
        case .keychain(let status): return "Could not store GitHub credentials (\(status))."
        case .notConnected: return "Connect GitHub again to continue."
        case .denied(let reason): return reason
        case .timedOut: return "GitHub sign-in timed out. Try again."
        }
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


/// Enumerate only repositories visible to both the signed-in user and this GitHub App.
/// The public list remains useful before installation. All requests stay on api.github.com.
enum GitHubRepositoryLoader {
    private struct Installation: Decodable { let id: Int }
    private struct Installations: Decodable { let total_count: Int; let installations: [Installation] }
    private struct Repositories: Decodable { let total_count: Int; let repositories: [GitHubRepository] }

    static func load(fetch: (String) async throws -> Data) async throws -> [GitHubRepository] {
        let decoder = JSONDecoder()
        var found: [Int: GitHubRepository] = [:]
        // Preserve existing public discovery; private discovery below is installation-scoped.
        let publicRepos = try decoder.decode([GitHubRepository].self, from: await fetch("https://api.github.com/user/repos?visibility=public&per_page=100&sort=updated"))
        for repo in publicRepos { found[repo.id] = repo }
        var installationCount = 0
        for page in 1...100 {
            try Task.checkCancellation()
            let batch = try decoder.decode(Installations.self, from: await fetch("https://api.github.com/user/installations?per_page=100&page=\(page)"))
            for installation in batch.installations {
                var repoCount = 0
                for repoPage in 1...100 {
                    try Task.checkCancellation()
                    let repos = try decoder.decode(Repositories.self, from: await fetch("https://api.github.com/user/installations/\(installation.id)/repositories?per_page=100&page=\(repoPage)"))
                    for repo in repos.repositories { found[repo.id] = repo }
                    repoCount += repos.repositories.count
                    if repoCount >= repos.total_count { break }
                    if repos.repositories.isEmpty || repoPage == 100 { throw GitHubConnectorError.denied("Repository listing was incomplete. Please try again or narrow the app’s repository access.") }
                }
            }
            installationCount += batch.installations.count
            if installationCount >= batch.total_count {
                return found.values.sorted { $0.full_name.localizedStandardCompare($1.full_name) == .orderedAscending }
            }
            if batch.installations.isEmpty { break }
        }
        throw GitHubConnectorError.denied("GitHub installation listing was incomplete. Please try again.")
    }
}

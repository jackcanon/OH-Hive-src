import Foundation

/// Publisher configuration, embedded in the signed app; never a member token.
/// Google Desktop client metadata is extractable by design, not a confidential secret.
enum SharedConnectorConfiguration {
    static var githubClientID: String {
        validatedGitHubClientID(Bundle.main.object(forInfoDictionaryKey: "HiveGitHubOAuthClientID") as? String)
    }
    static func validatedGitHubClientID(_ value: String?) -> String {
        guard let value, value.range(of: #"^[A-Za-z0-9_.-]+$"#, options: .regularExpression) != nil else { return "" }
        return value
    }
    static var googleClientSecret: String {
        Bundle.main.object(forInfoDictionaryKey: "HiveGoogleOAuthClientSecret") as? String ?? ""
    }
    static var googleClientID: String {
        validatedGoogleClientID(Bundle.main.object(forInfoDictionaryKey: "HiveGoogleOAuthClientID") as? String)
    }
    static func validatedGoogleClientID(_ value: String?) -> String {
        guard let value, value.range(of: #"^[A-Za-z0-9_-]+\.apps\.googleusercontent\.com$"#, options: .regularExpression) != nil else { return "" }
        return value
    }
}

import Foundation

/// Public publisher configuration, embedded in the signed app; never a member credential.
enum SharedConnectorConfiguration {
    static var githubClientID: String {
        validatedGitHubClientID(Bundle.main.object(forInfoDictionaryKey: "HiveGitHubOAuthClientID") as? String)
    }
    static func validatedGitHubClientID(_ value: String?) -> String {
        guard let value, value.range(of: #"^[A-Za-z0-9_.-]+$"#, options: .regularExpression) != nil else { return "" }
        return value
    }
    static var googleClientID: String {
        validatedGoogleClientID(Bundle.main.object(forInfoDictionaryKey: "HiveGoogleOAuthClientID") as? String)
    }
    static func validatedGoogleClientID(_ value: String?) -> String {
        guard let value, value.range(of: #"^[A-Za-z0-9_-]+\.apps\.googleusercontent\.com$"#, options: .regularExpression) != nil else { return "" }
        return value
    }
}

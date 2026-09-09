import AuthenticationServices
import CryptoKit
import Foundation
import Supabase
// import GoogleSignIn  -- added once the real Xcode project exists, see docs/IPHONE-APP-SCAFFOLD.md

/// Member identity for this app (ADR-021 §3) -- deliberately separate from the Mac app's node-key
/// pairing (ADR-004). Matches the web app's own two sign-in options exactly
/// (`apps/web/components/RequireMember.tsx`): Apple and Google, no email/password, no magic link.
@MainActor
final class AuthManager: NSObject, ObservableObject {
    @Published var session: Session?
    @Published var isLoading = true
    @Published var lastError: String?

    /// Nonce for the current Sign in with Apple attempt -- generated fresh per attempt and hashed
    /// into the request per Apple's replay-protection guidance; kept here so the delegate callback
    /// (which only gets the raw identity token) can pass the *unhashed* nonce through to Supabase.
    private var currentAppleNonce: String?

    override init() {
        super.init()
        Task { await restoreSession() }
    }

    private func restoreSession() async {
        isLoading = true
        defer { isLoading = false }
        session = try? await supabase.auth.session
    }

    func signOut() async {
        try? await supabase.auth.signOut()
        session = nil
    }

    // ---------- Sign in with Apple ----------

    func startSignInWithApple() -> ASAuthorizationAppleIDRequest {
        let nonce = Self.randomNonceString()
        currentAppleNonce = nonce
        let request = ASAuthorizationAppleIDProvider().createRequest()
        request.requestedScopes = [.fullName, .email]
        request.nonce = Self.sha256(nonce)
        return request
    }

    func completeSignInWithApple(_ authorization: ASAuthorization) async {
        guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
              let tokenData = credential.identityToken,
              let idToken = String(data: tokenData, encoding: .utf8),
              let nonce = currentAppleNonce else {
            lastError = "Apple sign-in didn't return a usable credential."
            return
        }
        do {
            let result = try await supabase.auth.signInWithIdToken(
                credentials: .init(provider: .apple, idToken: idToken, nonce: nonce)
            )
            session = result
        } catch {
            lastError = "Apple sign-in failed: \(error.localizedDescription)"
        }
    }

    // ---------- Google Sign-In ----------
    // Stubbed until GoogleSignIn-iOS is added in Xcode (docs/IPHONE-APP-SCAFFOLD.md). The shape is
    // the same as Apple's once GIDSignIn hands back an ID token: exchange it the same way.
    //
    // func signInWithGoogle(presenting: UIViewController) async {
    //     do {
    //         let result = try await GIDSignIn.sharedInstance.signIn(withPresenting: presenting)
    //         guard let idToken = result.user.idToken?.tokenString else {
    //             lastError = "Google sign-in didn't return an ID token."; return
    //         }
    //         session = try await supabase.auth.signInWithIdToken(
    //             credentials: .init(provider: .google, idToken: idToken)
    //         )
    //     } catch {
    //         lastError = "Google sign-in failed: \(error.localizedDescription)"
    //     }
    // }
    func signInWithGoogle() async {
        lastError = "Google Sign-In needs the GoogleSignIn-iOS package added in Xcode first \u{2014} see docs/IPHONE-APP-SCAFFOLD.md."
    }

    // ---------- nonce helpers (Apple's recommended implementation) ----------

    private static func randomNonceString(length: Int = 32) -> String {
        let charset: [Character] = Array("0123456789ABCDEFGHIJKLMNOPQRSTUVXYZabcdefghijklmnopqrstuvwxyz-._")
        var result = ""
        var remaining = length
        while remaining > 0 {
            var random: UInt8 = 0
            _ = SecRandomCopyBytes(kSecRandomDefault, 1, &random)
            if random < charset.count {
                result.append(charset[Int(random)])
                remaining -= 1
            }
        }
        return result
    }

    private static func sha256(_ input: String) -> String {
        SHA256.hash(data: Data(input.utf8)).compactMap { String(format: "%02x", $0) }.joined()
    }
}

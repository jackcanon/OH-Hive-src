import AuthenticationServices
import SwiftUI

struct SignInView: View {
    @EnvironmentObject private var auth: AuthManager

    var body: some View {
        VStack(spacing: 24) {
            Spacer()
            VStack(spacing: 8) {
                Image(systemName: "hexagon.fill").font(.system(size: 56)).foregroundStyle(.orange)
                Text("OH Hive").font(.largeTitle.bold())
                Text("Office Hours Global's compute-sharing network").font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            VStack(spacing: 12) {
                SignInWithAppleButton(.signIn, onRequest: { request in
                    let real = auth.startSignInWithApple()
                    request.requestedScopes = real.requestedScopes
                    request.nonce = real.nonce
                }, onCompletion: { result in
                    switch result {
                    case .success(let authorization):
                        Task { await auth.completeSignInWithApple(authorization) }
                    case .failure(let error):
                        auth.lastError = error.localizedDescription
                    }
                })
                .signInWithAppleButtonStyle(.black)
                .frame(height: 48)

                Button {
                    Task { await auth.signInWithGoogle() }
                } label: {
                    HStack {
                        Image(systemName: "g.circle.fill")
                        Text("Continue with Google")
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .frame(height: 48)

                if let error = auth.lastError {
                    Text(error).font(.caption).foregroundStyle(.red).multilineTextAlignment(.center)
                }
            }
            .padding(.horizontal, 32)
            Spacer()
        }
    }
}

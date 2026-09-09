import SwiftUI

@main
struct OHHiveMobileApp: App {
    @StateObject private var auth = AuthManager()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(auth)
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var auth: AuthManager

    var body: some View {
        Group {
            if auth.isLoading {
                ProgressView()
            } else if auth.session != nil {
                MainTabView()
            } else {
                SignInView()
            }
        }
    }
}

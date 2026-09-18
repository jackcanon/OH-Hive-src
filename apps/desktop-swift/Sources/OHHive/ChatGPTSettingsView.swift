import SwiftUI
import OHHiveFFI

struct ChatGPTSettingsView: View {
    @EnvironmentObject private var store: HiveStore
    @Environment(\.openURL) private var openURL
    @State private var status: ChatGptAccountStatus?
    @State private var busy = false
    @State private var binary = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Connect ChatGPT").font(.title2)
            Text("Use your ChatGPT account with Loki's Den. This preview connects your account; agent chat and fleet delegation are coming separately.")
                .foregroundStyle(.secondary)
            Text(status?.detail ?? "Connect your ChatGPT account to get started.")
            if let email = status?.email { Text(email).textSelection(.enabled) }
            if let plan = status?.plan { Text("Plan: \(plan)").foregroundStyle(.secondary) }
            if let code = status?.userCode { Text("Sign-in code: \(code)").font(.headline).textSelection(.enabled) }
            if let error { Text(error).foregroundStyle(.red) }
            HStack {
                if status?.state == "signing_in" {
                    Button("Open sign-in page") { openSignIn() }
                    Button("Cancel") { Task { await perform("cancel") } }
                } else if status?.state == "connected" {
                    Button("Disconnect") { Task { await perform("disconnect") } }
                } else {
                    Button("Connect ChatGPT") { Task { await perform("connect", openBrowser: true) } }
                    Button("Use device sign-in") { Task { await perform("device", openBrowser: true) } }
                }
                if busy { ProgressView().controlSize(.small) }
            }.disabled(busy)
            DisclosureGroup("Advanced") {
                VStack(alignment: .leading) {
                    Text("Requires the verified Codex 0.149.0 runtime. Loki's Den looks for it automatically.").foregroundStyle(.secondary)
                    TextField("Full path to Codex (optional)", text: $binary)
                        .textFieldStyle(.roundedBorder)
                }
            }
            Spacer()
        }
        .task {
            await perform("status")
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(2)) } catch { return }
                if status?.state == "signing_in" || status?.state == "connected" { await perform("status") }
            }
        }
    }

    @MainActor private func openSignIn() {
        guard let address = status?.authUrl, let url = URL(string: address) else { return }
        openURL(url)
    }

    @MainActor private func perform(_ action: String, openBrowser: Bool = false) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        error = nil
        do {
            status = try await store.chatgptAccount(action: action, binary: binary.isEmpty ? nil : binary)
            if openBrowser { openSignIn() }
        } catch { self.error = "Could not reach the account service. Please try again." }
    }
}

import SwiftUI

struct AboutYouSettingsView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var name = ""
    @State private var about = ""
    @State private var busy = false
    @State private var loaded = false
    @State private var message: String?
    private struct Profile: Decodable { let preferred_name: String; let about: String }
    var body: some View {
        GroupBox("About you") {
            VStack(alignment: .leading, spacing: 10) {
                TextField("What should we call you?", text: $name).textFieldStyle(.roundedBorder)
                TextField("What would you like your agents to know? (optional)", text: $about, axis: .vertical)
                    .lineLimit(3...6).textFieldStyle(.roundedBorder)
                Text("Shared with your private fleet’s agents for future replies. Cloud agents may receive this when you use them. Leave out passwords and secrets.")
                    .font(.caption).foregroundStyle(.secondary)
                if name.utf8.count > 120 || about.utf8.count > 2000 {
                    Text("Please shorten your name or background before saving.").font(.caption).foregroundStyle(.red)
                }
                HStack {
                    Button("Save") { Task { await save() } }.disabled(!loaded || busy || name.utf8.count > 120 || about.utf8.count > 2000)
                    if !loaded { Button("Try again") { Task { await load() } }.disabled(busy) }
                    if busy { ProgressView().controlSize(.small) }
                }
                if let message { Text(message).font(.caption).textSelection(.enabled) }
            }.disabled(busy).frame(maxWidth: .infinity, alignment: .leading)
        }.task { await load() }
    }
    private func load() async {
        busy = true; defer { busy = false }
        do {
            let json = try await store.bots.userProfile()
            let profile = try JSONDecoder().decode(Profile.self, from: Data(json.utf8))
            name = profile.preferred_name; about = profile.about; loaded = true; message = nil
        } catch { message = "Connect to your private fleet to load your profile. \(error.localizedDescription)" }
    }
    private func save() async {
        busy = true; defer { busy = false }
        do { try await store.bots.saveUserProfile(name: name, about: about); message = "Saved. Your agents will use this for future replies." }
        catch { message = "Could not save: \(error.localizedDescription)" }
    }
}

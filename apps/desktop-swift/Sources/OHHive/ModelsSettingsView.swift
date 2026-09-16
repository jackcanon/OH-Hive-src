import SwiftUI
import OHHiveFFI

/// Reuses setup's assessment/download path and the snapshot catalog used by the rest of the app.
struct ModelsSettingsView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var assessment: SetupAssessment?
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Local models").font(.headline)
                Text("Run models on your own computer. Choose an installed model or download one recommended for this Mac.")
                    .foregroundStyle(.secondary)
                if let error { SettingsNote(error) }
                if let a = assessment {
                    Label(a.ollama.running ? "Ollama is running" : "Ollama is not running",
                          systemImage: a.ollama.running ? "checkmark.circle" : "pause.circle")
                    if !a.ollama.running {
                        Button(a.ollama.installedApp == nil ? "Install Ollama" : "Start Ollama") {
                            Task { await install() }
                        }.disabled(busy)
                    }
                    if let recommendation = a.recommended {
                        GroupBox("Recommended for this Mac") {
                            VStack(alignment: .leading, spacing: 8) {
                                Text(recommendation.model).fontWeight(.semibold)
                                Text(recommendation.why).font(.caption)
                                Text("Download: \(formatStorageGB(recommendation.downloadBytes))").font(.caption)
                                if a.present.contains(where: { $0 == recommendation.model || $0.hasPrefix(recommendation.model + ":") }) {
                                    Text("Already installed").foregroundStyle(.secondary)
                                } else {
                                    Button("Download model") { Task { await download(recommendation.model) } }
                                        .disabled(busy || !a.ollama.running)
                                }
                            }.frame(maxWidth: .infinity, alignment: .leading)
                        }
                    } else {
                        SettingsNote("No model recommendation is available for this Mac. Installed models are listed below.")
                    }
                } else if busy { ProgressView("Checking this Mac…") }
                if let snap = store.snapshot {
                    GroupBox("Installed models") {
                        VStack(alignment: .leading, spacing: 8) {
                            if snap.models.isEmpty {
                                Text("No models found at your configured model server.")
                            } else {
                                Picker("Default model", selection: Binding(
                                    get: { snap.model ?? "" },
                                    set: { store.setConfig("HIVE_MODEL", $0) }
                                )) {
                                    Text("Automatic (first available)").tag("")
                                    if let selected = snap.model, !snap.models.contains(selected) {
                                        Text("\(selected) — unavailable").tag(selected)
                                    }
                                    ForEach(snap.models, id: \.self) { Text($0).tag($0) }
                                }
                                Text("Individual tasks may select a different model. Changes apply to the next task.")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Text("Connection details are in Advanced.").font(.caption).foregroundStyle(.secondary)
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                if busy, let progress = store.setupProgress {
                    Text(progress.text).font(.caption)
                    if progress.total > 0 {
                        ProgressView(value: Double(progress.completed), total: Double(progress.total))
                    } else { ProgressView() }
                }
                Button("Refresh") { Task { await refresh() } }.disabled(busy)
            }.padding(20)
        }
        .task { await refresh() }
    }

    @MainActor private func refresh() async {
        busy = true
        defer { busy = false }
        do { assessment = try await store.assess(); error = nil; store.refresh() }
        catch { self.error = "Could not check local models: \(error.localizedDescription)" }
    }
    @MainActor private func install() async {
        busy = true
        store.lastError = nil
        await store.ollamaInstall()
        let failure = store.lastError
        await refresh()
        if let failure { error = failure }
    }
    @MainActor private func download(_ model: String) async {
        busy = true
        store.lastError = nil
        await store.ollamaPull(model)
        let failure = store.lastError
        await refresh()
        if let failure { error = failure }
    }
}

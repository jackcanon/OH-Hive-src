import SwiftUI

private struct LocalBackendProfile: Codable {
    var kind = "legacy"
    var origin = ""
    var credentialFile = ""
    var thinkingBudget: Int?
    var defaultModel: String?
    var defaultCodingThink: Bool?
    var defaultCheckerThink: Bool?
    var workflowOwnerFile: String?
}

/// Both UI and MCP use the app-owned worker's validated profile operation.
struct LocalTextBackendSettings: View {
    @ObservedObject var worker: PrivateCodingWorkerModel
    @State private var kind = "legacy"
    @State private var origin = "http://127.0.0.1:18090"
    @State private var credentialFile = ""
    @State private var savedProfile = LocalBackendProfile()
    @State private var defaultModel = ""
    @State private var thinkingBudget = ""
    @State private var busy = false
    @State private var message = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Local inference engine", selection: $kind) {
                Text("Ollama / llama-server").tag("legacy")
                Text("TensorFold").tag("tensorfold")
            }
            if kind == "tensorfold" {
                TextField("TensorFold loopback origin", text: $origin)
                    .textFieldStyle(.roundedBorder)
                TextField("Private API key file (absolute path)", text: $credentialFile)
                    .textFieldStyle(.roundedBorder)
                TextField("Default served model", text: $defaultModel).textFieldStyle(.roundedBorder)
                TextField("Reasoning token budget (optional)", text: $thinkingBudget).textFieldStyle(.roundedBorder)
                Text("Start TensorFold separately with a supported checkpoint. Credentials stay on this Mac. Speed depends on the model and workload.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Button("Use this engine") {
                busy = true
                Task { @MainActor in
                    defer { busy = false }
                    do {
                        var profile = kind == "tensorfold" ? savedProfile : LocalBackendProfile()
                        profile.kind = kind
                        profile.origin = kind == "tensorfold" ? origin : ""
                        profile.credentialFile = kind == "tensorfold" ? credentialFile : ""
                        if kind == "tensorfold" {
                            profile.defaultModel = defaultModel.isEmpty ? nil : defaultModel
                            if !thinkingBudget.isEmpty, Int(thinkingBudget) == nil {
                                throw NSError(domain: "LokiDen", code: 1, userInfo: [NSLocalizedDescriptionKey: "Enter a whole-number reasoning budget."])
                            }
                            profile.thinkingBudget = Int(thinkingBudget)
                        }
                        let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
                        let data = try encoder.encode(profile)
                        _ = try await worker.selectBackend(String(decoding: data, as: UTF8.self))
                        savedProfile = profile
                        message = "Engine selected. Start the coding worker when ready."
                    } catch { message = error.localizedDescription }
                }
            }
            .disabled(worker.enabled || busy)
            if worker.enabled { Text("Stop the coding worker before switching engines.").font(.caption) }
            if !message.isEmpty { Text(message).font(.caption).accessibilityLabel(message) }
        }
        .task {
            do {
                let raw = try worker.backendProfile()
                let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
                let profile = try decoder.decode(LocalBackendProfile.self, from: Data(raw.utf8))
                savedProfile = profile; kind = profile.kind
                if !profile.origin.isEmpty { origin = profile.origin }
                credentialFile = profile.credentialFile
                defaultModel = profile.defaultModel ?? ""
                thinkingBudget = profile.thinkingBudget.map(String.init) ?? ""
            } catch { message = error.localizedDescription }
        }
    }
}

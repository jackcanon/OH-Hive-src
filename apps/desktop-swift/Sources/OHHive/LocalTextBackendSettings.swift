import SwiftUI

/// Both UI and MCP use the app-owned worker's validated profile operation.
struct LocalTextBackendSettings: View {
    @ObservedObject var worker: PrivateCodingWorkerModel
    @State private var kind = "legacy"
    @State private var origin = "http://127.0.0.1:18090"
    @State private var credentialFile = ""
    @State private var busy = false
    @State private var message = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Local inference engine", selection: $kind) {
                Text("Ollama / llama-server").tag("legacy")
                Text("TensorFold (optional)").tag("tensorfold")
            }
            if kind == "tensorfold" {
                TextField("TensorFold loopback origin", text: $origin)
                    .textFieldStyle(.roundedBorder)
                TextField("Private API key file (absolute path)", text: $credentialFile)
                    .textFieldStyle(.roundedBorder)
                Text("Start TensorFold separately with a supported checkpoint. Credentials stay on this Mac. Speed depends on the model and workload.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Button("Use this engine") {
                busy = true
                Task { @MainActor in
                    defer { busy = false }
                    do {
                        let data = try JSONSerialization.data(withJSONObject: ["kind": kind,
                            "origin": kind == "tensorfold" ? origin : "",
                            "credential_file": kind == "tensorfold" ? credentialFile : ""])
                        _ = try await worker.selectBackend(String(decoding: data, as: UTF8.self))
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
                if let profile = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: String] {
                    kind = profile["kind"] ?? "legacy"
                    if let value = profile["origin"], !value.isEmpty { origin = value }
                    credentialFile = profile["credential_file"] ?? ""
                }
            } catch { message = error.localizedDescription }
        }
    }
}

import SwiftUI
import OHHiveFFI

/// Two-stage provider-then-model picker for the Chat composer (2026-09-13, Jack: "We also need to
/// be able to pick what model we are chatting with like you can with CoWork and Codex and
/// Hermes... if we've connected multiple than we are having two phases, a provider picker, and
/// then the model is in a second picker"). Backed by the same `ByokKeysStatus` SettingsView
/// already reads -- only providers with a key on file are selectable here; adding a key is still
/// a Settings action, not something this picker does itself.
///
/// Stage 1 (provider) only lists configured providers. Stage 2 (model) offers that provider's
/// saved `preferred_model` as "Default", plus a free-text override -- there's no live per-provider
/// model list API yet, so this mirrors the same free-text convention `SettingsView`'s BYOK card
/// already established rather than inventing a second one.
struct ProviderModelPicker: View {
    let keysStatus: ByokKeysStatus?
    @Binding var provider: String?
    /// Empty string means "use this provider's saved preferred_model, or the function's default."
    @Binding var model: String

    @State private var customModelDraft = ""
    @State private var editingCustomModel = false

    private var configuredProviders: [String] {
        guard let keysStatus else { return [] }
        var result: [String] = []
        if keysStatus.anthropic != nil { result.append("anthropic") }
        if keysStatus.openai != nil { result.append("openai") }
        if keysStatus.nous != nil { result.append("nous") }
        return result
    }

    private func label(_ p: String) -> String {
        switch p {
        case "anthropic": return "Anthropic"
        case "openai": return "OpenAI"
        case "nous": return "Nous (Hermes)"
        default: return p
        }
    }

    private func savedModel(for p: String) -> String? {
        switch p {
        case "anthropic": return keysStatus?.anthropic?.preferredModel
        case "openai": return keysStatus?.openai?.preferredModel
        case "nous": return keysStatus?.nous?.preferredModel
        default: return nil
        }
    }

    var body: some View {
        HStack(spacing: 6) {
            // Stage 1: provider.
            Menu {
                ForEach(configuredProviders, id: \.self) { p in
                    Button {
                        provider = p
                        model = ""
                        editingCustomModel = false
                    } label: {
                        if provider == p {
                            Label(label(p), systemImage: "checkmark")
                        } else {
                            Text(label(p))
                        }
                    }
                }
            } label: {
                pill(text: provider.map(label) ?? "Provider", systemImage: "cloud")
            }
            .disabled(configuredProviders.isEmpty)

            // Stage 2: model, only once a provider is chosen.
            if let provider {
                Menu {
                    Button {
                        model = ""
                        editingCustomModel = false
                    } label: {
                        let defaultLabel = savedModel(for: provider).map { "Default (\($0))" } ?? "Default"
                        if model.isEmpty { Label(defaultLabel, systemImage: "checkmark") } else { Text(defaultLabel) }
                    }
                    Button {
                        customModelDraft = model.isEmpty ? (savedModel(for: provider) ?? "") : model
                        editingCustomModel = true
                    } label: {
                        Text("Custom\u{2026}")
                    }
                } label: {
                    pill(text: model.isEmpty ? "Model" : model, systemImage: "cpu")
                }
            }
        }
        .popover(isPresented: $editingCustomModel) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Model id").font(.caption).foregroundStyle(.secondary)
                TextField(
                    provider == "nous" ? "e.g. anthropic/claude-sonnet-4.6" : "e.g. claude-sonnet-4-5",
                    text: $customModelDraft
                )
                .textFieldStyle(.roundedBorder)
                .frame(width: 260)
                .onSubmit { model = customModelDraft; editingCustomModel = false }
                HStack {
                    Spacer()
                    Button("Use default") { model = ""; editingCustomModel = false }
                    Button("Set") { model = customModelDraft; editingCustomModel = false }
                        .buttonStyle(.borderedProminent)
                }
            }
            .padding(12)
        }
    }

    private func pill(text: String, systemImage: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: systemImage)
            Text(text)
            Image(systemName: "chevron.down").font(.caption2)
        }
        .font(.caption)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Color.secondary.opacity(0.12))
        .clipShape(Capsule())
    }
}

import SwiftUI

func providerDisplayName(_ provider: String) -> String {
    switch provider {
    case "anthropic": return "Anthropic"
    case "openai": return "OpenAI"
    case "nous": return "Nous (Hermes)"
    default: return provider
    }
}

func formatStorageGB(_ bytes: UInt64) -> String {
    let value = Double(bytes) / 1_073_741_824
    return String(format: value > 10 ? "%.0f GB" : "%.2f GB", value)
}

struct SettingsNote: View {
    let text: String
    var ok = false
    init(_ text: String, ok: Bool = false) { self.text = text; self.ok = ok }
    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ok ? Color.green.opacity(0.12) : Color.secondary.opacity(0.08))
            .clipShape(.rect(cornerRadius: 6))
    }
}

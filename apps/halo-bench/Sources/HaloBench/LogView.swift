import SwiftUI
import AppKit

struct LogView: View {
    var body: some View {
        VStack(spacing: 0) {
            RunTimelineView().padding(12)
            Divider()
            LogPane(title: "Current run log")
        }
        .navigationTitle("Logs")
    }
}

/// Monospaced, auto-scrolling log with copy/save. Shared by the Run tab and the Logs tab.
struct LogPane: View {
    @Environment(BenchStore.self) private var store
    var title: String

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(title).font(.headline)
                if store.phase == .running { ProgressView().controlSize(.small).padding(.leading, 4) }
                Spacer()
                Text("\(store.log.count) lines").font(.caption).foregroundStyle(.secondary)
                Button { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(store.log.joined(separator: "\n"), forType: .string) } label: { Image(systemName: "doc.on.doc") }
                    .buttonStyle(.borderless).help("Copy log")
                Button { save() } label: { Image(systemName: "square.and.arrow.down") }
                    .buttonStyle(.borderless).help("Save log…")
            }
            .padding(10)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(store.log.enumerated()), id: \.offset) { i, line in
                            Text(line)
                                .font(.system(size: 11.5, design: .monospaced))
                                .foregroundStyle(color(for: line))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                                .id(i)
                        }
                    }
                    .padding(10)
                }
                .onChange(of: store.log.count) { _, n in if n > 0 { proxy.scrollTo(n - 1, anchor: .bottom) } }
            }
            .background(Color(nsColor: .textBackgroundColor))
        }
    }

    private func color(for line: String) -> Color {
        if line.hasPrefix("!!") { return .red }
        if line.hasPrefix("==>") { return .honey }
        if line.hasPrefix("|") { return .mint }
        return .primary
    }

    private func save() {
        let p = NSSavePanel(); p.nameFieldStringValue = "halobench.log"
        if p.runModal() == .OK, let u = p.url { try? store.log.joined(separator: "\n").write(to: u, atomically: true, encoding: .utf8) }
    }
}

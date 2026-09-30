import SwiftUI
import OHHiveFFI
import AppKit

/// Imports a folder's `.md` files straight into one collection, scoped to it from the moment it
/// opens -- unlike "Directories and assets…" (`LibraryDirectoryBrowser`), which only ever builds
/// a local catalog of file paths on this Mac and never writes into any vault, no matter which
/// collection happens to be open when you scan. Jack, 2026-09-28, after finding that gap: "So how
/// do I import stuff into the collections? because that's what I'm really needing to accomplish
/// here." This is the "Scan a directory…" button `VaultView.noteBrowser` now opens instead.
///
/// Content is written verbatim via `vaultAddNote` -- the same call "New note" uses -- rather than
/// through `vaultIntakeApproveFile`: that call wraps every file in an "## Intake provenance"
/// block and refiles it under a synthesized `Intake/<project>/...` path, which is right for
/// Spark's own meeting importer but wrong for a member dropping their own already-organized notes
/// into a collection they want to keep readable as-is. `vaultIntakeListCandidates` is still used
/// for *discovery* (walking the folder, skipping hidden/oversized/non-.md files, guessing a
/// title) -- only the write path differs from what that FFI pairing was originally built for.
struct VaultFolderImportView: View {
    let vault: VaultInfo
    let store: HiveStore
    @Environment(\.dismiss) private var dismiss

    private struct Row: Identifiable {
        let candidate: IntakeCandidate
        var selected: Bool
        var outcome: Outcome = .pending
        var id: String { candidate.relativePath }
    }

    private enum Outcome: Equatable {
        case pending
        case imported
        case failed(String)
    }

    @State private var root: URL?
    @State private var rows: [Row] = []
    @State private var scanning = false
    @State private var importing = false
    @State private var message: String?

    private var selectedCount: Int { rows.filter { $0.selected }.count }
    private var allSelected: Bool { !rows.isEmpty && rows.allSatisfy { $0.selected } }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Scan a directory into \u{201C}\(vault.name)\u{201D}").font(.title2)
                Spacer()
                Button("Done") { dismiss() }
            }
            Text("Pick a folder; every Markdown (.md) file in it -- and its subfolders -- becomes a document in this collection, unchanged: no rewriting, no wrapper text. Importing again later updates matching documents in place instead of duplicating them.")
                .font(.callout)
                .foregroundStyle(.secondary)

            HStack {
                Button(root == nil ? "Choose a folder…" : "Choose a different folder…", action: chooseFolder)
                    .disabled(scanning || importing)
                if scanning { ProgressView().controlSize(.small) }
                if let root {
                    Text(root.path).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }

            if let message {
                Text(message).font(.caption).foregroundStyle(.secondary)
            }

            if !rows.isEmpty {
                HStack {
                    Toggle(isOn: Binding(
                        get: { allSelected },
                        set: { newValue in for i in rows.indices { rows[i].selected = newValue } }
                    )) {
                        Text("Select all")
                    }
                    .toggleStyle(.checkbox)
                    Spacer()
                    Text("\(selectedCount) of \(rows.count) selected")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Button("Import selected") { importSelected() }
                        .disabled(importing || selectedCount == 0)
                    if importing { ProgressView().controlSize(.small) }
                }

                List {
                    ForEach($rows) { $row in
                        HStack {
                            Toggle(isOn: $row.selected) { EmptyView() }
                                .toggleStyle(.checkbox)
                                .labelsHidden()
                                .disabled(importing)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(row.candidate.title).fontWeight(.medium)
                                Text(row.candidate.relativePath)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                            Spacer()
                            outcomeLabel(row.outcome)
                        }
                    }
                }
            }
        }
        .padding(20)
        .frame(minWidth: 640, idealWidth: 760, minHeight: 420, idealHeight: 520)
    }

    @ViewBuilder
    private func outcomeLabel(_ outcome: Outcome) -> some View {
        switch outcome {
        case .pending:
            EmptyView()
        case .imported:
            Label("Imported", systemImage: "checkmark.circle.fill")
                .labelStyle(.iconOnly)
                .font(.caption)
                .foregroundStyle(.green)
        case .failed(let reason):
            Label(reason, systemImage: "exclamationmark.triangle.fill")
                .font(.caption)
                .foregroundStyle(.orange)
        }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose the folder whose Markdown files should become documents in \u{201C}\(vault.name)\u{201D}."
        guard panel.runModal() == .OK, let chosen = panel.url else { return }
        root = chosen
        scanFolder(chosen)
    }

    private func scanFolder(_ root: URL) {
        scanning = true
        message = nil
        rows = []
        Task.detached(priority: .utility) {
            do {
                let candidates = try store.vaultIntakeListCandidates(root: root.path)
                await MainActor.run {
                    rows = candidates.map { Row(candidate: $0, selected: true) }
                    scanning = false
                    if rows.isEmpty {
                        message = "No Markdown (.md) files found in that folder."
                    }
                }
            } catch {
                await MainActor.run {
                    scanning = false
                    message = "Couldn't scan that folder: \(error.localizedDescription)"
                }
            }
        }
    }

    private func importSelected() {
        guard let root else { return }
        let targets: [(index: Int, candidate: IntakeCandidate)] = rows.indices.compactMap { i in
            rows[i].selected ? (i, rows[i].candidate) : nil
        }
        guard !targets.isEmpty else { return }
        importing = true
        Task.detached(priority: .utility) {
            for target in targets {
                let fileURL = root.appendingPathComponent(target.candidate.relativePath)
                let outcome: Outcome
                do {
                    let content = try String(contentsOf: fileURL, encoding: .utf8)
                    _ = try store.vaultAddNote(
                        vaultId: vault.id,
                        documentId: nil,
                        path: target.candidate.relativePath,
                        title: target.candidate.title,
                        content: content
                    )
                    outcome = .imported
                } catch {
                    outcome = .failed(error.localizedDescription)
                }
                await MainActor.run { rows[target.index].outcome = outcome }
            }
            await MainActor.run {
                importing = false
                let failed = rows.filter { if case .failed = $0.outcome { return true }; return false }.count
                let imported = rows.filter { $0.outcome == .imported }.count
                message = failed == 0
                    ? "Imported \(imported) document\(imported == 1 ? "" : "s") into \u{201C}\(vault.name)\u{201D}."
                    : "Imported \(imported), \(failed) failed -- see the list above for why."
            }
        }
    }
}

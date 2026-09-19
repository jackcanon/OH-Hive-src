import SwiftUI
import OHHiveFFI
import AppKit

/// "Vault" (2026-09-14, ADR-028) -- a second-brain knowledge library scoped to this member's own
/// Private Fleet, not the whole community Hive (Jack: "I want to make sure that we are
/// integrating a second brain like system into Hive so that our agents are able to search data
/// libraries that we curate during production. Something similar to an obsidian vault, but it's
/// built into Hive"). Sits alongside "Projects"/"Activity" under the sidebar's Private Fleet
/// section, per Jack's follow-up correction that this is user-level and Private-Fleet-wide, not
/// tied to any one paired machine.
///
/// First-pass scope (see `crates/ohhive-ffi/src/local_hub.rs`'s header for the full story): this
/// machine only. Notes are added by hand -- a title, a `.md`-suffixed label, and the text --
/// there is no folder-watching yet, and no way to read a vault from a *second* Private Fleet
/// machine yet. Both are real, designed, deliberately-deferred next increments, not gaps nobody
/// noticed.
struct VaultView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var status: VaultHostStatus?
    @State private var selectedVaultId: String?
    @State private var newVaultName = ""
    @State private var creatingVault = false
    @State private var query = ""
    @State private var hits: [VaultHit] = []
    @State private var searching = false
    @State private var editor: NoteEditor?
    @State private var error: String?
    @State private var maintenance: VaultMaintenanceStatus?
    @State private var showDirectoryCatalog = false

    private var selectedVault: VaultInfo? {
        status?.vaults.first { $0.id == selectedVaultId }
    }

    var body: some View {
        HStack(spacing: 0) {
            vaultList
                .frame(width: 220)
            Divider()
            if let vault = selectedVault {
                noteBrowser(vault)
            } else {
                emptyDetail
            }
        }
        .navigationTitle("Library")
        .onAppear { open() }
        .sheet(isPresented: $showDirectoryCatalog) {
            LibraryDirectoryBrowser()
        }
        .sheet(item: $editor) { draft in
            NoteEditorSheet(draft: draft, onSave: saveNote, onCancel: { editor = nil })
        }

    }

    // MARK: - Left column: vaults

    private var vaultList: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("A knowledge library for your Private Fleet's agents to search -- private to you, separate from the community Hive.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 10)
                .padding(.top, 10)

            if let error {
                Text(error).font(.caption).foregroundStyle(.secondary)
                    .padding(.horizontal, 10)
            }

            Button("Directories and assets…") { showDirectoryCatalog = true }
                .padding(.horizontal, 10)

            List(selection: $selectedVaultId) {
                ForEach(status?.vaults ?? [], id: \.id) { v in
                    Label(v.name, systemImage: v.state == "ready" ? "book.closed" : "exclamationmark.triangle")
                        .tag(v.id)
                }
            }
            .listStyle(.sidebar)

            HStack {
                TextField("New collection name…", text: $newVaultName)
                    .textFieldStyle(.roundedBorder)
                    .disabled(creatingVault)
                    .onSubmit(createVault)
                Button("Add") { createVault() }
                    .disabled(creatingVault || newVaultName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(10)
        }
    }

    private var emptyDetail: some View {
        VStack(spacing: 8) {
            Image(systemName: "books.vertical").font(.system(size: 36)).foregroundStyle(.secondary)
            Text(status == nil ? "Opening your Library…" : "Create a collection on the left, or pick one, to get started.")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Right column: one vault's notes

    private func noteBrowser(_ vault: VaultInfo) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(vault.name).font(.headline)
                Spacer()
                Button {
                    showDirectoryCatalog = true
                } label: {
                    Label("Scan a directory…", systemImage: "folder.badge.plus")
                }
                Button {
                    editor = NoteEditor(vaultId: vault.id, documentId: nil, path: "", title: "", content: "")
                } label: {
                    Label("New note", systemImage: "square.and.pencil")
                }
            }

            maintenanceRow(vault)
                .task(id: vault.id) { loadMaintenance(vault.id) }

            HStack {
                TextField("Search this collection…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await search(vault.id) } }
                Button("Search") { Task { await search(vault.id) } }
                    .disabled(query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if searching { ProgressView().controlSize(.small) }
            }

            if hits.isEmpty {
                Text(query.isEmpty
                     ? "Search is the browse view for now -- try a word from a note you've added."
                     : "No matches.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(hits, id: \.id) { hit in
                        Button {
                            Task { await openNote(vault.id, hit) }
                        } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                HStack {
                                    Text(hit.title).font(.callout).bold()
                                    Spacer()
                                    Text(hit.path).font(.caption2).foregroundStyle(.secondary)
                                }
                                Text(hit.snippet).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                            }
                            .padding(8)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(Color.secondary.opacity(0.06))
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding()
    }

    // MARK: - Actions

    private func open() {
        guard status == nil else { return }
        if let s = store.vaultOpen() {
            status = s
            error = nil
        } else {
            error = "Couldn't open your Library -- check Settings > General for the last error."
        }
    }

    private func createVault() {
        let name = newVaultName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        creatingVault = true
        defer { creatingVault = false }
        do {
            let v = try store.vaultCreate(name: name)
            status?.vaults.append(v)
            selectedVaultId = v.id
            newVaultName = ""
            error = nil
        } catch {
            self.error = "Couldn't create that collection: \(error.localizedDescription)"
        }
    }

    private func search(_ vaultId: String) async {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { hits = []; return }
        searching = true
        defer { searching = false }
        do {
            hits = try store.vaultSearch(vaultId: vaultId, query: q)
            error = nil
        } catch {
            self.error = "Search failed: \(error.localizedDescription)"
        }
    }

    /// Minimal status surface for `vault_maintenance.rs`'s host-owned staleness/duplicate scan
    /// (2026-09-15) -- the toggle is the only control here; interval/retention/quota tuning
    /// stays at their sane defaults (`VaultMaintenancePolicy`'s Rust-side `Default`) rather than
    /// a settings form nobody asked for yet. The host loop that actually runs ticks is already
    /// running (started once from `HiveStore.vaultOpen()`); this only changes whether this one
    /// vault's policy is `enabled`.
    private func maintenanceRow(_ vault: VaultInfo) -> some View {
        HStack(spacing: 8) {
            Toggle(isOn: Binding(
                get: { maintenance?.policy.enabled ?? false },
                set: { toggleMaintenance(vault, enabled: $0) }
            )) {
                Text("Auto-maintenance").font(.caption)
            }
            .toggleStyle(.switch)
            .controlSize(.small)

            Text(maintenanceSummary)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var maintenanceSummary: String {
        guard let maintenance else { return "" }
        guard maintenance.policy.enabled else { return "Off -- scans for stale/duplicate notes and trims old snapshots on a schedule." }
        guard let last = maintenance.lastResult else { return "Enabled -- first scan pending." }
        let when = Date(timeIntervalSince1970: Double(last.finishedMs) / 1000)
        let formatted = when.formatted(.relative(presentation: .named))
        if last.outcome == "scanned" {
            return "Last scan \(formatted): \(last.stale) stale, \(last.duplicates) duplicate."
        }
        return "Last scan \(formatted): \(last.outcome)."
    }

    private func loadMaintenance(_ vaultId: String) {
        maintenance = store.vaultMaintenanceStatus(vaultId: vaultId)
    }

    private func toggleMaintenance(_ vault: VaultInfo, enabled: Bool) {
        let policy = VaultMaintenancePolicy(
            enabled: enabled,
            intervalSeconds: maintenance?.policy.intervalSeconds ?? 86_400,
            staleAfterDays: maintenance?.policy.staleAfterDays ?? 90,
            redundantSnapshotDays: maintenance?.policy.redundantSnapshotDays,
            archiveQuotaBytes: maintenance?.policy.archiveQuotaBytes ?? 64 * 1024 * 1024
        )
        do {
            try store.vaultConfigureMaintenance(vaultId: vault.id, policy: policy)
            loadMaintenance(vault.id)
        } catch {
            self.error = String(describing: error)
        }
    }

    private func openNote(_ vaultId: String, _ hit: VaultHit) async {
        do {
            let doc = try store.vaultRead(vaultId: vaultId, documentId: hit.id, revision: hit.revision)
            editor = NoteEditor(vaultId: vaultId, documentId: doc.id, path: doc.path, title: doc.title, content: doc.content)
        } catch {
            self.error = "Couldn't open that note -- it may have changed since this search. Try searching again."
        }
    }

    private func saveNote(_ draft: NoteEditor) {
        do {
            _ = try store.vaultAddNote(
                vaultId: draft.vaultId,
                documentId: draft.documentId,
                path: draft.path,
                title: draft.title,
                content: draft.content
            )
            editor = nil
            if !query.isEmpty { Task { await search(draft.vaultId) } }
        } catch {
            self.error = "Couldn't save that note: \(error.localizedDescription)"
        }
    }


}

/// A note being created or edited. `documentId == nil` means "new note"; identity (§`local_hub.rs`)
/// is the id, never the path, so renaming `path` and saving still edits the same note.
private struct NoteEditor: Identifiable {
    var vaultId: String
    var documentId: String?
    var path: String
    var title: String
    var content: String
    var id: String { documentId ?? "new" }
}

/// Turns whatever the member typed (or left blank) into a valid vault path: relative,
/// slash-separated, `.md`-suffixed (the one requirement the storage layer actually enforces --
/// see `local_hub.rs`'s `vault_add_note` doc). Blank falls back to a slug of the title so "New
/// note" never requires the member to think about filenames at all.
private func normalizedNotePath(title: String, path: String) -> String {
    var p = path.trimmingCharacters(in: .whitespacesAndNewlines)
    if p.isEmpty {
        let slug = title
            .lowercased()
            .map { $0.isLetter || $0.isNumber ? $0 : "-" }
            .reduce(into: "") { result, c in
                if c != "-" || result.last != "-" { result.append(c) }
            }
            .trimmingCharacters(in: CharacterSet(charactersIn: "-"))
        p = slug.isEmpty ? "note" : slug
    }
    if !p.hasSuffix(".md") { p += ".md" }
    return p
}

private struct NoteEditorSheet: View {
    @State var draft: NoteEditor
    let onSave: (NoteEditor) -> Void
    let onCancel: () -> Void

    private var resolvedPath: String { normalizedNotePath(title: draft.title, path: draft.path) }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(draft.documentId == nil ? "New note" : "Edit note").font(.headline)
            TextField("Title", text: $draft.title).textFieldStyle(.roundedBorder)
            VStack(alignment: .leading, spacing: 3) {
                TextField("Path (optional -- e.g. recipes/lasagna)", text: $draft.path)
                    .textFieldStyle(.roundedBorder)
                Text("Saved as \(resolvedPath) -- .md is added automatically, no need to type it.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            TextEditor(text: $draft.content)
                .font(.system(.body, design: .monospaced))
                .frame(minWidth: 420, minHeight: 260)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.3)))
            HStack {
                Spacer()
                Button("Cancel", action: onCancel)
                Button("Save") {
                    var saved = draft
                    saved.path = resolvedPath
                    onSave(saved)
                }
                    .keyboardShortcut(.defaultAction)
                    .disabled(draft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
    }
}

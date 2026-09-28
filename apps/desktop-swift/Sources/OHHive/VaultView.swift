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
    @State private var browsing = false
    @State private var editor: NoteEditor?
    @State private var error: String?
    @State private var maintenance: VaultMaintenanceStatus?
    @State private var showDirectoryCatalog = false
    @State private var sharingCollection: VaultInfo?
    @State private var renamingVault: VaultInfo?
    @State private var renameText = ""
    @State private var pendingDeleteVault: VaultInfo?

    private var selectedVault: VaultInfo? {
        status?.vaults.first { $0.id == selectedVaultId }
    }

    /// Formats `VaultHit.documentDate` (Unix seconds, best-effort -- see
    /// `hive_core::local_hub::vault::extract_document_date`) for the search/browse row, so a
    /// member can confirm they've got the right meeting/note without opening it (Jack, 2026-09-28).
    private static let documentDateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .medium
        f.timeStyle = .short
        return f
    }()

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
        .sheet(isPresented: Binding(get: { sharingCollection != nil }, set: { if !$0 { sharingCollection = nil } })) {
            if let collection = sharingCollection {
                LibrarySharingView(collection: collection, store: store)
            }
        }
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
                        .contextMenu {
                            Button("Rename\u{2026}") {
                                renameText = v.name
                                renamingVault = v
                            }
                            Button("Delete", role: .destructive) {
                                pendingDeleteVault = v
                            }
                        }
                }
            }
            .listStyle(.sidebar)
            .alert(
                "Rename collection",
                isPresented: Binding(get: { renamingVault != nil }, set: { if !$0 { renamingVault = nil } })
            ) {
                TextField("Name", text: $renameText)
                Button("Rename") { if let v = renamingVault { renameVault(v) } }
                Button("Cancel", role: .cancel) { renamingVault = nil }
            } message: {
                Text(renamingVault.map { "Renaming \"\($0.name)\"." } ?? "")
            }
            .alert(
                "Delete this collection?",
                isPresented: Binding(get: { pendingDeleteVault != nil }, set: { if !$0 { pendingDeleteVault = nil } })
            ) {
                Button("Delete", role: .destructive) { if let v = pendingDeleteVault { deleteVault(v) } }
                Button("Cancel", role: .cancel) { pendingDeleteVault = nil }
            } message: {
                Text(pendingDeleteVault.map { "\"\($0.name)\" and everything in it will be permanently removed. This cannot be undone." } ?? "")
            }

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
                Button("Computer access…", systemImage: "desktopcomputer") { sharingCollection = vault }
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
                .task(id: vault.id) { await browse(vault.id) }

            HStack {
                TextField("Search this collection…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await search(vault.id) } }
                Button("Search") { Task { await search(vault.id) } }
                    .disabled(query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if searching { ProgressView().controlSize(.small) }
            }

            if browsing {
                ProgressView().controlSize(.small)
            } else if hits.isEmpty {
                Text(query.isEmpty
                     ? "This collection is empty -- add a note or scan a directory to get started."
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
                                    if let date = hit.documentDate {
                                        Text(VaultView.documentDateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(date))))
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
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
            // First-ever open, nothing here yet: seed one example so a new member sees what
            // belongs in this Library instead of an empty screen (Jack, 2026-09-27: "there
            // should be a default .md in there that shows the equivalent of a soul.md etc.").
            if s.vaults.isEmpty { seedStarterVault() }
        } else {
            error = "Couldn't open your Library -- check Settings > General for the last error."
        }
    }

    private func seedStarterVault() {
        do {
            let v = try store.vaultCreate(name: "Best Practices")
            _ = try store.vaultAddNote(
                vaultId: v.id,
                documentId: nil,
                path: "welcome/what-goes-here.md",
                title: "What goes in this Library",
                content: Self.starterDocContent
            )
            status?.vaults.append(v)
            selectedVaultId = v.id
        } catch {
            // An empty Library is still usable without the example note -- not worth surfacing
            // as an error on first launch.
        }
    }

    private static let starterDocContent = """
    # What goes in this Library

    This is an example of the kind of note your agents can search here -- the same shape as a \
    `soul.md`, `CLAUDE.md`, or `AGENTS.md` file: instructions an agent reads before it starts \
    working, not a transcript of what it did.

    ## Identity
    Who this agent is, and the one or two things it's responsible for. Specific enough that a \
    stranger reading it would know what to expect from this agent and what NOT to ask it to do.

    ## House rules
    Standing constraints that don't change task to task -- how you want commits written, what \
    it should never touch without asking, where its output belongs.

    ## Best practices worth remembering
    Nuggets discovered the hard way, so nobody re-learns them: a build quirk, a gotcha in a \
    dependency, a decision and why it was made. This is the running list every agent can search \
    before it hits the same wall someone already hit.

    ---

    Delete this note once you've added your own -- or use "Scan a directory…" above to pull in \
    `soul.md` / `CLAUDE.md` / `AGENTS.md` files you already have scattered across this machine.
    """

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

    private func renameVault(_ vault: VaultInfo) {
        let name = renameText.trimmingCharacters(in: .whitespacesAndNewlines)
        renamingVault = nil
        guard !name.isEmpty, name != vault.name else { return }
        do {
            try store.vaultRename(vaultId: vault.id, name: name)
            if let idx = status?.vaults.firstIndex(where: { $0.id == vault.id }) {
                status?.vaults[idx].name = name
            }
            error = nil
        } catch {
            self.error = "Couldn't rename that collection: \(error.localizedDescription)"
        }
    }

    private func deleteVault(_ vault: VaultInfo) {
        pendingDeleteVault = nil
        do {
            try store.vaultDelete(vaultId: vault.id)
            status?.vaults.removeAll { $0.id == vault.id }
            if selectedVaultId == vault.id { selectedVaultId = nil }
            error = nil
        } catch {
            self.error = "Couldn't delete that collection: \(error.localizedDescription)"
        }
    }

    private func search(_ vaultId: String) async {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { await browse(vaultId); return }
        searching = true
        defer { searching = false }
        do {
            hits = try store.vaultSearch(vaultId: vaultId, query: q)
            error = nil
        } catch {
            self.error = "Search failed: \(error.localizedDescription)"
        }
    }

    /// Default view of a vault: every document, no search term required -- the Finder-window-
    /// style browse Jack asked for. Runs whenever a vault is selected and whenever the search
    /// box is cleared back out.
    private func browse(_ vaultId: String) async {
        guard query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        browsing = true
        defer { browsing = false }
        do {
            hits = try store.vaultListDocuments(vaultId: vaultId)
            error = nil
        } catch {
            self.error = "Couldn't list this collection: \(error.localizedDescription)"
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

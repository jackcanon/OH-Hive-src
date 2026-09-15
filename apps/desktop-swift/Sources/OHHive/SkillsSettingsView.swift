import SwiftUI
import OHHiveFFI
import AppKit

/// Settings surface for ADR-027's Skills library (decision 5: "a member should be able to see
/// what's in their own library ... with delete" -- "the load-bearing mitigation" for skills being
/// written fully automatically, no approval step). Skills live at `<workspace>/.hive/skills/`
/// (`hive_core::skills::SkillStore`, wired into `coder.rs`) -- workspace-local by design, so
/// unlike the Vault there's no single library on this machine to open; the member points this at
/// a project folder to inspect what an agent has saved there.
struct SkillsSettingsView: View {
    @EnvironmentObject private var store: HiveStore
    @AppStorage("skillsSettingsWorkspacePath") private var workspacePath = ""
    @State private var inventory: SkillInventory?
    @State private var detail: SkillDocument?
    @State private var loading = false
    @State private var error: String?
    @State private var pendingDelete: SkillSummary?

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Procedures your agents have saved to themselves while working a card here -- fully automatic, no approval step, so this is where you catch a bad one.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                HStack {
                    TextField("Workspace folder…", text: $workspacePath)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit(refresh)
                    Button("Choose…", action: pickFolder)
                    Button {
                        refresh()
                    } label: {
                        if loading {
                            ProgressView().controlSize(.small)
                        } else {
                            Image(systemName: "arrow.clockwise")
                        }
                    }
                    .disabled(workspacePath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                if let error {
                    Text(error).font(.caption).foregroundStyle(.secondary)
                }

                if let inventory {
                    if inventory.skills.isEmpty && inventory.issues.isEmpty {
                        Text("No skills saved here yet.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    List {
                        if !inventory.skills.isEmpty {
                            Section("Skills") {
                                ForEach(inventory.skills, id: \.id) { s in
                                    skillRow(s)
                                }
                            }
                        }
                        if !inventory.issues.isEmpty {
                            Section("Couldn't load") {
                                ForEach(inventory.issues, id: \.id) { issue in
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(issue.id).font(.body.weight(.medium))
                                        Text(issue.error).font(.caption).foregroundStyle(.secondary)
                                    }
                                }
                            }
                        }
                    }
                    .listStyle(.inset)
                } else {
                    Spacer()
                    Text(workspacePath.isEmpty ? "Choose a project folder to see its saved skills." : "Not loaded yet -- press refresh.")
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                }
            }
            .padding(16)
            .frame(minWidth: 320)

            Divider()

            detailPane
                .frame(minWidth: 280)
        }
        .navigationTitle("Skills")
        .onAppear { if !workspacePath.isEmpty { refresh() } }
        .alert(
            "Delete this skill?",
            isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
        ) {
            Button("Delete", role: .destructive) { if let s = pendingDelete { delete(s) } }
            Button("Cancel", role: .cancel) { pendingDelete = nil }
        } message: {
            Text(pendingDelete.map { "\"\($0.name)\" will be permanently removed from \(workspacePath)." } ?? "")
        }
    }

    // MARK: - Rows

    private func skillRow(_ s: SkillSummary) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(s.name).font(.body.weight(.medium))
                Text(s.description).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                if let ms = s.lastUsedUnixMs {
                    Text("Last used \(Self.relativeDate(ms))").font(.caption2).foregroundStyle(.tertiary)
                } else {
                    Text("Never used").font(.caption2).foregroundStyle(.tertiary)
                }
            }
            Spacer()
            Button {
                pendingDelete = s
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
        }
        .contentShape(Rectangle())
        .onTapGesture { loadDetail(s.id) }
    }

    @ViewBuilder
    private var detailPane: some View {
        if let detail {
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    Text(detail.summary.name).font(.title3.weight(.semibold))
                    Text(detail.summary.description).font(.callout).foregroundStyle(.secondary)
                    Divider()
                    Text(detail.procedure)
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                }
                .padding(16)
            }
        } else {
            VStack {
                Spacer()
                Text("Select a skill to read its saved procedure.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
            }
        }
    }

    // MARK: - Actions

    private func pickFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose the project folder whose saved skills you want to review."
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        workspacePath = path
        refresh()
    }

    private func refresh() {
        let path = workspacePath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !path.isEmpty else { return }
        loading = true
        error = nil
        detail = nil
        do {
            inventory = try store.skillsList(workspacePath: path)
        } catch {
            self.error = "Couldn't read that folder: \(error.localizedDescription)"
            inventory = nil
        }
        loading = false
    }

    private func loadDetail(_ id: String) {
        let path = workspacePath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !path.isEmpty else { return }
        do {
            detail = try store.skillsRead(workspacePath: path, id: id)
        } catch {
            self.error = "Couldn't read that skill: \(error.localizedDescription)"
        }
    }

    private func delete(_ s: SkillSummary) {
        pendingDelete = nil
        let path = workspacePath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !path.isEmpty else { return }
        do {
            try store.skillsDelete(workspacePath: path, id: s.id, revision: s.revision)
            if detail?.summary.id == s.id { detail = nil }
            refresh()
        } catch {
            self.error = "Couldn't delete \"\(s.name)\": \(error.localizedDescription)"
        }
    }

    private static func relativeDate(_ unixMs: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(unixMs) / 1000)
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}

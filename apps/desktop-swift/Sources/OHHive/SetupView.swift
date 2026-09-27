import SwiftUI
import OHHiveFFI
import ServiceManagement
import AppKit

/// Mirrors `apps/desktop/src/Setup.tsx`'s 4-stage first-run flow: Ollama -> Model -> Pair -> Go.
/// Shown as `ContentView`'s default selection until `store.snapshot?.setupDone`, matching the
/// Tauri app's `cur ?? (setup_done ? "Node" : "Setup")` logic.
struct SetupView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var assessment: SetupAssessment?
    @State private var error: String?
    @State private var busy = false
    @State private var choice: String?
    @State private var showPairSheet = false
    @State private var signInLater = false
    @State private var agentFileCandidates: [IntakeCandidate]?
    @State private var agentFileSelection: Set<String> = []
    @State private var agentFileScanning = false
    @State private var agentFileImporting = false
    @State private var agentFileMessage: String?
    @State private var agentFileScannedRoot: String?

    private var hasModel: Bool {
        guard let a = assessment else { return false }
        if let choice {
            return a.present.contains(where: { $0 == choice || $0.hasPrefix(choice + ":") })
        }
        return !a.present.isEmpty
    }

    private var stage: Int {
        guard let a = assessment else { return 0 }
        if !a.ollama.running { return 1 }
        if !hasModel { return 2 }
        if store.snapshot?.paired != true && store.snapshot?.privateFleetEnrolled != true { return 3 }
        return 4
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if store.snapshot?.privateFleetEnrolled != true && !signInLater {
                    PrivateFleetEnrollmentView()
                    Button("Set up sign-in later") { signInLater = true }
                        .font(.caption)
                    Text("Private coding projects need registration. You can finish it later in Settings.").font(.caption).foregroundStyle(.secondary)
                } else if let a = assessment {
                    hardwareCard(a)
                    if let error {
                        Text(error).foregroundStyle(.red).font(.callout)
                    }
                    stageCard(n: 1, title: "Ollama", active: stage == 1, done: a.ollama.running) {
                        ollamaStage(a)
                    }
                    stageCard(n: 2, title: "Model", active: stage == 2, done: stage > 2) {
                        modelStage(a)
                    }
                    if let p = store.setupProgress, !p.done {
                        progressCard(p)
                    }
                    stageCard(n: 3, title: "Account", active: stage == 3, done: store.snapshot?.paired == true || store.snapshot?.privateFleetEnrolled == true) {
                        pairStage(a)
                    }
                    agentInstructionsCard()

                    stageCard(n: 4, title: "Go", active: stage == 4, done: false) {
                        goStage(a)
                    }
                } else if let error {
                    Text(error).foregroundStyle(.red)
                } else {
                    ProgressView("Looking at this machine\u{2026}")
                }
            }
            .padding(20)
        }
        .navigationTitle("Setup")
        .task { await runAssess() }
        .sheet(isPresented: $showPairSheet) {
            PairView(isPresented: $showPairSheet)
                .environmentObject(store)
        }
    }

    // MARK: - stages

    @ViewBuilder private func hardwareCard(_ a: SetupAssessment) -> some View {
        GroupBox("This machine") {
            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(a.hardware.cpuModel).fontWeight(.semibold)
                    Text("\(a.hardware.cpuCores) cores \u{00b7} \(formatStorageGB(a.hardware.ramBytes)) memory")
                        .font(.caption).foregroundStyle(.secondary)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(a.hardware.gpuModel ?? "no GPU").fontWeight(.semibold)
                    Text("\(formatStorageGB(a.budgetBytes)) usable for models \u{00b7} \(formatStorageGB(a.hardware.diskFreeBytes)) free disk")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder private func ollamaStage(_ a: SetupAssessment) -> some View {
        if a.ollama.running {
            Text("Running, version \(a.ollama.version ?? "unknown").")
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 8) {
                Text(a.ollama.installedApp != nil
                     ? "Ollama is installed (\(a.ollama.installedApp!)) but not running."
                     : "Ollama runs the models. I can install it for you \u{2014} nothing to type.")
                    .foregroundStyle(.secondary)
                HStack {
                    Button(a.ollama.installedApp != nil ? "Start Ollama" : "Install Ollama") {
                        Task { await installOllama() }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy)
                    Link("or download it yourself", destination: URL(string: "https://ollama.com/download")!)
                        .font(.caption)
                }
            }
        }
    }

    @ViewBuilder private func modelStage(_ a: SetupAssessment) -> some View {
        if a.fits.isEmpty {
            Text("Not enough memory for any of the Hive's models. This machine can still help as a regional server (disk, not compute).")
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 6) {
                Text("Most capable model that fits \(formatStorageGB(a.budgetBytes)):")
                    .foregroundStyle(.secondary)
                ForEach(a.fits, id: \.model) { r in
                    let present = a.present.contains(where: { $0 == r.model || $0.hasPrefix(r.model + ":") })
                    HStack(alignment: .top) {
                        RadioDot(selected: choice == r.model)
                            .onTapGesture { choice = r.model }
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text(r.model).fontWeight(.semibold)
                                if present {
                                    Text("\u{00b7} already here").font(.caption).foregroundStyle(.green)
                                }
                            }
                            Text("\(r.why) \u{00b7} \(formatStorageGB(r.downloadBytes)) download")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .contentShape(Rectangle())
                    .onTapGesture { choice = r.model }
                }
                if stage == 2, let choice {
                    Button("Download \(choice)") {
                        Task { await pullModel(choice) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy || !a.ollama.running)
                } else if stage > 2, let choice,
                          !a.present.contains(where: { $0 == choice || $0.hasPrefix(choice + ":") }) {
                    Button("Switch to \(choice)") {
                        Task { await pullModel(choice) }
                    }
                    .disabled(busy)
                }
            }
        }
    }

    @ViewBuilder private func progressCard(_ p: SetupProgress) -> some View {
        let pct: Int? = p.total > 0 ? Int((Double(p.completed) / Double(p.total)) * 100) : nil
        GroupBox {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text(p.text).font(.callout)
                    Spacer()
                    if let pct { Text("\(pct)%").font(.caption).foregroundStyle(.secondary) }
                }
                ProgressView(value: pct.map { Double($0) } ?? 5, total: 100)
                if p.total > 0 {
                    Text("\(formatStorageGB(p.completed)) of \(formatStorageGB(p.total))")
                        .font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder private func pairStage(_ a: SetupAssessment) -> some View {
        if store.snapshot?.privateFleetEnrolled == true {
            Text("This Mac is registered.").foregroundStyle(.secondary)
        } else {
            Button("Register this Mac") { signInLater = false }
        }
        DisclosureGroup("Optional: join an invited community Hive") {
            Text("Community membership is separate from your private computer setup.").font(.caption)
            Button("Get a community pairing code") { showPairSheet = true }.disabled(busy)
        }
    }

    @ViewBuilder private func goStage(_ a: SetupAssessment) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Start working now, open at login so this machine stays in the Hive" + (a.suggestServer ? ", and consider the Server section \u{2014} this machine has the disk for it." : "."))
                .foregroundStyle(.secondary)
            HStack {
                Button("Start working") {
                    if store.snapshot?.paired == true { Task { await goStart() } }
                    else { store.setupFinish() }
                }
                .buttonStyle(.borderedProminent)
                .disabled(stage != 4 || busy)
                Button("Skip for now") {
                    store.setupFinish()
                }
                .disabled(busy)
            }
        }
    }

    /// Onboarding prompt (Jack, 2026-09-27): most members already have soul.md/CLAUDE.md/
    /// AGENTS.md files scattered around this Mac from other tools. Offer to find and bring them
    /// into the Library right here, rather than leaving that to be discovered by accident later.
    /// Optional -- doesn't gate the numbered stage flow or "Start working".
    @ViewBuilder private func agentInstructionsCard() -> some View {
        GroupBox("Agent instructions on this Mac") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Find soul.md, CLAUDE.md, or AGENTS.md files already on this machine and bring them into your Library so agents can search them.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let agentFileMessage {
                    Text(agentFileMessage).font(.caption).foregroundStyle(.secondary)
                }

                if agentFileScanning {
                    ProgressView("Scanning\u{2026}").controlSize(.small)
                } else if let candidates = agentFileCandidates {
                    if candidates.isEmpty {
                        Text("No soul.md / CLAUDE.md / AGENTS.md files found there.")
                            .font(.caption).foregroundStyle(.secondary)
                        Button("Try a different folder\u{2026}") { pickAgentFileFolder() }
                    } else {
                        ForEach(candidates, id: \.relativePath) { c in
                            Toggle(isOn: Binding(
                                get: { agentFileSelection.contains(c.relativePath) },
                                set: { on in
                                    if on { agentFileSelection.insert(c.relativePath) }
                                    else { agentFileSelection.remove(c.relativePath) }
                                }
                            )) {
                                Text(c.relativePath).font(.caption)
                            }
                            .toggleStyle(.checkbox)
                        }
                        HStack {
                            Button("Import \(agentFileSelection.count) to Library") { importAgentFiles() }
                                .disabled(agentFileSelection.isEmpty || agentFileImporting)
                            if agentFileImporting { ProgressView().controlSize(.small) }
                            Button("Scan a different folder\u{2026}") { pickAgentFileFolder() }
                                .disabled(agentFileImporting)
                        }
                    }
                } else {
                    Button("Scan for agent instructions\u{2026}") { pickAgentFileFolder() }
                }
            }
        }
    }

    // MARK: - actions

    private func runAssess() async {
        do {
            let a = try await store.assess()
            assessment = a
            if choice == nil { choice = a.recommended?.model }
            error = nil
        } catch {
            self.error = String(describing: error)
        }
    }

    private func installOllama() async {
        busy = true
        error = nil
        await store.ollamaInstall()
        busy = false
        await runAssess()
    }

    private func pullModel(_ model: String) async {
        busy = true
        error = nil
        await store.ollamaPull(model)
        busy = false
        await runAssess()
    }

    private func goStart() async {
        busy = true
        await store.startWorking()
        do { try SMAppService.mainApp.register() } catch { /* dev builds / already registered */ }
        store.setupFinish()
        busy = false
    }

    private func pickAgentFileFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.message = "Choose a folder to search for soul.md, CLAUDE.md, or AGENTS.md files."
        guard panel.runModal() == .OK, let root = panel.url else { return }
        agentFileScannedRoot = root.path
        agentFileScanning = true
        agentFileMessage = nil
        agentFileCandidates = nil
        Task {
            defer { agentFileScanning = false }
            do {
                let found = try store.vaultAgentInstructionCandidates(root: root.path)
                agentFileCandidates = found
                agentFileSelection = Set(found.map { $0.relativePath })
            } catch {
                agentFileMessage = "Couldn't scan that folder: \(error.localizedDescription)"
            }
        }
    }

    private func importAgentFiles() {
        guard let root = agentFileScannedRoot else { return }
        let selected = (agentFileCandidates ?? []).filter { agentFileSelection.contains($0.relativePath) }
        guard !selected.isEmpty else { return }
        agentFileImporting = true
        Task {
            defer { agentFileImporting = false }
            do {
                let vaultId = try agentInstructionsVaultId()
                var imported = 0
                for candidate in selected {
                    _ = try store.vaultIntakeApproveFile(
                        vaultId: vaultId,
                        root: root,
                        relativePath: candidate.relativePath,
                        project: "agent-instructions"
                    )
                    imported += 1
                }
                agentFileMessage = "Imported \(imported) file\(imported == 1 ? "" : "s") into your Library."
                agentFileCandidates = nil
                agentFileSelection = []
            } catch {
                agentFileMessage = "Couldn't import: \(error.localizedDescription)"
            }
        }
    }

    /// Reuses the Library's own starter vault when it exists (the one `VaultView` seeds on
    /// first open) so these land next to the example note, rather than scattering a second
    /// collection nobody asked for; falls back to creating one if this machine's Library is
    /// somehow still empty when this runs.
    private func agentInstructionsVaultId() throws -> String {
        guard let status = store.vaultOpen() else {
            throw AgentIntakeError.libraryUnavailable
        }
        if let existing = status.vaults.first(where: { $0.name == "Best Practices" || $0.name == "Agent Instructions" }) {
            return existing.id
        }
        return try store.vaultCreate(name: "Agent Instructions").id
    }
}

private enum AgentIntakeError: LocalizedError {
    case libraryUnavailable
    var errorDescription: String? {
        "couldn't open your Library"
    }
}



private struct RadioDot: View {
    let selected: Bool
    var body: some View {
        Circle()
            .strokeBorder(Color.secondary, lineWidth: 1.5)
            .background(Circle().fill(selected ? Color.accentColor : Color.clear).padding(3))
            .frame(width: 16, height: 16)
            .padding(.top, 3)
    }
}

private func stageCard<Content: View>(n: Int, title: String, active: Bool, done: Bool, @ViewBuilder content: () -> Content) -> some View {
    GroupBox {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                ZStack {
                    Circle().fill(done ? Color.green : (active ? Color.accentColor : Color.secondary.opacity(0.25)))
                        .frame(width: 18, height: 18)
                    Text(done ? "\u{2713}" : "\(n)")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(done || active ? .white : .secondary)
                }
                Text(title).font(.headline)
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    .opacity(active || done ? 1 : 0.55)
}

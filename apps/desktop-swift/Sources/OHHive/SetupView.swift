import SwiftUI
import OHHiveFFI
import ServiceManagement

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
        if store.snapshot?.paired != true { return 3 }
        return 4
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let a = assessment {
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
                    stageCard(n: 3, title: "Pair with your account", active: stage == 3, done: store.snapshot?.paired == true) {
                        pairStage(a)
                    }
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
                    Text("\(a.hardware.cpuCores) cores \u{00b7} \(gb(a.hardware.ramBytes)) memory")
                        .font(.caption).foregroundStyle(.secondary)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(a.hardware.gpuModel ?? "no GPU").fontWeight(.semibold)
                    Text("\(gb(a.budgetBytes)) usable for models \u{00b7} \(gb(a.hardware.diskFreeBytes)) free disk")
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
                Text("Most capable model that fits \(gb(a.budgetBytes)):")
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
                            Text("\(r.why) \u{00b7} \(gb(r.downloadBytes)) download")
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
                    Text("\(gb(p.completed)) of \(gb(p.total))")
                        .font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder private func pairStage(_ a: SetupAssessment) -> some View {
        if store.snapshot?.paired == true {
            Text("Paired.").foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 8) {
                Text("You'll get a short code to enter at ohghive.com \u{2014} the key lands here automatically." + (a.suggestServer ? " On the pairing page, pick \u{201c}Compute and server\u{201d} if you want this machine to hold artifacts too." : ""))
                    .foregroundStyle(.secondary)
                Button("Get a pairing code") { showPairSheet = true }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy)
            }
        }
    }

    @ViewBuilder private func goStage(_ a: SetupAssessment) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Start working now, open at login so this machine stays in the Hive" + (a.suggestServer ? ", and consider the Server section \u{2014} this machine has the disk for it." : "."))
                .foregroundStyle(.secondary)
            HStack {
                Button("Start working") {
                    Task { await goStart() }
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
}

private func gb(_ bytes: UInt64) -> String {
    let v = Double(bytes) / 1_073_741_824
    let decimals = v > 10 ? 0 : 1
    return String(format: "%.\(decimals)f GB", v)
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

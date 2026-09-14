import SwiftUI
import UniformTypeIdentifiers

/// Configure and launch one run. The split order shown here is the one llama.cpp uses
/// (RPC devices first, local GPU last) -- the app builds `--tensor-split` for you so the
/// Test-01 inversion can't happen again.
struct RunView: View {
    @Environment(BenchStore.self) private var store
    @State private var showImporter = false
    @State private var showLogImporter = false
    @State private var importTitle = ""

    var body: some View {
        @Bindable var store = store
        HSplitView {
            Form {
                Section("Preset") {
                    Menu("Load a preset…") {
                        ForEach(Presets.all) { p in Button(p.name) { store.apply(p) } }
                    }
                }
                Section("Test") {
                    TextField("Title (becomes the report filename)", text: $store.plan.title)
                    HStack {
                        TextField("Model (.gguf path)", text: $store.plan.modelPath)
                        Button("Choose…") { showImporter = true }
                    }
                    TextField("Notes for the report", text: $store.plan.notes, axis: .vertical).lineLimit(2...4)
                }
                Section("Placement — tensor-split order: RPC workers first, then the host") {
                    Picker("Host (runs llama-bench)", selection: $store.plan.hostID) {
                        Text("This Mac" + (store.config.localHostID.flatMap { id in store.config.fleet.first { $0.id == id }?.name }.map { " (\($0))" } ?? "")).tag(UUID?.none)
                        ForEach(store.config.fleet.filter { $0.id != store.config.localHostID && !$0.wiredIP.isEmpty }) { h in
                            Text("\(h.name) over SSH · usable ~\(ReportStore.num(h.usableGB)) GB").tag(UUID?.some(h.id))
                        }
                    }
                    .onChange(of: store.plan.hostID) { _, new in
                        // The host can't also be a worker.
                        if let new { store.plan.workers.removeAll { $0.hostID == new } }
                    }
                    ForEach($store.plan.workers) { $w in
                        HStack {
                            Picker("", selection: $w.hostID) {
                                ForEach(store.config.fleet.filter { $0.id != store.config.localHostID && $0.id != store.plan.hostID }) { h in
                                    Text("\(h.name) · \(h.backend.rawValue) · usable ~\(ReportStore.num(h.usableGB)) GB").tag(h.id)
                                }
                            }.labelsHidden()
                            TextField("GB", value: $w.shareGB, format: .number).frame(width: 60)
                            Text("GB")
                            Button(role: .destructive) { store.plan.workers.removeAll { $0.id == w.id } } label: { Image(systemName: "minus.circle") }
                                .buttonStyle(.borderless)
                            if let h = store.config.fleet.first(where: { $0.id == w.hostID }), w.shareGB > h.usableGB {
                                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange).help("Over \(h.name)'s measured usable budget (~\(ReportStore.num(h.usableGB)) GB)")
                            }
                        }
                    }
                    HStack {
                        Button { if let h = store.config.fleet.first(where: { $0.id != store.config.localHostID && $0.id != store.plan.hostID }) { store.plan.workers.append(.init(hostID: h.id, shareGB: 8)) } } label: { Label("Add worker", systemImage: "plus") }
                        Spacer()
                        Text(store.plan.hostID == nil ? "Local share" : "Host share").foregroundStyle(.secondary)
                        TextField("GB", value: $store.plan.localShareGB, format: .number).frame(width: 60)
                        Text("GB")
                    }
                    if store.plan.isSplit {
                        LabeledContent("--tensor-split") { Text(store.plan.tensorSplit).font(.system(.body, design: .monospaced)) }
                        LabeledContent("Total") { Text("\(ReportStore.num(store.plan.workers.reduce(0) { $0 + $1.shareGB } + store.plan.localShareGB)) GB across \(store.plan.workers.count + 1) nodes") }
                        Toggle("Restart every worker before launch (they're single-client and never notice a dead host)", isOn: $store.plan.restartWorkers)
                    } else {
                        Text(store.plan.hostID == nil ? "No workers — runs on this Mac alone." : "No workers — runs on the host alone, over SSH.").foregroundStyle(.secondary)
                    }
                    if let h = store.plan.host(in: store.config.fleet), store.plan.localShareGB > h.usableGB {
                        Label("Host share exceeds \(h.name)'s measured usable budget (~\(ReportStore.num(h.usableGB)) GB)", systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).font(.caption)
                    }
                }
                Section("llama-bench") {
                    HStack {
                        Stepper("Prompt -p \(store.plan.promptTokens)", value: $store.plan.promptTokens, in: 64...4096, step: 64)
                        Stepper("Generate -n \(store.plan.genTokens)", value: $store.plan.genTokens, in: 32...4096, step: 32)
                        Stepper("Reps -r \(store.plan.repetitions)", value: $store.plan.repetitions, in: 1...10)
                    }
                    TextField("Extra args (e.g. -b 128 -ub 128)", text: $store.plan.extraArgs)
                }
                Section {
                    HStack {
                        if store.phase == .running {
                            Button(role: .destructive) { store.cancel() } label: { Label("Abort", systemImage: "stop.fill") }
                        } else {
                            Button { store.start() } label: { Label("Run test", systemImage: "play.fill") }
                                .buttonStyle(.borderedProminent).tint(.honey).keyboardShortcut(.return, modifiers: .command)
                        }
                        Spacer()
                        Button { showLogImporter = true } label: { Label("Import a log…", systemImage: "square.and.arrow.down") }
                            .help("File a report from a llama-bench log you ran by hand")
                    }
                    if let url = store.lastFiled {
                        HStack {
                            Image(systemName: "doc.text").foregroundStyle(.secondary)
                            Text("Filed: \(url.lastPathComponent)").font(.caption)
                            Button("Reveal") { NSWorkspace.shared.activateFileViewerSelecting([url]) }.buttonStyle(.link).font(.caption)
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .frame(minWidth: 380, idealWidth: 420, maxWidth: 500)

            VStack(spacing: 0) {
                RunTimelineView().padding(12)
                Divider()
                LogPane(title: "Live output")
            }
            .frame(minWidth: 380, maxWidth: .infinity)
        }
        .navigationTitle("Run")
        .fileImporter(isPresented: $showImporter, allowedContentTypes: [UTType(filenameExtension: "gguf") ?? .data, .data]) { r in
            if case .success(let u) = r { store.plan.modelPath = u.path }
        }
        .fileImporter(isPresented: $showLogImporter, allowedContentTypes: [.plainText, .log, .data]) { r in
            if case .success(let u) = r {
                let title = u.deletingPathExtension().lastPathComponent
                store.importLog(url: u, title: "Imported - \(title)")
            }
        }
    }
}

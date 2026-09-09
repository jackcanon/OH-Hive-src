import SwiftUI
import OHHiveFFI
import ServiceManagement

/// Parity with the Tauri app's Settings tab (`apps/desktop/src/App.tsx`'s `Settings`
/// component): model picker, launch-at-login, backend URL/region, and the ADR-006 trust
/// switches. "Launch at login" uses `SMAppService` (ServiceManagement) instead of Tauri's
/// `tauri-plugin-autostart` -- the native macOS 13+ API for exactly this, no plugin needed.
struct SettingsView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var llamaUrl: String = ""
    @State private var region: String = ""
    @State private var autostartOn = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let snap = store.snapshot {
                    modelCard(snap)
                    launchAtLoginCard()
                    backendCard(snap)
                    trustCard(snap)
                } else {
                    ProgressView()
                }
            }
            .padding(20)
        }
        .navigationTitle("Settings")
        .onAppear {
            llamaUrl = store.snapshot?.llamaUrl ?? ""
            region = store.snapshot?.region ?? ""
            autostartOn = SMAppService.mainApp.status == .enabled
        }
    }

    @ViewBuilder
    private func modelCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("Model") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Which local model takes cards. Cards that name a model override this.")
                    .font(.caption).foregroundStyle(.secondary)
                Picker("Model", selection: Binding(
                    get: { snap.model ?? "" },
                    set: { store.setConfig("HIVE_MODEL", $0) }
                )) {
                    Text("Automatic (first available)").tag("")
                    ForEach(snap.models, id: \.self) { m in Text(m).tag(m) }
                }
                .labelsHidden()
                if snap.running {
                    Text("Takes effect on the next card.").font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func launchAtLoginCard() -> some View {
        GroupBox("Launch at login") {
            HStack {
                Text("Open OH Hive when you sign in, so the node is ready without a click.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Toggle("", isOn: Binding(
                    get: { autostartOn },
                    set: { newValue in
                        do {
                            if newValue {
                                try SMAppService.mainApp.register()
                            } else {
                                try SMAppService.mainApp.unregister()
                            }
                            autostartOn = newValue
                        } catch {
                            store.lastError = "launch at login: \(error.localizedDescription)"
                        }
                    }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func backendCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("Backend") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Ollama / llama-server URL").font(.caption).foregroundStyle(.secondary)
                TextField("http://127.0.0.1:11434", text: $llamaUrl)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if llamaUrl != snap.llamaUrl { store.setConfig("HIVE_LLAMA_URL", llamaUrl) }
                    }
                Text("Region (optional, e.g. us-west)").font(.caption).foregroundStyle(.secondary)
                TextField("", text: $region)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if region != (snap.region ?? "") { store.setConfig("HIVE_REGION", region) }
                    }
                HStack {
                    Text("Hub: \(snap.hubUrl)").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Refresh") { store.refresh() }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func trustCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("Trust") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Whole-node choices that decide what a project's agent loop may do on this machine (ADR-006). Takes effect on the next check-in.")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("Allow projects to reach the internet from this machine", isOn: Binding(
                    get: { snap.allowInternet },
                    set: { store.setConfig("HIVE_ALLOW_INTERNET", $0 ? "true" : "false") }
                ))
                Text("Off by default. A card never gets network it didn't declare, even when this is on.")
                    .font(.caption).foregroundStyle(.secondary)
                Picker("Tools", selection: Binding(
                    get: { snap.toolsLevel },
                    set: { store.setConfig("HIVE_TOOLS_LEVEL", $0) }
                )) {
                    Text("Sandboxed tools (recommended)").tag("sandboxed_tools")
                    Text("Inference only").tag("inference_only")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

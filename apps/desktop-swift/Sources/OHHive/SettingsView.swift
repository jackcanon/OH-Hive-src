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
    @State private var whisperUrl: String = ""
    @State private var whisperModel: String = ""
    @State private var comfyuiUrl: String = ""
    @State private var comfyuiCheckpoint: String = ""

    // MARK: - BYOK cloud keys (2026-09-13, Jack: "I'd like the picker in the swift app as well,
    // in case people aren't always logging into the website. They need to be able to operate
    // independent of each other") -- mirrors apps/web/app/settings/page.tsx's key + per-key
    // model picker, but reachable purely from this node's key (see byok_keys.rs's header).
    @State private var byokStatus: ByokKeysStatus?
    @State private var byokLoaded = false
    @State private var byokProvider = "anthropic"
    @State private var byokKeyDraft = ""
    @State private var byokModelDrafts: [String: String] = [:]
    @State private var byokBusyProvider: String?
    @State private var byokError: String?

    private let byokProviders = ["anthropic", "openai", "nous"]

    private func byokLabel(_ p: String) -> String {
        switch p {
        case "anthropic": return "Anthropic"
        case "openai": return "OpenAI"
        case "nous": return "Nous (Hermes)"
        default: return p
        }
    }

    private func byokKeyPlaceholder(_ p: String) -> String {
        switch p {
        case "anthropic": return "sk-ant-\u{2026}"
        case "openai": return "sk-\u{2026}"
        case "nous": return "your Nous Portal key"
        default: return ""
        }
    }

    private func byokModelPlaceholder(_ p: String) -> String {
        switch p {
        case "anthropic": return "default \u{2014} e.g. claude-opus-5, claude-sonnet-5"
        case "openai": return "default \u{2014} e.g. gpt-5, gpt-5-mini"
        case "nous": return "default \u{2014} a Nous Portal model slug"
        default: return "default"
        }
    }

    private func byokInfo(_ p: String) -> ByokKeyInfo? {
        switch p {
        case "anthropic": return byokStatus?.anthropic
        case "openai": return byokStatus?.openai
        case "nous": return byokStatus?.nous
        default: return nil
        }
    }

    // 2026-09-13, Jack: "we need to split it up into tabbed sections, there is a lot of
    // scrolling and settings doesn't seem to be re-sizeable" -- each former stacked GroupBox
    // becomes its own tab (macOS's standard Preferences pattern: a TabView with .tabItem here
    // renders as the icon-and-label row across the top, not a sidebar), and the window gets an
    // explicit resizable frame range instead of sizing to whatever the tallest tab needed.
    var body: some View {
        Group {
            if let snap = store.snapshot {
                TabView {
                    tabScroll { modelCard(snap); launchAtLoginCard() }
                        .tabItem { Label("General", systemImage: "gearshape") }

                    tabScroll { byokCard() }
                        .tabItem { Label("Cloud Keys", systemImage: "key.fill") }

                    tabScroll { backendCard(snap) }
                        .tabItem { Label("Backend", systemImage: "network") }

                    tabScroll { mediaBackendsCard(snap) }
                        .tabItem { Label("Media", systemImage: "waveform") }

                    tabScroll { trustCard(snap) }
                        .tabItem { Label("Trust", systemImage: "lock.shield") }
                }
            } else {
                ProgressView().frame(width: 480, height: 360)
            }
        }
        .frame(minWidth: 480, idealWidth: 560, maxWidth: 760, minHeight: 420, idealHeight: 520, maxHeight: 800)
        .navigationTitle("Settings")
        .onAppear {
            llamaUrl = store.snapshot?.llamaUrl ?? ""
            region = store.snapshot?.region ?? ""
            autostartOn = SMAppService.mainApp.status == .enabled
            whisperUrl = store.snapshot?.whisperUrl ?? ""
            whisperModel = store.snapshot?.whisperModel ?? ""
            comfyuiUrl = store.snapshot?.comfyuiUrl ?? ""
            comfyuiCheckpoint = store.snapshot?.comfyuiCheckpoint ?? ""
        }
        .task { await refreshByokStatus() }
    }

    /// One tab's worth of content, still scrollable in case a single section (Media, Cloud Keys)
    /// runs long on a small display -- but now each tab only has to fit its own section, not
    /// every section stacked together.
    @ViewBuilder
    private func tabScroll<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                content()
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func refreshByokStatus() async {
        byokStatus = await store.byokKeysStatus()
        byokLoaded = true
    }

    @ViewBuilder
    private func byokCard() -> some View {
        GroupBox("Cloud API keys") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Bring your own Anthropic, OpenAI, or Nous key so chat and the coding agent use your account instead of the shared pool. Works from this Mac even if you never sign into the website \u{2014} same keys either way.")
                    .font(.caption).foregroundStyle(.secondary)

                if !byokLoaded {
                    ProgressView().controlSize(.small)
                } else {
                    ForEach(byokProviders, id: \.self) { p in
                        byokProviderRow(p)
                        if p != byokProviders.last { Divider() }
                    }
                }

                if let err = byokError {
                    Text(err).font(.caption).foregroundStyle(.red)
                }

                Divider().padding(.vertical, 2)

                HStack(spacing: 8) {
                    Picker("", selection: $byokProvider) {
                        ForEach(byokProviders, id: \.self) { p in Text(byokLabel(p)).tag(p) }
                    }
                    .labelsHidden()
                    .frame(width: 150)
                    SecureField(byokKeyPlaceholder(byokProvider), text: $byokKeyDraft)
                        .textFieldStyle(.roundedBorder)
                    Button("Save key") {
                        Task { await saveByokKey() }
                    }
                    .disabled(byokKeyDraft.trimmingCharacters(in: .whitespaces).count < 8 || byokBusyProvider != nil)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func byokProviderRow(_ p: String) -> some View {
        if let info = byokInfo(p) {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("\(byokLabel(p)) \u{00B7} \u{2022}\u{2022}\u{2022}\u{2022}\(info.last4)")
                        .font(.caption)
                    Spacer()
                    Button("Remove") {
                        Task { await removeByokKey(p) }
                    }
                    .buttonStyle(.link)
                    .font(.caption)
                    .disabled(byokBusyProvider != nil)
                }
                HStack(spacing: 8) {
                    TextField(byokModelPlaceholder(p), text: Binding(
                        get: { byokModelDrafts[p] ?? info.preferredModel ?? "" },
                        set: { byokModelDrafts[p] = $0 }
                    ))
                    .textFieldStyle(.roundedBorder)
                    .font(.caption)
                    let draft = byokModelDrafts[p] ?? info.preferredModel ?? ""
                    Button("Save model") {
                        Task { await saveByokModel(p, draft) }
                    }
                    .font(.caption)
                    .disabled(byokBusyProvider != nil || draft == (info.preferredModel ?? ""))
                }
            }
        } else {
            Text("No \(byokLabel(p)) key on file.").font(.caption).foregroundStyle(.secondary)
        }
    }

    private func saveByokKey() async {
        byokError = nil
        byokBusyProvider = byokProvider
        defer { byokBusyProvider = nil }
        do {
            try await store.setByokKey(provider: byokProvider, key: byokKeyDraft)
            byokKeyDraft = ""
            await refreshByokStatus()
        } catch {
            byokError = "\(byokLabel(byokProvider)): \(error.localizedDescription)"
        }
    }

    private func removeByokKey(_ p: String) async {
        byokError = nil
        byokBusyProvider = p
        defer { byokBusyProvider = nil }
        do {
            _ = try await store.removeByokKey(provider: p)
            byokModelDrafts[p] = nil
            await refreshByokStatus()
        } catch {
            byokError = "\(byokLabel(p)): \(error.localizedDescription)"
        }
    }

    private func saveByokModel(_ p: String, _ model: String) async {
        byokError = nil
        byokBusyProvider = p
        defer { byokBusyProvider = nil }
        do {
            try await store.setByokKeyModel(provider: p, model: model)
            await refreshByokStatus()
        } catch {
            byokError = "\(byokLabel(p)): \(error.localizedDescription)"
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
                Text("Open Hive when you sign in, so the node is ready without a click.")
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
                Text("Where this Mac actually runs local models -- the same server the General tab's Model picker lists models from. Change this only if you run llama-server yourself, or point Ollama at a non-default port or another machine on your network; most people never need to touch it.")
                    .font(.caption).foregroundStyle(.secondary)

                Text("Ollama / llama-server URL").font(.caption).foregroundStyle(.secondary)
                TextField("http://127.0.0.1:11434", text: $llamaUrl)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if llamaUrl != snap.llamaUrl { store.setConfig("HIVE_LLAMA_URL", llamaUrl) }
                    }

                Text("Region (optional, e.g. us-west) -- a hint so nearby regional servers and other nodes can be matched to this one. Leave blank if you don't know what this means.")
                    .font(.caption).foregroundStyle(.secondary)
                TextField("", text: $region)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if region != (snap.region ?? "") { store.setConfig("HIVE_REGION", region) }
                    }

                Divider().padding(.vertical, 2)

                Text("Hive network this node reports to -- shared by every member, not something you configure.")
                    .font(.caption).foregroundStyle(.secondary)
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
    private func mediaBackendsCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("Media backends (Hive network)") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Point these at a whisper.cpp server / ComfyUI instance you run (on this Mac or elsewhere) to use the Transcribe and Generate tabs' network path, and to let this node take other members' speech/image cards later. Leave blank to skip \u{2014} most nodes won't set these up.")
                    .font(.caption).foregroundStyle(.secondary)

                Text("whisper.cpp server URL").font(.caption).foregroundStyle(.secondary)
                TextField("http://127.0.0.1:8081", text: $whisperUrl)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if whisperUrl != (snap.whisperUrl ?? "") { store.setConfig("HIVE_WHISPER_URL", whisperUrl) }
                    }
                Text("whisper.cpp model name (for display only)").font(.caption).foregroundStyle(.secondary)
                TextField("ggml-large-v3-turbo", text: $whisperModel)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if whisperModel != (snap.whisperModel ?? "") { store.setConfig("HIVE_WHISPER_MODEL", whisperModel) }
                    }

                Divider().padding(.vertical, 4)

                Text("ComfyUI server URL").font(.caption).foregroundStyle(.secondary)
                TextField("http://127.0.0.1:8188", text: $comfyuiUrl)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if comfyuiUrl != (snap.comfyuiUrl ?? "") { store.setConfig("HIVE_COMFYUI_URL", comfyuiUrl) }
                    }
                Text("ComfyUI checkpoint filename").font(.caption).foregroundStyle(.secondary)
                TextField("flux1-dev-fp8.safetensors", text: $comfyuiCheckpoint)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        if comfyuiCheckpoint != (snap.comfyuiCheckpoint ?? "") { store.setConfig("HIVE_COMFYUI_CHECKPOINT", comfyuiCheckpoint) }
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

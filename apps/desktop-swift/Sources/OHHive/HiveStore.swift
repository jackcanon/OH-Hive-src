import Foundation
import SwiftUI
import OHHiveFFI

/// Bridges the UniFFI-generated `HiveNode` (see `crates/ohhive-ffi`, ADR-018) into SwiftUI:
/// owns the one `HiveNode` instance for the app's lifetime, republishes `snapshot()` as
/// `@Published` state, and turns `HiveEventListener` callbacks -- which land on a Rust-owned
/// background thread, not the main actor -- into `@MainActor` UI updates.
/// `@unchecked Sendable`: `HiveStore` is `@MainActor`-isolated, so all its mutable state is
/// already serialized through the main actor -- the `@unchecked` here just tells the compiler
/// what's already true, so `ChatEngine`'s `NodeStatusTool` (a `Sendable` `Tool` conformer per
/// Foundation Models' `Tool` protocol) can hold a reference and `await` into it safely.
@MainActor
final class HiveStore: ObservableObject, @unchecked Sendable {
    @Published var snapshot: HiveSnapshot?
    @Published var activity: [ActivityEntry] = []
    @Published var lastError: String?
    /// Latest `onSetupProgress` from an in-flight `ollamaInstall`/`ollamaPull` (phase 2). `nil`
    /// once nothing is running -- `SetupView` clears it after a `done` progress update.
    @Published var setupProgress: SetupProgress?
    /// Regional-server + reachability state (ADR-018 task #70). Refreshed alongside `snapshot`
    /// on the same 5s poll, plus immediately after `serverStart`/`serverStop`.
    @Published var server: ServerInfo?
    /// Cloudflare Tunnel reachability state (ADR-018 task #71). Refreshed alongside `server`.
    @Published var tunnel: TunnelInfo?

    private let node: HiveNode
    private var pollTask: Task<Void, Never>?

    init() {
        node = HiveNode()
        node.setListener(listener: Bridge(store: self))
        refresh()
        // The "changed" callback (pairing claimed, worker stopped) already triggers an
        // immediate refresh; this timer just covers hub-side fields (summary, models) that
        // drift on their own between events.
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                self?.refresh()
            }
        }
    }

    deinit {
        pollTask?.cancel()
    }

    var about: AboutInfo { node.about() }

    func refresh() {
        Task {
            do {
                let snap = try await node.snapshot()
                self.snapshot = snap
                self.activity = snap.activity
            } catch {
                self.lastError = String(describing: error)
            }
            self.server = await node.serverSnapshot()
            self.tunnel = await node.tunnelSnapshot()
        }
    }

    func startPairing() async throws -> PairingView {
        try await node.pairBegin()
    }

    func cancelPairing() async {
        await node.pairCancel()
    }

    func startWorking() async {
        do {
            try await node.workerStart()
        } catch {
            self.lastError = String(describing: error)
        }
        refresh()
    }

    func stopWorking(forget: Bool = true) async {
        await node.workerStop(forget: forget)
        refresh()
    }

    func setConfig(_ key: String, _ value: String) {
        do {
            _ = try node.setConfig(key: key, value: value)
        } catch {
            self.lastError = String(describing: error)
        }
        // Tauri's Settings tab always re-fetches after a `set_config` call so toggles/pickers
        // reflect the saved value immediately rather than waiting for the 5s poll -- match that.
        refresh()
    }

    // MARK: - Setup wizard (phase 2, ADR-018 decision 4)

    func assess() async throws -> SetupAssessment {
        try await node.assess()
    }

    func ollamaInstall() async {
        setupProgress = nil
        do {
            try await node.ollamaInstall()
        } catch {
            self.lastError = String(describing: error)
        }
    }

    func ollamaPull(_ model: String) async {
        setupProgress = nil
        do {
            try await node.ollamaPull(model: model)
        } catch {
            self.lastError = String(describing: error)
        }
    }

    func setupFinish() {
        do {
            try node.setupFinish()
        } catch {
            self.lastError = String(describing: error)
        }
        refresh()
    }

    // MARK: - Regional server (phase 2, ADR-018 task #70)

    func serverSnapshot() async -> ServerInfo {
        await node.serverSnapshot()
    }

    /// Raw `hive.node_projects_overview` JSON (array), for the Kanban view's "Hive" column
    /// (task #74). `nil` on any error (e.g. unpaired, hub unreachable) -- KanbanView shows an
    /// empty-state message rather than surfacing a raw error for what's a read-only nice-to-have.
    func kanbanCloudProjects() async -> String? {
        try? await node.kanbanCloudProjects()
    }

    func setDataDir(_ path: String) throws -> String {
        try node.setDataDir(path: path)
    }

    func serverStart(publicUrl: String?, storageGb: UInt32?, tier: String?) async {
        do {
            try await node.serverStart(publicUrl: publicUrl, storageGb: storageGb, tier: tier)
        } catch {
            self.lastError = String(describing: error)
        }
        refresh()
    }

    func serverStop(forget: Bool = true) async {
        await node.serverStop(forget: forget)
        refresh()
    }

    // MARK: - M8 media backends (Hive network, as opposed to on-device)

    /// Network speech-to-text via whisper.cpp (see `TranscribeEngine.swift` for the separate,
    /// on-device Apple path). Throws with a friendly message if `HIVE_WHISPER_URL` isn't set.
    func transcribeWhisper(audioPath: String, language: String? = nil) async throws -> TranscribeResult {
        try await node.transcribeWhisper(audioPath: audioPath, language: language)
    }

    /// Network text-to-image via ComfyUI. Throws with a friendly message if `HIVE_COMFYUI_URL`/
    /// `HIVE_COMFYUI_CHECKPOINT` aren't set. Can take a while -- callers should show progress.
    func generateImageComfyUI(prompt: String, negativePrompt: String? = nil) async throws -> GeneratedImage {
        try await node.generateImageComfyui(prompt: prompt, negativePrompt: negativePrompt)
    }

    // MARK: - Cloudflare Tunnel (ADR-018 task #71)

    func tunnelSnapshot() async -> TunnelInfo {
        await node.tunnelSnapshot()
    }

    func tunnelLogin(binPath: String) async {
        do {
            try await node.tunnelLogin(binPath: binPath)
        } catch {
            self.lastError = String(describing: error)
        }
    }

    func tunnelSetup(binPath: String, name: String, hostname: String) async throws -> String {
        try await node.tunnelSetup(binPath: binPath, name: name, hostname: hostname)
    }

    /// Satisfies the Rust-defined `HiveEventListener` callback interface. A separate object
    /// (rather than `HiveStore` conforming directly) because these calls arrive on whatever
    /// thread the Rust/Tokio side happens to be using -- it just hops back to the main actor.
    private final class Bridge: HiveEventListener {
        weak var store: HiveStore?
        init(store: HiveStore) { self.store = store }

        func onActivity(entry: ActivityEntry) {
            Task { @MainActor in
                guard let store else { return }
                store.activity.insert(entry, at: 0)
                if store.activity.count > 60 { store.activity.removeLast() }
            }
        }

        func onChanged() {
            Task { @MainActor in self.store?.refresh() }
        }

        func onSetupProgress(progress: SetupProgress) {
            Task { @MainActor in self.store?.setupProgress = progress }
        }
    }
}

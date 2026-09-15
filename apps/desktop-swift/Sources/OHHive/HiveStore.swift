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
    /// BYOK API key status, prefetched here (not lazily on first chat-tab appearance) so
    /// `ChatEngine.loadByokKeysIfNeeded()` (ChatEngine.swift) almost never has to await a fresh
    /// fetch -- 2026-09-15, Jack: the "Loading your keys..." flash on opening a new chat should
    /// just auto-detect up front instead. Refreshed on the same 5s poll as `server`/`tunnel` so a
    /// key added or removed in Settings elsewhere is picked up without restarting the app.
    @Published var byokKeys: ByokKeysStatus?

    func chatgptAccount(action: String, binary: String? = nil) async throws -> ChatGptAccountStatus {
        try await node.chatgptAccount(action: action, binary: binary)
    }

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
            self.byokKeys = await self.byokKeysStatus()
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

    /// Hosted text-to-image via OpenAI (tasks #125-128), BYOK-only as of 2026-09-13 -- uses the
    /// member's own OpenAI key (Settings, web app), billed by OpenAI directly, never Honey. No
    /// member-run ComfyUI server needed. Node-key authenticated the same way `submitFeatureRequest`
    /// is, since this Mac has no member Supabase session.
    func generateImageHosted(prompt: String, negativePrompt: String? = nil) async throws -> GeneratedImage {
        try await node.generateImageHosted(prompt: prompt, negativePrompt: negativePrompt)
    }

    // MARK: - BYOK chat (2026-09-13, alongside the on-device path in ChatEngine.swift)

    /// `history` ends with the new user turn; stateless, same shape as the web app's /new page --
    /// see `crates/ohhive-ffi/src/chat.rs`'s header for why. Throws a friendly message when no key
    /// is on file (ChatEngine.swift surfaces it as a system-role message, same as the on-device
    /// unavailability messages).
    func sendByokChat(history: [ByokChatTurn]) async throws -> ChatReply {
        try await node.sendByokChat(history: history)
    }

    /// Additive twin of `sendByokChat` for the chat composer's two-stage provider-then-model
    /// picker (2026-09-13) -- `provider` is explicit ("anthropic" | "openai" | "nous"), `model`
    /// overrides that provider's saved preferred_model for this turn only when non-nil/non-empty.
    func sendByokChat(history: [ByokChatTurn], provider: String, model: String?) async throws -> ChatReply {
        try await node.sendByokChatWith(history: history, provider: provider, model: model)
    }

    /// Persistent chat memory (2026-09-13, Hermes-agent survey) -- what BYOK sessions have taught
    /// the assistant about this member, read here so the on-device path can fold it into its
    /// instructions too. `nil` on any error (unpaired, hub unreachable), same swallow-and-show-
    /// nothing behavior as `kanbanCloudProjects()` -- this is a nice-to-have, not a blocker for
    /// opening the Chat tab.
    func chatMemory() async -> ChatMemory? {
        try? await node.getChatMemory()
    }

    // MARK: - BYOK key/model management (2026-09-13, Jack: "I'd like the picker in the swift app
    // as well, in case people aren't always logging into the website. They need to be able to
    // operate independent of each other") -- same node-key-resolved RPCs as the web Settings
    // page's key management, so this works whether or not the member has ever visited the site.

    /// `nil` on any error (unpaired, hub unreachable) -- same swallow-and-show-nothing behavior as
    /// `chatMemory()`; SettingsView shows "unavailable" rather than surfacing a raw error for a
    /// panel most members will only glance at.
    func byokKeysStatus() async -> ByokKeysStatus? {
        try? await node.byokKeysStatus()
    }

    /// Throws a friendly message on failure (e.g. malformed key) -- SettingsView surfaces it
    /// inline next to the field being edited, same convention as `lastError` elsewhere.
    func setByokKey(provider: String, key: String) async throws {
        try await node.setByokKey(provider: provider, key: key)
    }

    /// Returns whether a key was actually removed (`false` if there was none on file).
    func removeByokKey(provider: String) async throws -> Bool {
        try await node.removeByokKey(provider: provider)
    }

    /// `model` empty clears the override back to the provider's default.
    func setByokKeyModel(provider: String, model: String) async throws {
        try await node.setByokKeyModel(provider: provider, model: model)
    }

    /// Live model catalog for `provider` ("anthropic" | "openai" | "nous"), 2026-09-15 -- see
    /// `ProviderModelPicker`'s doc for why this replaced the old Default/Custom-only stage 2.
    /// `nil` means the fetch failed (no key on file, network error, provider API error) -- the
    /// picker falls back to Default/Custom in that case, same swallow-and-degrade behavior
    /// `byokKeysStatus()` already establishes rather than surfacing a raw error in the composer.
    func byokModels(provider: String) async -> [ByokModelInfo]? {
        try? await node.listByokModels(provider: provider)
    }

    // MARK: - Release notes (2026-09-13, #178 -- "when users login after an update there should
    // be release notes"). Same node-key resolution as chat memory above.

    /// Whatever release notes this machine's owning member hasn't seen yet, oldest first. `nil`
    /// on any error (unpaired, hub unreachable) -- same swallow-and-show-nothing behavior as
    /// `chatMemory()`, since a failed fetch here shouldn't block using the app.
    func releaseNotesUnseen() async -> [ReleaseNote]? {
        try? await node.releaseNotesUnseen()
    }

    /// Acknowledges every release note published so far -- called once the member dismisses the
    /// "what's new" sheet, so it won't come back on this machine or any other they sign into.
    func releaseNotesMarkSeen() async {
        try? await node.releaseNotesMarkSeen()
    }

    // MARK: - Private Fleet channel (2026-09-13, ADR-022 S2, #184 -- Swift catching up to the
    // web app's #183). Same node-key resolution as feedback/chat memory above.

    /// `nodeId: nil` is the "all machines" view; pass one of `snapshot`'s node ids to filter to a
    /// single paired machine -- same fleet-wide feed either way (ADR-022 S2 decision 2). Throws
    /// rather than swallowing errors: unlike `kanbanCloudProjects`, this view's whole point is to
    /// show real state, so a silent empty list would be misleading.
    func channelList(nodeId: String? = nil, limit: UInt32 = 200) async throws -> [ChannelPost] {
        try await node.channelList(nodeId: nodeId, limit: limit)
    }

    /// Posts a member-authored message into the channel from this machine -- the same action as
    /// typing into the web app's Private Fleet page.
    func channelPost(_ body: String) async throws -> ChannelPost {
        try await node.channelPost(body: body)
    }

    // MARK: - Feedback (feature requests & bug reports)

    /// Submits as this node's owning member (resolved server-side from the node key -- see
    /// `crates/ohhive-ffi/src/feedback.rs`'s header for why the desktop app can't just use
    /// `auth.uid()` the way the web app's /requests page does).
    func submitFeatureRequest(title: String, description: String) async throws {
        try await node.submitFeatureRequest(title: title, description: description)
    }

    /// Same node-key resolution as `submitFeatureRequest`, see `crates/ohhive-ffi/src/feedback.rs`.
    /// Text-only for now -- no attachment upload from this app (Settings/web is where screenshots
    /// and logs get attached), see that file's header for why.
    func submitBugReport(title: String, description: String, anonymous: Bool) async throws {
        try await node.submitBugReport(title: title, description: description, anonymous: anonymous)
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

    // MARK: - Private Fleet vault (2026-09-14, ADR-028 "central-query v1" -- Sif built and
    // tested the storage/search layer; this is the UniFFI wiring her build report flagged as
    // missing, see `crates/ohhive-ffi/src/local_hub.rs`'s header for the exact scope of this
    // first pass: this machine only, notes added by hand, no cross-machine reading yet).
    // All synchronous (local SQLite, no network) -- same convention as `setConfig`/`about`.

    /// Opens (creating on first use) this machine's vault store. Safe to call repeatedly --
    /// `VaultView` calls this `onAppear`.
    func vaultOpen() -> VaultHostStatus? {
        do {
            return try node.vaultOpen()
        } catch {
            self.lastError = String(describing: error)
            return nil
        }
    }

    func vaultCreate(name: String) throws -> VaultInfo {
        try node.vaultCreate(name: name)
    }

    func vaultSearch(vaultId: String, query: String, limit: UInt32 = 20) throws -> [VaultHit] {
        try node.vaultSearch(vaultId: vaultId, query: query, limit: limit)
    }

    func vaultRead(vaultId: String, documentId: String, revision: String) throws -> VaultDocument {
        try node.vaultRead(vaultId: vaultId, documentId: documentId, revision: revision)
    }

    /// `documentId: nil` creates a new note; pass the id back (from a prior `VaultDocument`) to
    /// edit one in place -- there is no list-documents call in this pass, so `VaultView` holds
    /// onto ids itself once a note has been created or found via search.
    func vaultAddNote(vaultId: String, documentId: String?, path: String, title: String, content: String) throws -> VaultDocument {
        try node.vaultAddNote(vaultId: vaultId, documentId: documentId, path: path, title: title, content: content)
    }

    func vaultRemoveNote(vaultId: String, documentId: String) throws {
        try node.vaultRemoveNote(vaultId: vaultId, documentId: documentId)
    }

    /// Lists `.md` files under `root` (an absolute path on this machine) for a member to review
    /// before approving any into this vault -- read-only, nothing is added until
    /// `vaultIntakeApproveFile` is called for a specific file.
    func vaultIntakeListCandidates(root: String) throws -> [IntakeCandidate] {
        try node.vaultIntakeListCandidates(root: root)
    }

    /// Submits one member-approved file from `root` into `vaultId` (must already be a managed,
    /// non-folder vault). Safe to call again for the same file later: unchanged content is a
    /// no-op, changed content replaces it in place.
    func vaultIntakeApproveFile(vaultId: String, root: String, relativePath: String, project: String?) throws -> IntakeReceipt {
        try node.vaultIntakeApproveFile(vaultId: vaultId, root: root, relativePath: relativePath, project: project)
    }

    /// Current maintenance policy/status for one vault -- `nil` means maintenance has never been
    /// configured for it. The host loop that actually runs ticks (2026-09-15) starts once,
    /// automatically, from `vaultOpen()` -- this just reads where things stand.
    func vaultMaintenanceStatus(vaultId: String) -> VaultMaintenanceStatus? {
        try? node.vaultMaintenanceStatus(vaultId: vaultId)
    }

    /// Enables/updates this vault's maintenance policy (disabled by default). The host loop is
    /// already running; this only changes whether/how often it does anything for this vault.
    func vaultConfigureMaintenance(vaultId: String, policy: VaultMaintenancePolicy) throws {
        try node.vaultConfigureMaintenance(vaultId: vaultId, policy: policy)
    }

    // MARK: - Skills (ADR-027 decision 5) -- workspace-local, so every call takes the folder to
    // look under; unlike the vault there's no single "the" library on this machine to open once.
    // All synchronous (local disk, no network) -- same convention as the vault methods above.

    /// Lists the skills saved under `<workspacePath>/.hive/skills/`. Empty (no thrown error)
    /// covers both "no skills yet" and "not a workspace with a `.hive/` at all".
    func skillsList(workspacePath: String) throws -> SkillInventory {
        try node.skillsList(workspacePath: workspacePath)
    }

    /// Full procedure text for one skill, e.g. to show before the member deletes it.
    func skillsRead(workspacePath: String, id: String) throws -> SkillDocument {
        try node.skillsRead(workspacePath: workspacePath, id: id)
    }

    /// `revision` must be what `skillsList`/`skillsRead` last showed the caller, so a stale
    /// Settings list can't delete a skill out from under a state it never actually saw.
    func skillsDelete(workspacePath: String, id: String, revision: String) throws {
        try node.skillsDelete(workspacePath: workspacePath, id: id, revision: revision)
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

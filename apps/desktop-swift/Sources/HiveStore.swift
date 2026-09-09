import Foundation
import SwiftUI

/// Bridges the UniFFI-generated `HiveNode` (see `crates/ohhive-ffi`, ADR-018) into SwiftUI:
/// owns the one `HiveNode` instance for the app's lifetime, republishes `snapshot()` as
/// `@Published` state, and turns `HiveEventListener` callbacks -- which land on a Rust-owned
/// background thread, not the main actor -- into `@MainActor` UI updates.
@MainActor
final class HiveStore: ObservableObject {
    @Published var snapshot: HiveSnapshot?
    @Published var activity: [ActivityEntry] = []
    @Published var lastError: String?

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
    }
}

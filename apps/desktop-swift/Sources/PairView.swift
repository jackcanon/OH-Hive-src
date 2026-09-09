import SwiftUI

/// Named `PairView`, not `PairingView` -- `PairingView` is the FFI record type generated from
/// Rust's `PairingView` (code / url / expires_in_seconds); reusing that name for a SwiftUI
/// `View` would collide with it in the same module.
struct PairView: View {
    @EnvironmentObject private var store: HiveStore
    @Binding var isPresented: Bool
    @State private var pairing: PairingView?
    @State private var error: String?

    var body: some View {
        VStack(spacing: 16) {
            Text("Pair this machine").font(.title2.bold())
            if let pairing {
                Text(pairing.code)
                    .font(.system(size: 40, weight: .bold, design: .monospaced))
                Text("Enter this code at")
                Link(pairing.url, destination: URL(string: pairing.url) ?? URL(string: "https://ohghive.com/pair")!)
                Text("Expires in \(pairing.expiresInSeconds / 60) min")
                    .font(.caption).foregroundStyle(.secondary)
            } else if let error {
                Text(error).foregroundStyle(.red)
            } else {
                ProgressView("Requesting a pairing code\u{2026}")
            }
            Button("Cancel") {
                Task {
                    await store.cancelPairing()
                    isPresented = false
                }
            }
        }
        .padding(32)
        .frame(width: 360)
        .task {
            do {
                pairing = try await store.startPairing()
            } catch {
                self.error = String(describing: error)
            }
        }
        .onReceive(store.$snapshot) { snap in
            // Pairing clears on the Rust side once claimed (or expired) -- close automatically
            // as soon as this machine shows as paired.
            if snap?.paired == true { isPresented = false }
        }
    }
}

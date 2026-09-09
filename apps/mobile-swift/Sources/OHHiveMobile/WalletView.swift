import SwiftUI

struct WalletView: View {
    @State private var wallet: WalletInfo?
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        List {
            Section {
                if let wallet {
                    Text(String(format: "%.2f \u{1F36F}", wallet.balance)).font(.system(size: 40, weight: .bold))
                } else if loading {
                    ProgressView()
                }
                if let error { Text(error).font(.caption).foregroundStyle(.secondary) }
            }
            if let recent = wallet?.recent, !recent.isEmpty {
                Section("Recent") {
                    ForEach(recent) { entry in
                        HStack {
                            Text(entry.entryType)
                            Spacer()
                            Text(String(format: "%.2f \u{1F36F}", entry.amount)).foregroundStyle(entry.amount >= 0 ? .green : .red)
                        }
                        .font(.caption)
                    }
                }
            }
        }
        .navigationTitle("Wallet")
        .task { await load() }
        .refreshable { await load() }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            // NOTE: field names in `WalletInfo`/`WalletEntry` (Models.swift) are a best-effort guess
            // at `hive_my_wallet`'s shape -- verify against the actual RPC response and adjust the
            // CodingKeys if the app throws a decode error here on first run.
            wallet = try await supabase.rpc("hive_my_wallet", params: ["p_limit": 20]).execute().value
            error = nil
        } catch {
            self.error = "Couldn't load wallet (\(error.localizedDescription)). See the NOTE in WalletView.swift."
        }
    }
}

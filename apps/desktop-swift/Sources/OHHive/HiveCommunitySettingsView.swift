import SwiftUI

/// Community controls have one home. Private Fleet enrollment does not unlock this surface.
struct HiveCommunitySettingsView<TrustContent: View>: View {
    @ViewBuilder let trustContent: () -> TrustContent
    @State private var section: Section = .membership

    private enum Section: String, CaseIterable, Identifiable {
        case membership = "Membership"
        case node = "Node"
        case server = "Server"
        case earnings = "Earnings"
        var id: Self { self }
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("Hive settings", selection: $section) {
                ForEach(Section.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .padding()

            switch section {
            case .membership:
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Membership & Trust").font(.headline)
                        Text("This Mac is paired with the Hive. Community access is checked by the Hive when you connect.")
                            .font(.callout).foregroundStyle(.secondary)
                        trustContent()
                    }
                    .padding(20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            case .node: NodeView()
            case .server: ServerView()
            case .earnings: EarningsView()
            }
        }
    }
}

/// Connects existing invited members; never issues an invitation or grants membership.
struct HiveJoinSettingsView: View {
    @State private var showPairing = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Label("Connect to Hive", systemImage: "person.3").font(.title2)
                Text("Hives are private, invitation-only communities where members share projects and spare computing power.")
                Text("Already invited and have access? Pair this Mac with your existing Hive account. This does not request an invitation or create a membership. Your private workspace works without a Hive.")
                    .foregroundStyle(.secondary)
                Button("Pair with my Hive account") { showPairing = true }
                Text("Pairing does not start sharing this Mac’s computing power. You choose when to check in.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .sheet(isPresented: $showPairing) {
            PairView(isPresented: $showPairing)
        }
    }
}

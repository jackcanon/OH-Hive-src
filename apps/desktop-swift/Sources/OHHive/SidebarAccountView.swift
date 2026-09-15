import SwiftUI

/// Fixed sidebar footer. A node's display name is not its owner's account name.
struct SidebarAccountView: View {
    let summaryJSON: String?
    let paired: Bool
    let privateEnrolled: Bool
    let privateOwnerID: String?
    let loading: Bool

    private struct Summary: Decodable {
        let account: Account?
    }
    private struct Account: Decodable {
        let id: String
        let display_name: String?
        let email: String?
    }
    private var account: Account? {
        guard paired, let summaryJSON, let data = summaryJSON.data(using: .utf8),
              let account = try? JSONDecoder().decode(Summary.self, from: data).account else { return nil }
        // A separately enrolled private identity must never display another community account.
        guard !privateEnrolled || account.id.lowercased() == privateOwnerID?.lowercased() else { return nil }
        return account
    }
    private func nonempty(_ value: String?) -> String? {
        guard let text = value?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else { return nil }
        return text
    }
    private var title: String {
        if loading { return "Loading account…" }
        if let account { return nonempty(account.display_name) ?? nonempty(account.email) ?? "Hive account" }
        if privateEnrolled { return "Private Fleet account" }
        return paired ? "Hive account" : "Not connected"
    }
    private var subtitle: String {
        if let account { return nonempty(account.email) ?? "Connected to Hive" }
        if privateEnrolled { return "Verified on this Mac" }
        if paired { return "Account details unavailable" }
        return loading ? "" : "Sign in during setup"
    }
    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: paired || privateEnrolled ? "person.crop.circle.fill" : "person.crop.circle")
                .font(.system(size: 28))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.callout.weight(.medium)).lineLimit(1)
                if !subtitle.isEmpty {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.top, 12)
        .padding(.bottom, 6)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
        .help([title, subtitle].filter { !$0.isEmpty }.joined(separator: "\n"))
    }
}

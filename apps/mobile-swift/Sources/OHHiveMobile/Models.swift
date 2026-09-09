import Foundation

/// `hive_projects_overview` result (member-scoped -- includes `my_role`, unlike the Mac app's
/// node-scoped `hive_node_projects_overview`). Same shape the web app's `/projects` page decodes.
struct CloudProject: Decodable, Identifiable {
    let id: String
    let title: String
    let goal: String
    let licenseKind: String
    let ownerName: String?
    let myRole: String?
    let fundBalance: Double
    let cards: [String: Int]?

    enum CodingKeys: String, CodingKey {
        case id, title, goal, cards
        case licenseKind = "license_kind"
        case ownerName = "owner"
        case myRole = "my_role"
        case fundBalance = "fund_balance"
    }
}

/// `hive_my_wallet` result.
struct WalletInfo: Decodable {
    let balance: Double
    let recent: [WalletEntry]?
}
struct WalletEntry: Decodable, Identifiable {
    let id = UUID()
    let amount: Double
    let entryType: String
    let createdAt: String

    enum CodingKeys: String, CodingKey {
        case amount
        case entryType = "entry_type"
        case createdAt = "created_at"
    }
}

/// `hive_member_nodes` result (ADR-021 §4).
struct MemberNode: Decodable, Identifiable {
    let id: String
    let displayName: String
    let role: String
    let region: String
    let presence: String
    let lastHeartbeat: String?

    enum CodingKeys: String, CodingKey {
        case id, role, region, presence
        case displayName = "display_name"
        case lastHeartbeat = "last_heartbeat"
    }
}

/// `hive_project_comments_list` result.
struct ForumComment: Decodable, Identifiable {
    let id: String
    let authorName: String
    let body: String
    let createdAt: String
    let parentCommentId: String?

    enum CodingKeys: String, CodingKey {
        case id, body
        case authorName = "author_name"
        case createdAt = "created_at"
        case parentCommentId = "parent_comment_id"
    }
}

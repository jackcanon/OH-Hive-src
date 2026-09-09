import SwiftUI

private struct ProjectBoard: Decodable {
    struct Project: Decodable {
        let id: String
        let title: String
        let goal: String
        let fundBalance: Double
        enum CodingKeys: String, CodingKey { case id, title, goal; case fundBalance = "fund_balance" }
    }
    let project: Project?
}

struct ProjectDetailView: View {
    let projectId: String

    @State private var board: ProjectBoard?
    @State private var comments: [ForumComment] = []
    @State private var newComment = ""
    @State private var fundAmount = ""
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        List {
            if let project = board?.project {
                Section {
                    Text(project.title).font(.title3.weight(.semibold))
                    Text(project.goal).font(.subheadline).foregroundStyle(.secondary)
                    Text(String(format: "%.0f \u{1F36F} in fund", project.fundBalance)).font(.caption)
                }
            }
            Section("Add $honey to this project") {
                HStack {
                    TextField("Amount", text: $fundAmount).keyboardType(.decimalPad)
                    Button("Fund") { Task { await fund() } }
                        .disabled(busy || Double(fundAmount) == nil)
                }
            }
            Section("Forum") {
                ForEach(comments) { comment in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(comment.authorName).font(.caption.weight(.semibold))
                        Text(comment.body).font(.caption)
                    }
                }
                HStack {
                    TextField("Post to the forum\u{2026}", text: $newComment, axis: .vertical)
                    Button("Post") { Task { await postComment() } }.disabled(newComment.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            if let error { Text(error).font(.caption).foregroundStyle(.secondary) }
        }
        .navigationTitle(board?.project?.title ?? "Project")
        .task { await loadAll() }
        .refreshable { await loadAll() }
    }

    private func loadAll() async {
        async let boardTask: ProjectBoard? = try? await supabase.rpc("hive_project_board", params: ["p_project_id": projectId]).execute().value
        async let commentsTask: [ForumComment]? = try? await supabase.rpc("hive_project_comments_list", params: ["p_project_id": projectId]).execute().value
        board = await boardTask
        comments = await commentsTask ?? []
    }

    private func fund() async {
        guard let amount = Double(fundAmount) else { return }
        busy = true
        defer { busy = false }
        do {
            _ = try await supabase.rpc("hive_fund_project", params: [
                "p_project_id": projectId, "p_amount": amount, "p_anonymous": false,
            ] as [String: any Encodable & Sendable]).execute()
            fundAmount = ""
            await loadAll()
        } catch {
            self.error = "Couldn't fund the project (\(error.localizedDescription))."
        }
    }

    private func postComment() async {
        let body = newComment.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        newComment = ""
        do {
            _ = try await supabase.rpc("hive_project_comment_create", params: ["p_project_id": projectId, "p_body": body]).execute()
            await loadAll()
        } catch {
            self.error = "Couldn't post that (\(error.localizedDescription))."
        }
    }
}

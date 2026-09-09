import SwiftUI

private struct InterviewMessage: Identifiable, Codable {
    let id = UUID()
    let role: String // "user" | "assistant"
    let content: String
    enum CodingKeys: String, CodingKey { case role, content }
}

private struct EdgeReply: Decodable {
    let reply: String?
    let projectId: String?
    let cards: Int?
    let error: String?
    let detail: String?
    enum CodingKeys: String, CodingKey { case reply, error, detail, cards; case projectId = "project_id" }
}

/// Ported from `apps/web/app/new/page.tsx`'s `splitPlan` -- the interviewer embeds a JSON plan in a
/// fenced code block (or a trailing `{"schema_version":...}` object) once it has enough to propose
/// a project; everything before that is the human-readable summary to actually show.
private func splitPlan(_ content: String) -> (text: String, plan: [String: Any]?) {
    let patterns = ["```json\\s*([\\s\\S]*?)```", "(\\{[\\s\\S]*\"schema_version\"[\\s\\S]*\\})\\s*$"]
    for pattern in patterns {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]),
              let match = regex.firstMatch(in: content, range: NSRange(content.startIndex..., in: content)),
              let range = Range(match.range(at: 1), in: content) else { continue }
        let jsonText = String(content[range])
        guard let data = jsonText.data(using: .utf8),
              let plan = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
        let beforeRange = content.startIndex..<(Range(match.range, in: content)?.lowerBound ?? content.endIndex)
        let text = String(content[beforeRange]).replacingOccurrences(of: "PLAN\\s*$", with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return (text.isEmpty ? "Here's the plan." : text, plan)
    }
    return (content, nil)
}

/// v1 simplification vs. the web app: always uses the provider-backed `interview` Edge Function
/// (purchased/grant Honey), skipping the web app's local-text-pool fallback path
/// (`hive_interview_send`/`hive_interview_poll`, used when no cloud provider is available). That
/// dual routing is real logic worth porting later -- not done in this first pass, see
/// docs/IPHONE-APP-SCAFFOLD.md.
struct NewProjectView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var messages: [InterviewMessage] = []
    @State private var input = ""
    @State private var pending = false
    @State private var error: String?
    @State private var createdProjectId: String?

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if messages.isEmpty {
                        Text("Tell me about the project you want to start \u{2014} what it is, who it's for, what modality (text/code/image/video/audio).")
                            .font(.caption).foregroundStyle(.secondary).padding()
                    }
                    ForEach(messages) { m in
                        Text(m.content)
                            .padding(10)
                            .background(m.role == "user" ? Color.accentColor.opacity(0.2) : Color.secondary.opacity(0.1))
                            .clipShape(RoundedRectangle(cornerRadius: 10))
                            .frame(maxWidth: .infinity, alignment: m.role == "user" ? .trailing : .leading)
                    }
                    if pending { ProgressView().padding() }
                    if let error { Text(error).font(.caption).foregroundStyle(.red).padding(.horizontal) }
                    if let createdProjectId {
                        Text("Project created! (\(createdProjectId))").font(.caption).foregroundStyle(.green).padding(.horizontal)
                    }
                }
                .padding()
            }
            Divider()
            HStack {
                TextField("Message the interviewer\u{2026}", text: $input, axis: .vertical)
                Button("Send") { Task { await send() } }.disabled(input.trimmingCharacters(in: .whitespaces).isEmpty || pending)
            }
            .padding()
        }
        .navigationTitle("New Project")
        .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } } }
    }

    private func send() async {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        input = ""
        messages.append(InterviewMessage(role: "user", content: text))
        pending = true
        defer { pending = false }
        do {
            let reply: EdgeReply = try await supabase.functions.invoke("interview", options: .init(body: ["messages": messages]))
            if let err = reply.error {
                error = err + (reply.detail.map { ": \($0)" } ?? "")
                return
            }
            error = nil
            guard let content = reply.reply else { return }
            // The Edge Function path (used here) creates the project itself and returns
            // `project_id` directly when it does -- `splitPlan` still strips the raw JSON plan
            // block out of what's *shown*, but there's nothing further to call ourselves on this
            // path (that's only needed on the web app's local-session path, not built here).
            let (text, _) = splitPlan(content)
            messages.append(InterviewMessage(role: "assistant", content: text))
            if let projectId = reply.projectId {
                createdProjectId = projectId
            }
        } catch {
            self.error = "Couldn't reach the interviewer (\(error.localizedDescription))."
        }
    }
}

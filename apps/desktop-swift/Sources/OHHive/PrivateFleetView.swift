import SwiftUI
import OHHiveFFI

/// "Private Fleet" (2026-09-13, ADR-022 S2, #184) -- Swift catching up to the web app's #183.
/// Jack, 2026-09-13: "I want to be able to see the receipts, so I think we take Buzz's channel
/// model and run with it," then, mid-build: "call 'Fleet' 'Private Fleet' so that it's more
/// obvious that it's only your own machines, rather than another version of the Hive." Every
/// paired machine's own activity (came online, went offline, claimed/completed/failed a card)
/// posts here automatically, alongside anything the member types -- one fleet-wide record,
/// independent of any model's summarization.
///
/// v1 scope, matching the web app's first cut (this session's "web first" decision): the full
/// feed, no per-machine filter yet -- the web page's node-filter dropdown reuses `hive_my_wallet`
/// (a member-JWT RPC this Mac has no session for); giving Swift its own node list is a small
/// fast-follow, not blocking this view from shipping. Polls like `ChatEngine`'s memory fetch and
/// `KanbanView`'s cloud column: no push/websocket layer in this app.
struct PrivateFleetView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var posts: [ChannelPost] = []
    @State private var draft = ""
    @State private var busy = false
    @State private var posting = false
    @State private var error: String?
    @State private var pollTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Everything your own paired machines are doing, and anything you want to tell them -- the receipts for your Personal Hive. Only you can see this; it's not the community Hive.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let error { fleetNoteBox(error) }

            HStack {
                TextField("Post a message to your private fleet…", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .disabled(posting)
                    .onSubmit { Task { await post() } }
                Button("Post") { Task { await post() } }
                    .disabled(posting || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if posting { ProgressView().controlSize(.small) }
            }

            if busy && posts.isEmpty {
                ProgressView().controlSize(.small)
            } else if posts.isEmpty {
                Text("Nothing here yet -- pair a machine and put it to work, or post a message above.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(posts, id: \.id) { p in
                        VStack(alignment: .leading, spacing: 3) {
                            HStack {
                                Text(label(for: p)).font(.caption).bold()
                                if p.authorKind == "node" {
                                    Text("· \(p.eventType.replacingOccurrences(of: "_", with: " "))")
                                        .font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(p.createdAt).font(.caption2).foregroundStyle(.secondary)
                            }
                            Text(p.body).font(.callout)
                        }
                        .padding(8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.secondary.opacity(0.06))
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                    }
                }
            }
        }
        .padding()
        .navigationTitle("Private Fleet")
        .onAppear { start() }
        .onDisappear { pollTask?.cancel() }
    }

    private func label(for p: ChannelPost) -> String {
        switch p.authorKind {
        case "member": return "You"
        case "assistant": return "Assistant"
        default: return p.nodeDisplay ?? "A machine"
        }
    }

    private func start() {
        guard pollTask == nil else { return }
        pollTask = Task {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
    }

    private func load() async {
        do {
            let fresh = try await store.channelList()
            posts = fresh
            error = nil
        } catch {
            self.error = "Couldn't load your Private Fleet channel: \(error.localizedDescription)"
        }
        busy = false
    }

    private func post() async {
        let body = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }
        posting = true
        defer { posting = false }
        do {
            _ = try await store.channelPost(body)
            draft = ""
            await load()
        } catch {
            self.error = "Couldn't post that: \(error.localizedDescription)"
        }
    }
}

@ViewBuilder
private func fleetNoteBox(_ text: String) -> some View {
    Text(text)
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.secondary.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 6))
}

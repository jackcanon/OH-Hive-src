import Foundation
import Observation
import CryptoKit
import Darwin

struct TeamMemberDraft: Codable, Identifiable, Equatable {
    var id = UUID()
    var name: String
    var bio: String
    var instructions: String
    var avatar: String
    var included = true
    var agentID: String?
    var creationPending = false
    var complete = false

    init(template: AgentRoleTemplate) {
        name = template.name; bio = template.bio
        instructions = template.instructions + "\n" + template.limitation
        avatar = template.avatar
    }
    var valid: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 200
            && bio.utf8.count <= 4000 && instructions.utf8.count <= 16000
    }
    func freshCopy() -> Self {
        var copy = self
        copy.id = UUID(); copy.agentID = nil; copy.creationPending = false; copy.complete = false
        return copy
    }
}

struct TeamRecipe: Codable, Identifiable {
    var id = UUID()
    var name: String
    var members: [TeamMemberDraft]
}

/// A recipe contains no credentials, resource grants, model identifiers or machine bindings.
/// Progress is saved BEFORE a create request. An ambiguous response is held for review,
/// never retried as another creation request.
@MainActor @Observable
final class TeamStarter {
    var members: [TeamMemberDraft] = []
    var recipes: [TeamRecipe] = []
    var error: String?
    private(set) var busy = false
    private(set) var progress = ""
    private let progressURL: URL
    private let recipesURL: URL
    private var loadFailed = false
    var started: Bool { members.contains { $0.agentID != nil || $0.creationPending } }
    var canCreate: Bool {
        !loadFailed && !busy && members.contains { $0.included && !$0.complete }
            && members.filter(\.included).allSatisfy { $0.valid && !($0.creationPending && $0.agentID == nil) }
    }

    init(context: String, directory: URL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("ohhive/team-starters", isDirectory: true)) {
        let key = SHA256.hash(data: Data(context.utf8)).map { String(format: "%02x", $0) }.joined()
        progressURL = directory.appendingPathComponent("progress-" + key + ".json")
        recipesURL = directory.appendingPathComponent("recipes.json")
        do {
            if FileManager.default.fileExists(atPath: progressURL.path) {
                let data = try Data(contentsOf: progressURL)
                members = try JSONDecoder().decode([TeamMemberDraft].self, from: data)
            }
            if FileManager.default.fileExists(atPath: recipesURL.path) {
                let data = try Data(contentsOf: recipesURL)
                recipes = try JSONDecoder().decode([TeamRecipe].self, from: data)
            }
        } catch {
            loadFailed = true
            self.error = "Saved team setup could not be read. Your saved data has been kept; do not create the team again until it has been reviewed."
        }
        if members.contains(where: { $0.creationPending && $0.agentID == nil }) {
            error = "An earlier creation has an unknown result. Check the agent list before starting another team. This setup will not retry that request."
        }
    }
    func choose(_ templates: [AgentRoleTemplate], included: Bool = true) {
        guard !started && !busy && !loadFailed else { return }
        members = templates.map { var member = TeamMemberDraft(template: $0); member.included = included; return member }
        error = nil
    }
    func use(_ recipe: TeamRecipe) {
        guard !started && !busy && !loadFailed else { return }
        members = recipe.members.map { $0.freshCopy() }
    }
    func saveRecipe(name: String) throws {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty && name.utf8.count <= 200,
              members.contains(where: \.included), members.filter(\.included).allSatisfy(\.valid) else {
            throw TeamStarterError("Choose agents and enter a preset name of up to 200 bytes.")
        }
        let saved = recipes + [TeamRecipe(name: name, members: members.filter(\.included).map { $0.freshCopy() })]
        try write(try JSONEncoder().encode(saved), to: recipesURL)
        recipes = saved
    }
    func newTeam() {
        guard !busy && !loadFailed && members.filter(\.included).allSatisfy(\.complete) else { return }
        do {
            if FileManager.default.fileExists(atPath: progressURL.path) { try FileManager.default.removeItem(at: progressURL) }
            members = []; progress = ""
        } catch { self.error = error.localizedDescription }
    }
    private func write(_ data: Data, to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try data.write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }
    private func persist() throws { try write(try JSONEncoder().encode(members), to: progressURL) }
    func create(createAgent: (String) async throws -> String,
                saveProfile: (String, TeamMemberDraft) async throws -> Void) async {
        guard canCreate else { return }
        let lockFD: Int32
        do {
            try FileManager.default.createDirectory(at: progressURL.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            lockFD = Darwin.open(progressURL.appendingPathExtension("lock").path, O_CREAT | O_RDWR, 0o600)
            guard lockFD >= 0 else { throw TeamStarterError("Could not lock team setup.") }
            guard flock(lockFD, LOCK_EX | LOCK_NB) == 0 else {
                Darwin.close(lockFD)
                throw TeamStarterError("This team is already being created in another window. Wait for it to finish.")
            }
        } catch { self.error = error.localizedDescription; return }
        defer { flock(lockFD, LOCK_UN); Darwin.close(lockFD) }
        do {
            // Another window may have completed work since this sheet opened.
            if FileManager.default.fileExists(atPath: progressURL.path) {
                members = try JSONDecoder().decode([TeamMemberDraft].self, from: Data(contentsOf: progressURL))
            }
            guard canCreate else {
                error = "Saved setup has completed or needs review. Check the agent list before creating more agents."
                return
            }
        } catch { self.error = "Saved progress could not be read. No agents were created."; return }
        busy = true; error = nil
        defer { busy = false }
        do {
            for index in members.indices where members[index].included && !members[index].complete {
                progress = "Setting up \(members[index].name)…"
                if members[index].agentID == nil {
                    members[index].creationPending = true
                    try persist()
                    let id = try await createAgent(members[index].name)
                    members[index].agentID = id
                    members[index].creationPending = false
                    try persist()
                }
                guard let id = members[index].agentID else { continue }
                try await saveProfile(id, members[index])
                members[index].complete = true
                try persist()
            }
            progress = "Your agents are ready to customize in their profiles. Connect tools separately when needed."
        } catch {
            self.error = members.contains(where: { $0.creationPending && $0.agentID == nil })
                ? "Creation could not be confirmed. Check the agent list; this setup will not repeat the request and risk a duplicate. Details: \(error.localizedDescription)"
                : "Setup stopped. Completed agents are kept. Continue to finish the remaining profiles. Details: \(error.localizedDescription)"
        }
    }
}

struct TeamStarterError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

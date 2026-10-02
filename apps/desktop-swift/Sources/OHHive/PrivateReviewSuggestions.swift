import OHHiveFFI

/// Suggestions only: readiness metadata never grants execution permission.
enum PrivateReviewSuggestions {
    struct Selection {
        var host = ""
        var agent = ""
        var model = ""
    }
    static func agents(on host: String, excluding coder: String?, from agents: [BotsAgent]) -> [BotsAgent] {
        agents.filter { $0.preferredHost == host && !$0.archived && $0.runtimeKind == "local" && $0.id != coder }
    }
    static func onHost(_ host: PrivateCodingHost, coder: String?, model: String?, agents: [BotsAgent]) -> Selection {
        guard host.fresh && host.workerEnabled && host.codingEnabled else { return Selection() }
        let reviewers = self.agents(on: host.nodeId, excluding: coder, from: agents)
        let models = host.models.filter { $0.supportsTools != false }
        let chosenModel = models.first { $0.id == model }?.id ?? (models.count == 1 ? models[0].id : "")
        return Selection(host: host.nodeId, agent: reviewers.count == 1 ? reviewers[0].id : "", model: chosenModel)
    }
    static func suggest(coder: String?, model: String?, hosts: [PrivateCodingHost], agents: [BotsAgent]) -> Selection {
        let preferredHost = agents.first { $0.id == coder }?.preferredHost
        let candidates = hosts.filter {
            $0.fresh && $0.workerEnabled && $0.codingEnabled &&
            !self.agents(on: $0.nodeId, excluding: coder, from: agents).isEmpty &&
            $0.models.contains { $0.supportsTools != false }
        }
        let host = candidates.first { $0.nodeId == preferredHost } ?? (candidates.count == 1 ? candidates[0] : nil)
        guard let host else { return Selection() }
        return onHost(host, coder: coder, model: model, agents: agents)
    }
}

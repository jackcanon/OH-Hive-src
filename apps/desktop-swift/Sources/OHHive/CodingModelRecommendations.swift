/// Limited calibration recommendations, independent of any particular computer or agent.
enum CodingModelRecommendations {
    static func reasoningOff(model: String, savedThinking: Bool? = nil) -> Bool {
        if let savedThinking { return !savedThinking }
        return model == "qwen3.6:35b-a3b"
    }
}

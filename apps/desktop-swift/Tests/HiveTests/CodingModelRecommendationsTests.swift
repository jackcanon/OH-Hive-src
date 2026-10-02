import XCTest
@testable import Hive

final class CodingModelRecommendationsTests: XCTestCase {
    func testCalibratedModelAndUnknownModels() {
        XCTAssertTrue(CodingModelRecommendations.reasoningOff(model: "qwen3.6:35b-a3b"))
        for model in ["", "qwen3.6:27b", "qwen3.8:27b", "custom/qwen3.6:35b-a3b"] {
            XCTAssertFalse(CodingModelRecommendations.reasoningOff(model: model))
        }
    }

    func testSavedOwnerChoiceTakesPrecedenceOverRecommendation() {
        XCTAssertFalse(CodingModelRecommendations.reasoningOff(model: "qwen3.6:35b-a3b", savedThinking: true))
        XCTAssertTrue(CodingModelRecommendations.reasoningOff(model: "other", savedThinking: false))
    }
}

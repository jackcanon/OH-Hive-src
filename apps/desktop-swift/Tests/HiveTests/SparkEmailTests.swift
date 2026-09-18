import XCTest
@testable import Hive
final class SparkEmailTests: XCTestCase {
    func testIDsCannotInjectFlags() throws {
        XCTAssertEqual(try SparkEmailCommands.id("123"), "123")
        for id in ["", "--send", "1 2", "1; echo bad", "１２", "../file"] {
            XCTAssertThrowsError(try SparkEmailCommands.id(id))
        }
    }
    func testDraftIsOnlyDraftAndBodyStaysOneArgument() throws {
        let body = "Hello\n$(touch /tmp/not-run) --user other@example.com"
        let args = try SparkEmailCommands.draft(account: "me@example.com", to: "you@example.com", subject: "Hello", body: body)
        XCTAssertEqual(args.first, "draft")
        XCTAssertEqual(args.last, body)
        XCTAssertFalse(args.contains("--user"))
        XCTAssertFalse(args.contains("send"))
        XCTAssertThrowsError(try SparkEmailCommands.draft(account: "me@example.com", to: "you@example.com\nBcc:x@y.com", subject: "Hello", body: "Hi"))
    }
    func testActionAllowlistExcludesSendDeleteAndSharing() {
        for action in ["send", "moveToTrash", "shareInTeam", "unsubscribe"] {
            XCTAssertThrowsError(try SparkEmailCommands.organize(action, message: "123"))
        }
        XCTAssertEqual(try SparkEmailCommands.organize("archive", message: "123"), ["action", "archive", "123"])
    }
    @MainActor
    func testEmailCapabilitiesDefaultOffAndPersistIndependently() {
        let name = "SparkEmailTests-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let c = SparkEmailConnector(defaults: defaults)
        XCTAssertFalse(c.readEnabled); XCTAssertFalse(c.sendEnabled)
        c.readEnabled = true
        let restored = SparkEmailConnector(defaults: defaults)
        XCTAssertTrue(restored.readEnabled); XCTAssertFalse(restored.draftEnabled)
        XCTAssertFalse(restored.organizeEnabled); XCTAssertFalse(restored.sendEnabled)
    }
    @MainActor
    func testChangedDraftAndDisabledSendingNeverSend() async {
        let name = "SparkEmailReview-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        var calls: [[String]] = []
        let c = SparkEmailConnector(defaults: defaults) { args in
            calls.append(args)
            return "Edited draft"
        }
        c.readEnabled = true; c.sendEnabled = true
        await c.sendReviewedDraft("123", expected: "Original draft")
        XCTAssertEqual(calls, [["thread", "123"]])
        XCTAssertTrue(c.status.contains("changed"))
        calls = []; c.sendEnabled = false
        await c.sendReviewedDraft("123", expected: "Edited draft")
        XCTAssertTrue(calls.isEmpty)
    }
}

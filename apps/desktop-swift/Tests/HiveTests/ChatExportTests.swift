import XCTest
@testable import Hive

final class ChatExportTests: XCTestCase {
    func testExportsEveryMessageInOrderAndKeepsSnapshot() {
        var messages = (0..<24).map { ChatMessage(role: $0.isMultiple(of: 2) ? .user : .assistant, text: "Message \($0)\nCafé 🐝") }
        messages.insert(ChatMessage(role: .system, text: "System context"), at: 0)
        let snapshot = ChatExportSnapshot(title: "Project notes", messages: messages)
        let sections = snapshot.markdown.components(separatedBy: "\n\n---\n\n")
        XCTAssertEqual(snapshot.filename, "Project notes.md")
        XCTAssertEqual(sections.count, 25)
        XCTAssertTrue(sections[0].hasPrefix("## System\n\nSystem context"))
        for index in 0..<24 {
            let role = index.isMultiple(of: 2) ? "User" : "Assistant"
            XCTAssertTrue(sections[index + 1].hasPrefix("## \(role)\n\nMessage \(index)\nCafé 🐝"))
        }
        messages.removeAll()
        XCTAssertTrue(snapshot.markdown.contains("Message 23"))
    }
    func testBlankTitleFallback() {
        XCTAssertEqual(ChatExportSnapshot(title: " \n", messages: []).filename, "Chat.md")
    }
}

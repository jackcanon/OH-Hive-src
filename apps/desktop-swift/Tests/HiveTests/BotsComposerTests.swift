import AppKit
import XCTest
@testable import Hive

@MainActor
final class BotsComposerTests: XCTestCase {
    private func enter(_ flags: NSEvent.ModifierFlags = [], repeatKey: Bool = false, key: UInt16 = 36) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: flags,
            timestamp: 0, windowNumber: 0, context: nil, characters: "\r",
            charactersIgnoringModifiers: "\r", isARepeat: repeatKey, keyCode: key)!
    }
    func testEnterSubmitsWithoutChangingDraft() {
        let editor = BotsMessageTextView()
        editor.string = "Hello"
        editor.canSubmit = true
        var sends = 0
        editor.submit = { sends += 1 }
        editor.keyDown(with: enter())
        XCTAssertEqual(sends, 1)
        XCTAssertEqual(editor.string, "Hello")
        editor.keyDown(with: enter(repeatKey: true))
        XCTAssertEqual(sends, 1)
    }
    func testDisabledEnterPreservesDraftAndDoesNotSend() {
        let editor = BotsMessageTextView()
        editor.string = "Keep this draft"
        editor.submit = { XCTFail("Disabled composer must not send") }
        editor.keyDown(with: enter())
        XCTAssertEqual(editor.string, "Keep this draft")
    }
    func testShiftEnterInsertsNewline() {
        let editor = BotsMessageTextView()
        editor.string = "Hello"
        editor.setSelectedRange(NSRange(location: 5, length: 0))
        editor.canSubmit = true
        editor.submit = { XCTFail("Shift-Return must not send") }
        editor.keyDown(with: enter(.shift))
        XCTAssertEqual(editor.string, "Hello\n")
    }
    func testKeypadEnterSubmits() {
        let editor = BotsMessageTextView()
        editor.canSubmit = true
        var sent = false
        editor.submit = { sent = true }
        editor.keyDown(with: enter(key: 76))
        XCTAssertTrue(sent)
    }
}

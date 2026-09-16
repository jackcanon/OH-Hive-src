import XCTest
@testable import Hive

final class GoogleConnectorTests: XCTestCase {
    @MainActor
    func testRealLoopbackListenerStarts() async throws {
        let (port, listener) = try await GoogleAuthManager.startLoopbackListener()
        defer { listener.cancel() }
        XCTAssertGreaterThan(port, 0)
    }

    func testRejectsInjectedOrMultipleRecipients() {
        for address in ["user@example.com\r\nBcc: victim@example.com", "a@example.com,b@example.com", "", "a@", "Name <a@example.com>"] {
            XCTAssertThrowsError(try GoogleEmail.message(to: address, subject: "Hi", body: "Hello"))
        }
    }
    func testSubjectInjectionAndUnicodeBody() throws {
        let message = try GoogleEmail.message(to: "user@example.com", subject: "Hello\r\nBcc: victim@example.com", body: "Café 🐝")
        XCTAssertFalse(message.contains("Bcc:"))
        XCTAssertTrue(message.contains("Subject: =?UTF-8?B?SGVsbG8=?="))
        let body = try XCTUnwrap(message.components(separatedBy: "\r\n\r\n").last)
        XCTAssertEqual(String(data: try XCTUnwrap(Data(base64Encoded: body)), encoding: .utf8), "Café 🐝")
    }
    func testCredentialWriteRequiresSuccessAndReadback() throws {
        XCTAssertThrowsError(try GoogleKeychain.verifyWrite(status: -25293, stored: "token", expected: "token"))
        XCTAssertThrowsError(try GoogleKeychain.verifyWrite(status: 0, stored: nil, expected: "token"))
        XCTAssertThrowsError(try GoogleKeychain.verifyWrite(status: 0, stored: "old", expected: "token"))
        XCTAssertNoThrow(try GoogleKeychain.verifyWrite(status: 0, stored: "token", expected: "token"))
    }
    func testSharedClientConfiguration() {
        XCTAssertEqual(SharedConnectorConfiguration.validatedGoogleClientID(nil), "")
        XCTAssertEqual(SharedConnectorConfiguration.validatedGoogleClientID("user-entered-secret"), "")
        XCTAssertEqual(SharedConnectorConfiguration.validatedGoogleClientID("123-test.apps.googleusercontent.com"), "123-test.apps.googleusercontent.com")
    }
    func testBase64URL() {
        XCTAssertEqual(Data([251, 255]).base64URLEncodedString(), "-_8")
    }
}

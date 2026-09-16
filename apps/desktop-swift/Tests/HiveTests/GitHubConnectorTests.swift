import XCTest
@testable import Hive

final class GitHubConnectorTests: XCTestCase {
    func testCallbackRejectsWrongRouteAndDuplicateState() {
        XCTAssertEqual(GitHubCallback.parse("GET /oauth2callback?state=nonce&code=test HTTP/1.1\r\n")?["state"], "nonce")
        for request in ["GET /?state=n&code=x HTTP/1.1", "POST /oauth2callback?state=n HTTP/1.1", "GET /oauth2callback?state=n&state=x HTTP/1.1", "GET /oauth2callback?code=x HTTP/1.1"] {
            XCTAssertNil(GitHubCallback.parse(request))
        }
    }
    func testTokenLifetimes() throws {
        let decoder = JSONDecoder()
        let old = try decoder.decode(GitHubToken.self, from: Data(#"{"access_token":"test","token_type":"bearer"}"#.utf8))
        XCTAssertNil(old.expires_in)
        let expiring = try decoder.decode(GitHubToken.self, from: Data(#"{"access_token":"test","token_type":"bearer","expires_in":28800,"refresh_token":"refresh"}"#.utf8))
        XCTAssertEqual(expiring.expires_in, 28800)
        XCTAssertEqual(expiring.refresh_token, "refresh")
    }
    func testRepositoryLinkValidation() {
        for url in ["javascript:alert(1)", "https://github.com.evil.test/repo", "https://user@github.com/repo", "http://github.com/repo"] {
            XCTAssertNil(GitHubRepository(id: 1, full_name: "repo", html_url: url).safeURL)
        }
        XCTAssertNotNil(GitHubRepository(id: 1, full_name: "owner/repo", html_url: "https://github.com/owner/repo").safeURL)
    }
    func testCredentialReadback() {
        XCTAssertThrowsError(try GitHubKeychain.verifyWrite(status: 0, stored: nil, expected: "test"))
        XCTAssertThrowsError(try GitHubKeychain.verifyWrite(status: -1, stored: "test", expected: "test"))
        XCTAssertNoThrow(try GitHubKeychain.verifyWrite(status: 0, stored: "test", expected: "test"))
    }
}

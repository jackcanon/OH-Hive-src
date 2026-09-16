import XCTest
@testable import Hive

final class GitHubConnectorTests: XCTestCase {
    func testDevicePollingHonorsPendingAndSlowDown() async throws {
        var time = Date(timeIntervalSince1970: 0)
        var delays: [Double] = []
        var responses = [#"{"error":"authorization_pending"}"#, #"{"error":"slow_down","interval":12}"#, #"{"access_token":"ok","token_type":"bearer"}"#]
        let device = GitHubDeviceCode(device_code: "device", user_code: "code", verification_uri: "https://github.com/login/device", expires_in: 100, interval: 5)
        let token = try await GitHubDeviceFlow.poll(device: device, now: { time }, sleep: { delay in
            delays.append(delay); time.addTimeInterval(delay)
        }, request: { Data(responses.removeFirst().utf8) })
        XCTAssertEqual(token.access_token, "ok")
        XCTAssertEqual(delays, [5, 5, 12])
    }
    func testDeviceExpiryStopsBeforeRequest() async {
        var time = Date(timeIntervalSince1970: 0)
        let device = GitHubDeviceCode(device_code: "d", user_code: "c", verification_uri: "https://github.com/login/device", expires_in: 2, interval: 5)
        do {
            _ = try await GitHubDeviceFlow.poll(device: device, now: { time }, sleep: { time.addTimeInterval($0) }, request: {
                XCTFail("Expired code must not be polled"); return Data()
            })
            XCTFail("Expected expiry")
        } catch { XCTAssertTrue(error is GitHubConnectorError) }
    }
    func testDeviceDenialAndCancellation() async {
        let device = GitHubDeviceCode(device_code: "d", user_code: "c", verification_uri: "https://github.com/login/device", expires_in: 900, interval: 5)
        for response in ["access_denied", "expired_token", "incorrect_client_credentials"] {
            do {
                _ = try await GitHubDeviceFlow.poll(device: device, sleep: { _ in }, request: { Data("{\"error\":\"\(response)\"}".utf8) })
                XCTFail("Expected terminal failure")
            } catch { XCTAssertTrue(error is GitHubConnectorError) }
        }
        do {
            _ = try await GitHubDeviceFlow.poll(device: device, sleep: { _ in throw CancellationError() }, request: { XCTFail("Cancelled"); return Data() })
            XCTFail("Expected cancellation")
        } catch { XCTAssertTrue(error is CancellationError) }
    }
    func testSessionBindingRejectsLegacyAndOtherApps() throws {
        let session = GitHubSession(accessToken: "test", refreshToken: "refresh", expiresAt: nil, oauthClientID: "app")
        let raw = String(decoding: try JSONEncoder().encode(session), as: UTF8.self)
        XCTAssertNotNil(GitHubSession.read(raw, clientID: "app"))
        XCTAssertNil(GitHubSession.read(raw, clientID: "other"))
        XCTAssertNil(GitHubSession.read(raw, clientID: ""))
        XCTAssertNil(GitHubSession.read(#"{"accessToken":"legacy"}"#, clientID: "app"))
        XCTAssertEqual(SharedConnectorConfiguration.validatedGitHubClientID("Iv23.example"), "Iv23.example")
        XCTAssertEqual(SharedConnectorConfiguration.validatedGitHubClientID("bad id"), "")
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

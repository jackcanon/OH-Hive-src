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
    func testInstallationRepositoriesIncludePrivateAndDeduplicatePublic() async throws {
        var requested: [String] = []
        let repos = try await GitHubRepositoryLoader.load { url in
            requested.append(url)
            if url.contains("/user/repos?") {
                return Data(#"[{"id":1,"full_name":"owner/public","html_url":"https://github.com/owner/public","private":false}]"#.utf8)
            }
            if url.contains("/user/installations?") {
                return Data(#"{"total_count":1,"installations":[{"id":42}]}"#.utf8)
            }
            if url.hasSuffix("page=1") {
                return Data(#"{"total_count":2,"repositories":[{"id":1,"full_name":"owner/public","html_url":"https://github.com/owner/public","private":false}]}"#.utf8)
            }
            return Data(#"{"total_count":2,"repositories":[{"id":2,"full_name":"owner/private","html_url":"https://github.com/owner/private","private":true}]}"#.utf8)
        }
        XCTAssertEqual(repos.count, 2)
        XCTAssertEqual(repos.first(where: { $0.id == 2 })?.private, true)
        XCTAssertTrue(requested.contains("https://api.github.com/user/installations/42/repositories?per_page=100&page=2"))
    }
    /// A user with more than one page of public repositories used to lose everything past the
    /// first 100, silently. The installation half of this loader has always paginated; this is the
    /// public half catching up.
    func testPublicRepositoriesPastTheFirstPageAreNotLost() async throws {
        var requested: [String] = []
        let repos = try await GitHubRepositoryLoader.load { url in
            requested.append(url)
            if url.contains("/user/repos?") {
                if url.hasSuffix("page=1") {
                    let full = (1...100).map {
                        #"{"id":\#($0),"full_name":"owner/r\#($0)","html_url":"https://github.com/owner/r\#($0)","private":false}"#
                    }.joined(separator: ",")
                    return Data("[\(full)]".utf8)
                }
                if url.hasSuffix("page=2") {
                    return Data(#"[{"id":101,"full_name":"owner/last","html_url":"https://github.com/owner/last","private":false}]"#.utf8)
                }
                return Data("[]".utf8)
            }
            return Data(#"{"total_count":0,"installations":[]}"#.utf8)
        }
        XCTAssertEqual(repos.count, 101, "the 101st repository is the whole point")
        XCTAssertTrue(repos.contains { $0.id == 101 })
        XCTAssertTrue(requested.contains("https://api.github.com/user/repos?visibility=public&per_page=100&sort=updated&page=2"))
        XCTAssertFalse(
            requested.contains("https://api.github.com/user/repos?visibility=public&per_page=100&sort=updated&page=3"),
            "a short page is the last page; do not keep asking"
        )
    }

    func testNoInstallationRetainsPublicListing() async throws {
        let repos = try await GitHubRepositoryLoader.load { url in
            Data((url.contains("/user/repos?") ? "[]" : #"{"total_count":0,"installations":[]}"#).utf8)
        }
        XCTAssertTrue(repos.isEmpty)
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

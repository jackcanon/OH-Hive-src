import XCTest
import Security
@testable import Hive

final class ConnectorStartupTests: XCTestCase {
    func testStartupCredentialProbesRefuseInteractiveApproval() {
        for query in [GitHubKeychain.readQuery("session", allowInteraction: false),
                      GoogleKeychain.readQuery("refreshToken", allowInteraction: false)] {
            XCTAssertEqual(query[kSecUseAuthenticationUI as String] as? String, kSecUseAuthenticationUIFail as String)
            XCTAssertEqual(query[kSecReturnData as String] as? Bool, true)
        }
    }

    func testExplicitConnectorAccessRetainsNormalSystemApproval() {
        XCTAssertNil(GitHubKeychain.readQuery("session", allowInteraction: true)[kSecUseAuthenticationUI as String])
        XCTAssertNil(GoogleKeychain.readQuery("refreshToken", allowInteraction: true)[kSecUseAuthenticationUI as String])
    }
}

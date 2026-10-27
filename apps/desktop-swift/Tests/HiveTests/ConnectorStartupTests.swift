import XCTest
import Security
@testable import Hive

final class ConnectorStartupTests: XCTestCase {
    func testLegacyNoInteractionGuardRestoresPriorStateAfterRefusedRead() {
        var before = DarwinBoolean(false)
        XCTAssertEqual(SecKeychainGetUserInteractionAllowed(&before), errSecSuccess)
        let result: String? = KeychainReadAccess.perform(allowInteraction: false) {
            var during = DarwinBoolean(true)
            XCTAssertEqual(SecKeychainGetUserInteractionAllowed(&during), errSecSuccess)
            XCTAssertFalse(during.boolValue)
            return nil
        }
        XCTAssertNil(result)
        var after = DarwinBoolean(false)
        XCTAssertEqual(SecKeychainGetUserInteractionAllowed(&after), errSecSuccess)
        XCTAssertEqual(before.boolValue, after.boolValue)
    }
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

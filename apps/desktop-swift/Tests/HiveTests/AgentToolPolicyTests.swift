import XCTest
@testable import Hive

final class AgentToolPolicyTests: XCTestCase {
    func testOlderProfilePreservesUnspecifiedNewGrants() throws {
        let data = Data(#"{"revision":2,"template":"researcher-v1","readable_vaults":[]}"#.utf8)
        let policy = try JSONDecoder().decode(AgentToolPolicy.self, from: data)
        XCTAssertNil(policy.writableVaults)
        XCTAssertNil(policy.webHosts)
        XCTAssertNil(policy.handoffTargets)
        let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(policy)) as! [String: Any]
        XCTAssertNil(encoded["writable_vaults"])
        XCTAssertNil(encoded["web_hosts"])
        // Unknown server write permissions are not sent back as empty lists by an older draft.
        XCTAssertNil(encoded["web_post_hosts"])
    }

    func testExplicitEmptySelectionsAreSentToRevokeGrants() throws {
        var policy = AgentToolPolicy()
        policy.writableVaults = []
        policy.webHosts = []
        policy.handoffTargets = []
        let encoded = try JSONSerialization.jsonObject(with: JSONEncoder().encode(policy)) as! [String: Any]
        XCTAssertEqual((encoded["writable_vaults"] as? [String])?.count, 0)
        XCTAssertEqual((encoded["web_hosts"] as? [String])?.count, 0)
        XCTAssertEqual((encoded["handoff_targets"] as? [String])?.count, 0)
    }
}

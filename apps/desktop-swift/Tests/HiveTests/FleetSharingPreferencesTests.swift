import XCTest
@testable import Hive
final class FleetSharingPreferencesTests: XCTestCase {
    func testSharingIntentSurvivesNewInstanceAndStopClearsIt() {
        let suite = "fleet-sharing-test-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let first = FleetSharingPreferences(defaults: defaults)
        XCTAssertNil(first.address)
        first.remember(address: "192.168.1.7:8787")
        let relaunched = FleetSharingPreferences(defaults: defaults)
        XCTAssertEqual(relaunched.address, "192.168.1.7:8787")
        relaunched.stop()
        XCTAssertNil(first.address)
    }
}

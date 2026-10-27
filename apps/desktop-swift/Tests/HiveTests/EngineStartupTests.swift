import XCTest
import OHHiveFFI

final class EngineStartupTests: XCTestCase {
    func testGeneratedBindingsInitializeTheLoadedEngine() {
        // Construction checks every UniFFI API checksum. Presentation-only tests
        // can pass while the real app immediately traps on a mismatched binding.
        XCTAssertFalse(HiveNode().about().coreVersion.isEmpty)
    }
}

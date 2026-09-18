import XCTest
@testable import Hive

final class PrivateFleetDiscoveryTests: XCTestCase {
    func testPrivateNetworkBoundaries() {
        for value in ["10.0.0.1", "172.16.0.1", "172.31.255.254", "192.168.1.1", "169.254.1.2"] {
            XCTAssertTrue(FleetNetwork.isPrivateIPv4(value), value)
        }
        for value in ["8.8.8.8", "127.0.0.1", "0.0.0.0", "172.15.0.1", "172.32.0.1", "192.169.1.1", "10.0.0.256", "10.0.1", "010.0.0.1", "10.0.0.1/path", "::1"] {
            XCTAssertFalse(FleetNetwork.isPrivateIPv4(value), value)
        }
    }
    func testDiscoveryCannotRedirectToUnresolvedOrPublicAddressOrSelf() {
        let privateIP = "192.168.1.12"
        XCTAssertEqual(FleetNetwork.discoveredEndpoint(address: privateIP, port: 8787, resolved: [privateIP], local: []), "http://192.168.1.12:8787")
        XCTAssertNil(FleetNetwork.discoveredEndpoint(address: privateIP, port: 8787, resolved: ["192.168.1.13"], local: []))
        XCTAssertNil(FleetNetwork.discoveredEndpoint(address: "8.8.8.8", port: 8787, resolved: ["8.8.8.8"], local: []))
        XCTAssertNil(FleetNetwork.discoveredEndpoint(address: privateIP, port: 8787, resolved: [privateIP], local: [privateIP]))
        for port in [0, -1, 65536] {
            XCTAssertNil(FleetNetwork.discoveredEndpoint(address: privateIP, port: port, resolved: [privateIP], local: []))
        }
    }
    func testBrowserReturnRejectsWrongStateDuplicatesAndWrongPath() {
        let approval = Data("signed-approval-placeholder".utf8).base64EncodedString()
        func request(_ query: String, path: String = "/fleet-enrollment") -> String {
            "GET \(path)?\(query) HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        }
        XCTAssertEqual(FleetSignInCallback.approval(request("state=expected&approval=\(approval)"), state: "expected"), "signed-approval-placeholder")
        XCTAssertNil(FleetSignInCallback.approval(request("state=other&approval=\(approval)"), state: "expected"))
        XCTAssertNil(FleetSignInCallback.approval(request("state=expected&state=expected&approval=\(approval)"), state: "expected"))
        XCTAssertNil(FleetSignInCallback.approval(request("state=expected&approval=\(approval)", path: "/other"), state: "expected"))
    }

}

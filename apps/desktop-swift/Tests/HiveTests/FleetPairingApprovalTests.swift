import XCTest
@testable import Hive

final class FleetPairingApprovalTests: XCTestCase {
    func testHTTPFramingRejectsAmbiguityAndBrowserRequests() throws {
        let body = "{\"name\":\"Overgaard\"}"
        let head = "POST /request HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: \(body.utf8.count)"
        XCTAssertNil(try FleetPairingHTTP.parse(Data((head + "\r\n\r\n" + body.dropLast()).utf8)))
        XCTAssertEqual(try FleetPairingHTTP.parse(Data((head + "\r\n\r\n" + body).utf8))?.path, "/request")
        for extra in ["\r\nContent-Length: 1", "\r\nTransfer-Encoding: chunked", "\r\nOrigin: https://example.com"] {
            XCTAssertThrowsError(try FleetPairingHTTP.parse(Data((head + extra + "\r\n\r\n" + body).utf8)))
        }
        XCTAssertThrowsError(try FleetPairingHTTP.parse(Data((head + "\r\n\r\n" + body + "extra").utf8)))
        XCTAssertThrowsError(try FleetPairingHTTP.parse(Data(repeating: 65, count: 8193)))
        XCTAssertFalse(FleetPairingHTTP.validName("\nApprove me"))
        XCTAssertFalse(FleetPairingHTTP.validName(String(repeating: "a", count: 101)))
    }
    @MainActor
    func testLocalApprovalIsRequiredBeforeCodeIsIssued() async throws {
        let broker = FleetPairingApproval()
        var issued = 0
        let port = try await broker.start(address: "127.0.0.1") { issued += 1; return "test-only-code" }
        defer { broker.stop() }
        let client = Task { try await FleetPairingClient.requestCode(endpoint: "http://127.0.0.1:\(port)", name: "Test computer") }
        defer { client.cancel() }
        for _ in 0..<40 {
            if !broker.requests.isEmpty { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        let request = try XCTUnwrap(broker.requests.first)
        XCTAssertEqual(issued, 0)
        await broker.approve(request.id)
        let code = try await client.value
        XCTAssertEqual(code, "test-only-code")
        XCTAssertEqual(issued, 1)
        await broker.approve(request.id)
        XCTAssertEqual(issued, 1)
    }
    @MainActor
    func testDeclineNeverIssuesCode() async throws {
        let broker = FleetPairingApproval()
        let port = try await broker.start(address: "127.0.0.1") { XCTFail("Declined request must not issue a code"); return "wrong" }
        defer { broker.stop() }
        let client = Task { try await FleetPairingClient.requestCode(endpoint: "http://127.0.0.1:\(port)", name: "Test computer") }
        defer { client.cancel() }
        for _ in 0..<40 {
            if !broker.requests.isEmpty { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        broker.decline(try XCTUnwrap(broker.requests.first).id)
        do { _ = try await client.value; XCTFail("Expected decline") }
        catch FleetPairingError.declined { }
    }
}

import XCTest
import AppKit
import Darwin
@testable import Hive

final class OwnerAppControlTests: XCTestCase {
    @MainActor
    final class ActionTarget: NSObject {
        var presses = 0
        @objc func press() { presses += 1 }
    }
    @MainActor
    func testRoundTripAndLiveRevocation() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        final class Permission { var allowed = true }
        let permission = Permission()
        let server = OwnerControlSocket(directory: root)
        try server.start(authorized: { permission.allowed }) { input in
            ["operation": input["operation"] as? String ?? ""]
        }
        let path = root.appendingPathComponent("app.sock").path
        let first = try await Task.detached { try Self.call(path) }.value
        XCTAssertTrue(first.contains("\"ok\":true"))
        XCTAssertTrue(first.contains("status"))
        permission.allowed = false
        let second = try await Task.detached { try Self.call(path) }.value
        XCTAssertTrue(second.contains("\"ok\":false"))
        XCTAssertTrue(second.contains("disabled"))
        withExtendedLifetime(server) {}
    }

    private static func call(_ path: String) throws -> String {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8CString)
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw NSError(domain: "path", code: 1) }
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes.map { UInt8(bitPattern: $0) }) }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw NSError(domain: "socket", code: 1) }
        defer { close(fd) }
        var timeout = timeval(tv_sec: 3, tv_usec: 0)
        _ = setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard result == 0 else { throw NSError(domain: "connect", code: Int(errno)) }
        let request = Data("{\"operation\":\"status\"}\n".utf8)
        _ = request.withUnsafeBytes { send(fd, $0.baseAddress, $0.count, 0) }
        var data = Data(); var byte: UInt8 = 0
        while recv(fd, &byte, 1, 0) == 1, byte != 10 { data.append(byte) }
        return String(decoding: data, as: UTF8.self)
    }

    func testRejectsSymlinkDirectoryAndDoesNotReplaceTarget() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let target = root.appendingPathComponent("target")
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: false)
        let link = root.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: target)
        XCTAssertThrowsError(try OwnerControlSocket(directory: link).start { _ in [:] })
        XCTAssertTrue(FileManager.default.fileExists(atPath: target.path))
    }

    func testPrivateListenerRefusesSecondInstance() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        // macOS Unix socket paths are short. Use a short child of the test temp directory.
        let server = OwnerControlSocket(directory: root)
        try server.start(authorized: { true }) { _ in ["alive": true] }
        try withExtendedLifetime(server) {
            XCTAssertThrowsError(try OwnerControlSocket(directory: root).start { _ in [:] })
        }
    }

    func testDoesNotReplaceRegularFileAtSocketPath() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("app.sock")
        try Data("preserve".utf8).write(to: file)
        XCTAssertThrowsError(try OwnerControlSocket(directory: root).start { _ in [:] })
        XCTAssertEqual(try String(contentsOf: file), "preserve")
    }

    @MainActor
    func testOwnUIActionRejectsStaleSnapshotAndSecureField() throws {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 200),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let field = NSSecureTextField(frame: NSRect(x: 10, y: 10, width: 150, height: 30))
        field.stringValue = "must-never-be-returned"
        window.contentView?.addSubview(field)
        window.orderFront(nil)
        defer { window.close() }
        let control = OwnerAppControl()
        let snapshot = control.snapshot()
        let json = try JSONSerialization.data(withJSONObject: snapshot)
        XCTAssertTrue(String(decoding: json, as: UTF8.self).contains("Secure field"))
        XCTAssertFalse(String(decoding: json, as: UTF8.self).contains("must-never-be-returned"))
        XCTAssertThrowsError(try control.act(["snapshot": "stale", "element": "0", "action": "press"]))
    }

    @MainActor
    func testOwnButtonActionDispatchesAndSnapshotCannotBeReplayed() throws {
        _ = NSApplication.shared
        let target = ActionTarget()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 200),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let button = NSButton(title: "Owner control test", target: target, action: #selector(ActionTarget.press))
        window.contentView?.addSubview(button)
        window.orderFront(nil)
        defer { window.close() }
        let control = OwnerAppControl()
        let snapshot = control.snapshot()
        let rows = try XCTUnwrap(snapshot["elements"] as? [[String: Any]])
        let row = try XCTUnwrap(rows.first { $0["label"] as? String == "Owner control test" })
        let request: [String: Any] = ["snapshot": snapshot["snapshot"]!, "element": row["id"]!, "action": "press"]
        let result = try control.act(request)
        XCTAssertEqual(result["dispatched"] as? Bool, true)
        XCTAssertEqual(target.presses, 1)
        XCTAssertThrowsError(try control.act(request))
    }
}

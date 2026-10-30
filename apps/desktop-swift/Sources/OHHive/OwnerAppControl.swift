import AppKit
import Darwin
import Foundation

/// Opt-in control of this app by the signed-in macOS owner. Never listens on TCP.
final class OwnerControlSocket: @unchecked Sendable {
    private final class Response: @unchecked Sendable { var data: Data? }
    static let enabledKey = "ownerAutomationEnabled"
    static var directory: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/ohhive/owner-control")
    }
    private var listener: Int32 = -1
    private var lock: Int32 = -1
    private let queue = DispatchQueue(label: "media.happyjack.hive.owner-control")
    private let directory: URL

    init(directory: URL = OwnerControlSocket.directory) { self.directory = directory }

    func start(authorized: @escaping @MainActor @Sendable () -> Bool = {
        UserDefaults.standard.bool(forKey: OwnerControlSocket.enabledKey)
    }, handler: @escaping @MainActor @Sendable ([String: Any]) async throws -> [String: Any]) throws {
        guard listener < 0 else { return }
        let dir = directory.path
        var metadata = stat()
        if lstat(dir, &metadata) != 0 {
            guard errno == ENOENT else { throw ControlError.invalidDirectory }
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: false,
                attributes: [.posixPermissions: 0o700])
            guard lstat(dir, &metadata) == 0 else { throw ControlError.invalidDirectory }
        }
        guard metadata.st_uid == getuid(), metadata.st_mode & S_IFMT == S_IFDIR,
              metadata.st_mode & 0o077 == 0 else { throw ControlError.invalidDirectory }
        lock = open(dir + "/listener.lock", O_CREAT | O_RDWR | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard lock >= 0, fstat(lock, &metadata) == 0,
              metadata.st_uid == getuid(), metadata.st_mode & S_IFMT == S_IFREG,
              metadata.st_mode & 0o077 == 0, flock(lock, LOCK_EX | LOCK_NB) == 0 else {
            if lock >= 0 { close(lock); lock = -1 }
            throw ControlError.alreadyRunning
        }
        do {
            let path = dir + "/app.sock"
            if lstat(path, &metadata) == 0 {
                guard metadata.st_uid == getuid(), metadata.st_mode & S_IFMT == S_IFSOCK else {
                    throw ControlError.invalidDirectory
                }
                guard unlink(path) == 0 else { throw ControlError.socketFailure }
            } else if errno != ENOENT { throw ControlError.socketFailure }
            var address = sockaddr_un()
            address.sun_family = sa_family_t(AF_UNIX)
            let bytes = Array(path.utf8CString)
            guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw ControlError.pathTooLong }
            withUnsafeMutableBytes(of: &address.sun_path) { target in
                target.copyBytes(from: bytes.map { UInt8(bitPattern: $0) })
            }
            let fd = socket(AF_UNIX, SOCK_STREAM, 0)
            guard fd >= 0 else { throw ControlError.socketFailure }
            guard fcntl(fd, F_SETFD, FD_CLOEXEC) == 0 else { close(fd); throw ControlError.socketFailure }
            let bound = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
                }
            }
            guard bound == 0, chmod(path, 0o600) == 0, listen(fd, 8) == 0 else {
                close(fd); throw ControlError.socketFailure
            }
            listener = fd
            queue.async {
                while true {
                    let client = accept(fd, nil, nil)
                    guard client >= 0 else { break }
                    defer { close(client) }
                    guard fcntl(client, F_SETFD, FD_CLOEXEC) == 0 else { continue }
                    var uid: uid_t = 0; var gid: gid_t = 0
                    guard getpeereid(client, &uid, &gid) == 0, uid == getuid() else { continue }
                    var timeout = timeval(tv_sec: 5, tv_usec: 0)
                    var noSignal: Int32 = 1
                    _ = setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
                    _ = setsockopt(client, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
                    _ = setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
                    var data = Data(); var byte: UInt8 = 0
                    while data.count <= 65_536, recv(client, &byte, 1, 0) == 1, byte != 10 { data.append(byte) }
                    guard byte == 10, data.count <= 65_536,
                          let request = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
                    // The connection is deliberately serialized: app actions retain their order.
                    let finished = DispatchSemaphore(value: 0)
                    let response = Response()
                    Task { @MainActor in
                        let reply: [String: Any]
                        do {
                            guard authorized() else { throw ControlError.disabled }
                            reply = ["ok": true, "result": try await handler(request)]
                        } catch { reply = ["ok": false, "error": String(describing: error)] }
                        if var encoded = try? JSONSerialization.data(withJSONObject: reply), encoded.count <= 1_048_576 {
                            encoded.append(10)
                            response.data = encoded
                        }
                        finished.signal()
                    }
                    finished.wait()
                    // Keep a stalled client off the app's main actor.
                    response.data?.withUnsafeBytes { buffer in
                        var offset = 0
                        while offset < buffer.count {
                            let count = send(client, buffer.baseAddress!.advanced(by: offset), buffer.count - offset, 0)
                            if count <= 0 { break }; offset += count
                        }
                    }
                }
            }
        } catch {
            close(lock); lock = -1; throw error
        }
    }

    deinit {
        if listener >= 0 { Darwin.shutdown(listener, SHUT_RDWR); close(listener) }
        if lock >= 0 { close(lock) }
    }

    enum ControlError: Error { case invalidDirectory, alreadyRunning, socketFailure, pathTooLong, disabled, unknownOperation, staleSnapshot, invalidAction }
}

@MainActor
final class OwnerAppControl {
    private let socket = OwnerControlSocket()
    private var shuttingDown = false
    private var elements: [String: any NSAccessibilityProtocol] = [:]
    private var snapshotID = ""
    private var snapshotShape = ""
    private var sensitiveText: () -> [String] = { [] }
    private(set) var error: String?

    func start(worker: PrivateCodingWorkerModel, github: GitHubAuthManager) {
        guard UserDefaults.standard.bool(forKey: OwnerControlSocket.enabledKey) else { return }
        sensitiveText = { [weak github] in [github?.userCode].compactMap { $0 } }
        do {
            try socket.start { [weak self] request in
                guard let self else { throw OwnerControlSocket.ControlError.disabled }
                switch request["operation"] as? String {
                case "status":
                    return ["pid": ProcessInfo.processInfo.processIdentifier,
                            "source": Bundle.main.object(forInfoDictionaryKey: "OHHiveSourceCommit") as? String ?? "development",
                            "workerEnabled": worker.enabled, "workerStatus": worker.status,
                            "githubConnected": github.isConnected, "shuttingDown": self.shuttingDown]
                case "capabilities":
                    return ["operations": ["status", "capabilities", "worker.start", "worker.stop", "ui.snapshot", "ui.action", "app.quit"],
                            "scope": "This app, signed-in macOS owner only; existing fleet permissions apply.",
                            "limitations": ["OS approval dialogs require the owner", "SwiftUI content controls are not yet exposed; window controls only", "Same-user code requires separate workload isolation"]]
                case "worker.start":
                    guard !self.shuttingDown else { throw OwnerControlSocket.ControlError.invalidAction }
                    worker.start(github: github); return ["accepted": true, "workerStatus": worker.status]
                case "worker.stop":
                    worker.stop(); return ["accepted": true, "workerStatus": worker.status]
                case "ui.snapshot": return self.snapshot()
                case "ui.action": return try self.act(request)
                case "app.quit":
                    self.shuttingDown = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { NSApp.terminate(nil) }
                    return ["accepted": true]
                default: throw OwnerControlSocket.ControlError.unknownOperation
                }
            }
        } catch { self.error = "Owner app control unavailable: \(error)" }
    }

    func beginShutdown() { shuttingDown = true }
    func cancelShutdown() { shuttingDown = false }

    func snapshot() -> [String: Any] {
        elements.removeAll(); snapshotID = UUID().uuidString
        var rows: [[String: Any]] = []
        var seen = Set<ObjectIdentifier>()
        func walk(_ item: any NSAccessibilityProtocol, parent: String?, depth: Int) {
            guard depth < 35, rows.count < 1800, seen.insert(ObjectIdentifier(item)).inserted else { return }
            let id = String(rows.count)
            let role = item.accessibilityRole()?.rawValue ?? "unknown"
            let secure = item.accessibilitySubrole()?.rawValue == "AXSecureTextField"
            let label = secure ? "Secure field" : redact(item.accessibilityLabel() ?? item.accessibilityTitle() ?? "")
            var row: [String: Any] = ["id": id, "role": role, "label": label, "enabled": item.isAccessibilityEnabled()]
            if let parent { row["parent"] = parent }
            if !secure, let value = item.accessibilityValue() as? String { row["value"] = redact(String(value.prefix(2000))) }
            rows.append(row); if !secure { elements[id] = item }
            guard !secure else { return }
            for child in item.accessibilityChildren() ?? [] {
                if let child = child as? any NSAccessibilityProtocol { walk(child, parent: id, depth: depth + 1) }
            }
        }
        for window in NSApp.windows where window.isVisible { walk(window, parent: nil, depth: 0) }
        // System menus include OS-level actions outside this app's control scope.
        // Restrict discovery and actions to descendants of our own windows.
        snapshotShape = shape(rows)
        return ["snapshot": snapshotID, "elements": rows, "truncated": rows.count >= 1800,
                "coverage": "Own AppKit window controls only; SwiftUI content coverage is incomplete."]
    }

    private func redact(_ text: String) -> String {
        sensitiveText().filter { !$0.isEmpty }.reduce(text) { $0.replacingOccurrences(of: $1, with: "[redacted]") }
    }

    private func shape(_ rows: [[String: Any]]) -> String {
        rows.map { "\($0["id"] ?? "")|\($0["role"] ?? "")|\($0["label"] ?? "")|\($0["value"] ?? "")" }.joined(separator: "\n")
    }

    func act(_ request: [String: Any]) throws -> [String: Any] {
        guard request["snapshot"] as? String == snapshotID,
              let id = request["element"] as? String, let old = elements[id] else { throw OwnerControlSocket.ControlError.staleSnapshot }
        let previousShape = snapshotShape
        _ = snapshot()
        guard previousShape == snapshotShape, let current = elements[id],
              ObjectIdentifier(old) == ObjectIdentifier(current), current.isAccessibilityEnabled() else {
            throw OwnerControlSocket.ControlError.staleSnapshot
        }
        let success: Bool
        switch request["action"] as? String {
        case "press": success = current.accessibilityPerformPress()
        case "pick": success = current.accessibilityPerformPick()
        case "increment": success = current.accessibilityPerformIncrement()
        case "decrement": success = current.accessibilityPerformDecrement()
        case "type":
            guard let text = request["text"] as? String, text.utf8.count <= 16_384,
                  current.accessibilityRole() == .textField || current.accessibilityRole() == .textArea else {
                throw OwnerControlSocket.ControlError.invalidAction
            }
            current.setAccessibilityFocused(true)
            guard current.isAccessibilityFocused(), let editor = NSApp.keyWindow?.firstResponder as? NSTextView else { throw OwnerControlSocket.ControlError.invalidAction }
            editor.selectAll(nil); editor.insertText(text, replacementRange: editor.selectedRange())
            success = true
        default: throw OwnerControlSocket.ControlError.invalidAction
        }
        elements.removeAll(); snapshotID = ""
        // AppKit can return false even after dispatching the action and changing
        // the UI. Never encourage a blind retry: report dispatch and require readback.
        return ["dispatched": true, "accessibilityReportedSuccess": success, "refreshRequired": true]
    }
}

// No dependency on Hive's Rust bridge: failures can be explained before the app loads it.
import AppKit
import Darwin

let bundle = Bundle.main.bundleURL
let executable = bundle.appendingPathComponent("Contents/MacOS/Hive-bin").path
let library = bundle.appendingPathComponent("Contents/Frameworks/libohhive_ffi.dylib").path
let checkOnly = CommandLine.arguments.contains("--hive-bundle-check")

func fail(_ detail: String) -> Never {
    let message = "Hive couldn't start because its app bundle is incomplete or damaged. Please reinstall Hive or get a fresh copy of the app.\n\n\(detail)"
    fputs(message + "\n", stderr)
    if !checkOnly {
        let logs = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/Hive")
        try? FileManager.default.createDirectory(at: logs, withIntermediateDirectories: true)
        try? "\(Date()): \(message)\n".write(to: logs.appendingPathComponent("launch-error.log"), atomically: true, encoding: .utf8)
        let alert = NSAlert()
        alert.messageText = "Hive couldn't start"
        alert.informativeText = message
        alert.alertStyle = .critical
        alert.addButton(withTitle: "OK")
        _ = alert.runModal()
    }
    exit(1)
}

guard FileManager.default.isExecutableFile(atPath: executable) else { fail("The app executable is missing.") }
guard let handle = dlopen(library, RTLD_NOW | RTLD_LOCAL) else {
    let detail = dlerror().map { String(cString: $0) } ?? "The bundled engine could not be loaded."
    fail(detail)
}
// Refuse a stale or foreign core before Swift/UniFFI can open user data.
guard let expected = Bundle.main.object(forInfoDictionaryKey: "OHHiveSourceCommit") as? String,
      !expected.isEmpty, expected != "unknown", expected != "unstamped" else {
    fail("The app build identity is missing.")
}
guard let symbol = dlsym(handle, "ohhive_core_source_commit") else {
    fail("The bundled engine has no build identity. Please install a matching app and engine.")
}
typealias CoreStamp = @convention(c) () -> UnsafePointer<CChar>?
let readStamp = unsafeBitCast(symbol, to: CoreStamp.self)
guard let pointer = readStamp() else { fail("The bundled engine build identity is missing.") }
let actual = String(cString: pointer)
guard actual == expected else {
    fail("App/core build mismatch: app \(expected), core \(actual).")
}
let identityLine = "Loki's Den build identity: app=\(expected) core=\(actual)"
fputs(identityLine + "\n", stderr)
if !checkOnly {
    let logs = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/Hive")
    try? FileManager.default.createDirectory(at: logs, withIntermediateDirectories: true)
    try? "\(Date()): \(identityLine)\n".write(to: logs.appendingPathComponent("build-identity.log"), atomically: true, encoding: .utf8)
}
dlclose(handle)
if checkOnly { print("Hive bundle engine loaded successfully."); exit(0) }
let args = ([executable] + CommandLine.arguments.dropFirst()).map { strdup($0) } + [nil]
args.withUnsafeBufferPointer { buffer in _ = execv(executable, buffer.baseAddress!) }
fail("The app executable could not start: \(String(cString: strerror(errno))).")

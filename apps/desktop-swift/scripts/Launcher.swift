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
dlclose(handle)
if checkOnly { print("Hive bundle engine loaded successfully."); exit(0) }
let args = ([executable] + CommandLine.arguments.dropFirst()).map { strdup($0) } + [nil]
args.withUnsafeBufferPointer { buffer in _ = execv(executable, buffer.baseAddress!) }
fail("The app executable could not start: \(String(cString: strerror(errno))).")

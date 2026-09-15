// ADR-029 phase-2, gate #1, comparison slice: same read-only AX proof as ../native-pilot-macos,
// but run as a real .app bundle launched via `open`, not a bare binary run from a shell.
//
// The question this answers: does bundling + ad-hoc signing + LaunchServices launch get this
// process its own distinct Accessibility identity, separate from whatever launched it? The bare
// -CLI sibling package already answered "no" for the unbundled case (confirmed live: the OS
// prompt was literally titled "Terminal", and only "Terminal" appears in System Settings, never
// the CLI binary's own name).
//
// Same non-goals as the CLI sibling: no XPC helper split (a plain single process, no service
// boundary), no window picker (frontmost window only), no screen/pixel capture (title/position/
// size only, never pixels), no input of any kind (no CGEvent import, no AXUIElementPerformAction
// call anywhere in this file).
//
// Launched via `open`, stdout isn't attached to any terminal, so this also writes everything to
// a log file next to wherever the .app bundle itself lives -- see README.md for the exact path
// and run steps.

import ApplicationServices
import AppKit
import Foundation

let logURL: URL = {
    let bundleDir = Bundle.main.bundleURL.deletingLastPathComponent()
    return bundleDir.appendingPathComponent("output.log")
}()

var logHandle: FileHandle? = {
    FileManager.default.createFile(atPath: logURL.path, contents: nil)
    return FileHandle(forWritingAtPath: logURL.path)
}()

func log(_ s: String) {
    print(s)
    if let data = (s + "\n").data(using: .utf8) {
        logHandle?.write(data)
    }
}

log("=== ADR-029 native pilot (bundled .app slice): read-only AX proof ===")
log("Bundle:  \(Bundle.main.bundleURL.path)")
log("Bundle ID: \(Bundle.main.bundleIdentifier ?? "?")")
log("Process: \(ProcessInfo.processInfo.processName)  PID: \(ProcessInfo.processInfo.processIdentifier)")
log("Log:     \(logURL.path)")
log("")

let promptOptions = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
let trusted = AXIsProcessTrustedWithOptions(promptOptions)

if !trusted {
    log("Not yet trusted. macOS should have shown (or will show) an Accessibility permission")
    log("prompt. THE KEY QUESTION: what name does that prompt show? Check whether it says")
    log("\"NativePilotApp\" (this bundle got its own identity) or \"Terminal\"/something else")
    log("(the grant landed on whatever launched it, same as the bare-CLI sibling test).")
    log("Grant it in System Settings > Privacy & Security > Accessibility, then reopen this app.")
    logHandle?.closeFile()
    exit(1)
}

log("Trusted: AX permission is granted to this process.")
log("")

func readFrontmostWindow() -> String {
    guard let app = NSWorkspace.shared.frontmostApplication else {
        return "  (no frontmost application)"
    }
    let axApp = AXUIElementCreateApplication(app.processIdentifier)
    var windowRef: CFTypeRef?
    let winResult = AXUIElementCopyAttributeValue(axApp, kAXFocusedWindowAttribute as CFString, &windowRef)
    guard winResult == .success, let window = windowRef else {
        return "  frontmost app: \(app.localizedName ?? "?") (bundle \(app.bundleIdentifier ?? "?")) -- no focused window via AX"
    }
    let axWindow = window as! AXUIElement

    var titleRef: CFTypeRef?
    AXUIElementCopyAttributeValue(axWindow, kAXTitleAttribute as CFString, &titleRef)
    let title = (titleRef as? String) ?? "(untitled)"

    var posRef: CFTypeRef?
    var posValue = CGPoint.zero
    if AXUIElementCopyAttributeValue(axWindow, kAXPositionAttribute as CFString, &posRef) == .success,
       let posAX = posRef {
        AXValueGetValue(posAX as! AXValue, .cgPoint, &posValue)
    }

    var sizeRef: CFTypeRef?
    var sizeValue = CGSize.zero
    if AXUIElementCopyAttributeValue(axWindow, kAXSizeAttribute as CFString, &sizeRef) == .success,
       let sizeAX = sizeRef {
        AXValueGetValue(sizeAX as! AXValue, .cgSize, &sizeValue)
    }

    return "  frontmost app: \(app.localizedName ?? "?") (bundle \(app.bundleIdentifier ?? "?"), pid \(app.processIdentifier))\n" +
           "  window title:  \(title)\n" +
           "  geometry:      x=\(Int(posValue.x)) y=\(Int(posValue.y)) w=\(Int(sizeValue.width)) h=\(Int(sizeValue.height))"
}

log("Reading frontmost window every 2s. This process performs NO input of any kind.")
log("Runs for up to 60s on its own, then stops -- no Ctrl+C available since this is detached.")
log("")

var iterations = 0
let maxIterations = 30
while iterations < maxIterations {
    log("[\(iterations)] \(Date())")
    log(readFrontmostWindow())
    log("")
    iterations += 1
    Thread.sleep(forTimeInterval: 2.0)
}
log("Reached the \(maxIterations)-iteration safety bound. Stopping.")
logHandle?.closeFile()

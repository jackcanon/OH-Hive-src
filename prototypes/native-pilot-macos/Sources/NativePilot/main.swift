// ADR-029 phase-2, gate #1 slice: read-only Accessibility proof of signing/TCC attribution.
//
// What this proves, if it runs clean:
//   - Whether AXIsProcessTrustedWithOptions attributes the permission prompt to THIS binary
//     (by name, in System Settings > Privacy & Security > Accessibility) as expected for a
//     plain ad-hoc-signed command-line executable -- no XPC helper, no app bundle yet.
//   - Whether a read-only AX query (frontmost app, focused window title, position, size) works
//     once granted -- geometry/bounds data, the same shape the real policy engine's Observation
//     needs.
//   - Whether that grant survives a rebuild (rerun after `swift build` again, without re-granting,
//     to see if ad-hoc signing loses it -- the known HaloBench pain point noted in CONTINUITY.md).
//
// What this does NOT prove (explicitly out of scope for this slice):
//   - XPC process-boundary attribution (this is one plain process, not a helper split).
//   - Window PICKER/filtering across multiple windows -- this only reads the frontmost window.
//   - Screen/pixel capture of any kind -- title/position/size only, never pixels or window content.
//   - Any local indicator UI or Stop control beyond this console loop and Ctrl+C.
//   - Any input (click/type) -- this package cannot post events, by construction: no CGEvent
//     import, no AXUIElementPerformAction call anywhere in this file.
//
// Run against a synthetic/neutral app (TextEdit with a throwaway untitled document is a good
// choice), not anything with real personal content, per the design doc's own guidance.

import ApplicationServices
import AppKit
import Foundation

print("=== ADR-029 native pilot: read-only AX proof (gate #1 slice) ===")
print("Process: \(ProcessInfo.processInfo.processName)  PID: \(ProcessInfo.processInfo.processIdentifier)")
print("Binary:  \(CommandLine.arguments.first ?? "?")")
print()

let promptOptions = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
let trusted = AXIsProcessTrustedWithOptions(promptOptions)

if !trusted {
    print("Not yet trusted. macOS should have shown (or will show) an Accessibility permission")
    print("prompt attributed to this process. Grant it in System Settings > Privacy & Security >")
    print("Accessibility, then run this again -- do not click Allow on anything you did not")
    print("expect to see prompted as THIS binary.")
    exit(1)
}

print("Trusted: AX permission is granted to this process.")
print()

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

print("Reading frontmost window every 2s. This process performs NO input of any kind.")
print("Stop = Ctrl+C.")
print()

var iterations = 0
let maxIterations = 30 // ~60s safety bound so this never runs unattended forever
while iterations < maxIterations {
    print("[\(iterations)] \(Date())")
    print(readFrontmostWindow())
    print()
    iterations += 1
    Thread.sleep(forTimeInterval: 2.0)
}
print("Reached the \(maxIterations)-iteration safety bound. Stopping. Rerun to continue observing.")

# ADR-029 native pilot -- gate #1 slice (signing/TCC/AX read-only proof)

Standalone throwaway Swift package, deliberately outside `apps/desktop-swift`. It exists to answer
one narrow question before any real design work continues: **does a plain ad-hoc-signed macOS
command-line binary get a clean, correctly-attributed Accessibility (TCC) permission, and does a
read-only AX query actually return usable geometry/window data once granted?**

This is the smallest possible slice of design doc section 9, gate #1 ("Native read-only pilot").
It does **not** attempt XPC process-boundary attribution, window picker/filtering across multiple
windows, screen/pixel capture, a real local-indicator UI, or any input -- those are separate,
larger pieces of gate #1 still to build once this narrow question is answered.

**Status: untested.** Written from a sandboxed shell with no Swift toolchain (same limitation
noted elsewhere in CONTINUITY.md) -- this has never been compiled. Treat the first `swift build`
as a real compile attempt, not a formality; the Accessibility API's Swift bridging (particularly
around `Unmanaged<CFString>` constants and `AXValueGetValue`'s pointer conversion) is exactly the
kind of thing that's right in a reference but wrong in a specific SDK/toolchain version. Expect to
iterate on compiler errors together.

## Run it

```
cd "/Volumes/10TB JBOD/Agents/Claude/Projects/Apps/OH Cloud-src/prototypes/native-pilot-macos"
swift build
.build/debug/NativePilot
```

First run should trigger a macOS Accessibility permission prompt. **Check that the prompt names
this binary specifically** (not some other process) before clicking Allow -- that attribution is
exactly what this prototype is testing. Grant it in System Settings > Privacy & Security >
Accessibility if the prompt doesn't appear or you dismiss it.

Before granting, switch the frontmost window to something synthetic/neutral -- TextEdit with an
untitled document is fine. Don't run this against a window with real personal content; per the
design doc's own guidance, synthetic documents are enough to prove the mechanism.

## What a clean run looks like

- Prompt correctly attributed to `NativePilot` (or whatever `swift build` names the binary).
- Once granted, the tool prints the frontmost app's name/bundle id, the focused window's title,
  and its position/size, once every 2 seconds, for up to 30 iterations (~60s), then stops on its
  own. Ctrl+C stops it early.
- Rerun it again (with no code changes) to see whether the grant persisted across the same binary
  running twice, and rerun after a `swift build` rebuild to see whether ad-hoc signing loses the
  grant on rebuild -- CONTINUITY.md already notes this bit HaloBench ("ad-hoc builds lose their
  macOS permissions on every rebuild"), worth confirming whether the same is true here.

## After a clean run

Worth also running `codesign -dv --verbose=4 .build/debug/NativePilot` and pasting the output back
-- that's the other half of "signing proof": what identity/Team ID (if any) this ad-hoc build
carries, which matters for the real XPC-helper-vs-single-process boundary decision Jack/Claude
still owe from the ADR-029 review ("prototyping real TCC/signing attribution before committing to
the XPC-helper process boundary").

## Explicitly not covered here (future slices of the same gate)

- XPC helper split and its own TCC attribution.
- Window picker/filtering (multiple windows, cross-app).
- Any screen/pixel capture (ScreenCaptureKit) -- this reads AX metadata only, never pixels.
- A real local indicator overlay UI and Stop control (this has a console loop and Ctrl+C only).
- Any input whatsoever -- there is no CGEvent import and no `AXUIElementPerformAction` call in
  this package, by construction.

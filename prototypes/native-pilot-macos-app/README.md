# ADR-029 native pilot -- bundled `.app` slice (signing/TCC comparison)

Second half of the gate #1 signing/TCC experiment. `../native-pilot-macos` already answered the
bare-CLI question live: a plain binary run from Terminal gets no identity of its own -- the
permission prompt was literally titled "Terminal", and only "Terminal" ever appeared in System
Settings (see CONTINUITY.md, "Native pilot gate #1 headline finding").

This package asks the other half of the same question: does a real, ad-hoc-signed `.app` bundle,
launched through LaunchServices (`open`, not run directly), get its **own** distinct entry?

**Status: untested**, same caveat as the CLI sibling -- written from a shell with no Swift
toolchain, never compiled.

## Build and run

```
cd "/Volumes/10TB JBOD/Agents/Claude/Projects/Apps/OH Cloud-src/prototypes/native-pilot-macos-app"
./build_app.sh
open NativePilotApp.app
```

**Important: use `open NativePilotApp.app`, not the binary inside it directly.** Running
`NativePilotApp.app/Contents/MacOS/NativePilotApp` straight from the shell would just repeat the
bare-CLI test with extra steps -- the whole point is the LaunchServices-mediated launch path.

`build_app.sh` prints the `codesign -dv --verbose=4` output automatically as part of the build --
paste that back along with what happens next.

## What to check, in order

1. **The permission prompt's title.** This is the entire question. Does it say
   "NativePilotApp would like to control this computer..." (own identity -- the bundle path
   worked) or does it say "Terminal" again, or something else entirely (still inherited)?
2. **System Settings > Privacy & Security > Accessibility.** Is there now a `NativePilotApp`
   entry, separate from Terminal? (Screenshot it, same as last time -- that list is the ground
   truth, not what the prompt happened to say in the moment.)
3. **The log.** `open` detaches stdout, so output goes to `NativePilotApp.app/../output.log`
   (i.e. right next to the `.app` in this same directory) instead. `cat output.log` after it
   finishes (~60s) or stops early on the not-yet-trusted path.

## Point the frontmost window at something synthetic

Same guidance as the CLI sibling: switch to TextEdit with an untitled document (or similar)
before/while this runs, not a window with real personal content.

## Explicitly not covered here (same scope limits as the CLI sibling)

XPC helper split, window picker/filtering, any screen/pixel capture, a real local-indicator
overlay UI, any input whatsoever (no `CGEvent` import, no `AXUIElementPerformAction` call
anywhere in this package).

## If this DOES get its own identity

That's the positive result the design doc's XPC-helper-boundary decision was waiting on --
proof that *some* form of proper bundling/signing produces isolated, per-app TCC identity on
this machine, matching what Codex Computer Use and CuaDriver already demonstrate exist as
possibilities here. It would still leave open whether an XPC **service** specifically (vs. this
plain bundled single process) is needed for the next gate, or whether bundle-level identity is
enough for gate #1's purposes -- that distinction is future work, not answered by this slice.

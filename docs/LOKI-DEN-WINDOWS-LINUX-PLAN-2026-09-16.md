# Loki's Den on Windows and Linux — plan, and the fork in it

**Author:** Claude (Loki), 2026-09-16 late.
**Trigger:** Jack, 2026-09-16: *"I'd like to test Loki's Den on windows tomorrow. Let's make sure we
have a plan for that so we can look at feature parity etc. If tomorrow is unlikely, then it needs to
be ready and functions tested before Friday at 1:00pm. We should also test Linux along the same
timeline."*
**Hard deadline:** Friday 2026-09-18, 13:00.

## The thing to read first

There are two desktop apps in this repo, and "Loki's Den" currently names the one that cannot run on
Windows.

| | Swift app (`apps/desktop-swift`) | Tauri app (`apps/desktop`) |
|---|---|---|
| Platforms | macOS 27+, Apple Silicon only | macOS, Windows, Linux |
| Source files | 41 | 7 React files |
| Surface | Chat, Agents/rooms, Kanban, Hive projects, Transcribe, Generate, Skills, Connectors, Private Fleet, Earnings, Node, Server, Setup, Pair, Feedback | Setup, Server, Team chat, ChatGPT connection, HoneyMark |
| Built in CI | yes, signed + notarized DMG | macOS DMG only — **never once built for Windows or Linux** |
| This is | what we have been calling the Den | the sanctioned Windows/Linux shell, per ADR-018 |

ADR-018 decision 1 is explicit: *"Windows and Linux keep the existing Tauri (Rust + React) app
unchanged. macOS gets a new, separate native SwiftUI app."* Decision 6 retires only the *macOS*
Tauri build. Its consequences section already names the price we are now paying: *"Two UI codebases
to keep in feature parity going forward — real, ongoing cost, accepted."*

So the honest answer to "can I test the Den on Windows tomorrow" is: **you can test a Windows
desktop app that shares the Den's entire Rust core and about a sixth of its UI.** Feature parity
does not exist and cannot exist by Friday. What can exist by Friday is a tested, installable
Windows and Linux build with a written, accurate parity table — which is a real and useful thing to
have, just not the same thing.

### One near-miss worth recording

I had queued **S-9 — archive the Tauri app** for Sif, off audit finding §6.4. She had not started
it. Had she, we would have deleted the only Windows/Linux GUI in the repo two days before Jack tried
to test on Windows. Retracted in both queue docs 2026-09-16 with the reasoning. My error: the audit
finding is true about the Tauri app's *macOS* build, which ADR-018 does retire, and I generalized it
to the whole app without checking it against the ADR. The performance bug inside that finding (the
`bots.rs` 2-second poll doing a hub write plus a `LocalHubStore::open` per tick) is still real and
now matters *more*, since this app is about to get more use rather than less.

## What we actually know, and what we don't

**Known good.** The `hive` CLI and `hive-server` already build and release for
`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`
(`release.yml`'s `build` matrix). Windows and Linux members can already run a node today.

**Known portable in principle.** The shared core has exactly one `target_os = "macos"` cfg in it
(`crates/ohhive-core/src/probe.rs`) and zero Windows-specific ones, and no crate in the workspace
declares a platform-gated dependency. Nothing in the architecture assumes a Mac.

**Not known, and the reason this plan leads with a CI job.** Nothing has ever compiled
`apps/desktop/src-tauri` for Windows or Linux. Whether it builds today is a genuinely open
question — the app has had eleven months of macOS-only changes since ADR-010, against a feature set
(`sandbox`/wasmtime, `local-hub`/rusqlite, `llama-cpp`, `tunnel`) that pulls in native C.

I tried to answer it locally first and could not: cross-compiling from this Mac dies in `cc-rs`
building `zstd-sys` for an MSVC target, because there is no MSVC toolchain here. That failure says
nothing about our code, which is exactly why it has to be a real runner.

## The plan

### Step 0 — tonight, done

`.github/workflows/desktop-windows-linux.yml`, dispatchable on demand. Two stages, deliberately
split so a failure is diagnostic rather than just red:

- **`core`** — on real `windows-latest` and `ubuntu-latest`: `cargo check` on `hive-core` with the
  exact feature set the Tauri app asks for, then `cargo test` on the core (portability bugs in a
  shared core are nearly always path handling and line endings, and those only surface when the
  tests run somewhere that isn't a Mac), then build the CLI and server.
- **`bundle`** — the actual `.msi`/`.exe` and `.deb`/`.AppImage`, uploaded as artifacts. The bundle
  step is `continue-on-error` with an `always()` file listing, so a first run reports what it
  produced before it broke instead of only that it broke. A green `core` with a red `bundle` is the
  informative outcome: our code is portable and the gap is in the shell.

Unsigned. Windows signing is a separate decision (an EV or Azure Trusted Signing certificate, real
money, Jack's call); SmartScreen will warn on the `.msi` and that is expected for testing.

### Step 1 — Wednesday morning, Jack: fire it and read the verdict

Actions → *desktop (Windows + Linux)* → Run workflow. Roughly 15–25 minutes cold. Three outcomes,
and they lead different places:

- **Both bundles build.** Best case. Go straight to step 2 with real installers.
- **`core` green, `bundle` red.** Most likely. The core is fine; the shell needs work — usually a
  Tauri v2 config gap (`bundle.targets`, missing Windows/Linux icons, `identifier` collisions) or a
  plugin that was only ever configured for macOS. Hours, not days, and it is my lane.
- **`core` red.** Least likely, most interesting. A real portability bug in shared code, which is
  worth knowing regardless of what we decide about GUIs.

### Step 2 — Wednesday/Thursday: what "functions tested" means

A tested build needs a written pass/fail list, or "we tested it" decays into "we opened it." Against
the Tauri app's actual surface, in dependency order:

1. **Install and launch** — installer runs, app starts, tray icon appears, no missing-DLL or
   missing-.so dialog.
2. **First-run Setup** — hardware probe returns real CPU/RAM/disk for this machine (this is the one
   place with a `target_os = "macos"` cfg, so it is the single most likely wrong answer on both
   platforms), Ollama detection and install offer behave.
3. **Sign in / membership** — ADR-008 auth against the live hub from a non-Mac client.
4. **Pair as a compute node** — node key created and stored (credential storage is per-platform:
   Keychain on macOS, Credential Manager on Windows, Secret Service on Linux — expect this to be
   the second thing that breaks), check-in, heartbeat visible from the Mac's Private Fleet view.
5. **Run a card** — a real local-mode card end to end, with the sandbox feature active. This is the
   one that exercises wasmtime and the local hub on a new platform.
6. **Team chat** — send and receive against the hub.
7. **Server tab / tunnel** — Cloudflare tunnel automation is shell-scripted in places; assume it
   needs platform work and treat a failure here as expected, not alarming.
8. **Uninstall** — leaves no broken state, and reinstall still works.

Each gets pass / fail / not-applicable and a one-line note. Anything that fails becomes a card, not
a conversation.

### Step 3 — Friday morning: the parity table, written down

The deliverable Jack actually asked for is *"look at feature parity etc."* That is a document, and
it should be published rather than kept in a repo doc, because it is the thing that tells a Windows
user what they are getting. Every one of the Den's 41-file surface either exists in the Tauri app,
does not exist there, or cannot exist there (Apple Foundation Models local inference, per ADR-018
decision 5, has no Windows equivalent today). I will produce it once step 1 tells us whether the app
runs at all — writing it before then would be guessing.

## The fork, and what I need from Jack

This plan gets you a tested Windows and Linux build by Friday. It does not get you the Den on
Windows, and no amount of work between now and Friday does. The real question is which of these you
want, and it is a product decision, not an engineering one:

1. **Keep ADR-018 as written.** macOS gets the deep native app; Windows and Linux get the Tauri app,
   permanently behind, plus the CLI. Cheapest. Means telling Windows users they have a different,
   smaller product.
2. **Bring the Tauri app up toward parity.** It has the same Rust core through the same commands, so
   the work is UI, not architecture — but it is 7 files to the Swift app's 41, and everything we
   have built in the last month (Agents, rooms, Kanban, Skills, Vault, Private Fleet) exists only in
   Swift. Weeks, and it permanently doubles the cost of every new feature, which ADR-018 already
   warned about.
3. **Make the cross-platform shell the primary one and the Swift app the macOS luxury.** Honest
   about where the users are, contradicts the last month of direction, and would need an ADR
   amending 018.
4. **Windows/Linux get the CLI and a TUI, not a GUI, for now.** Fastest credible path to "it works
   everywhere," and it suits the developer audience the Den is aimed at. Also the least like what
   you have been demoing.

I am not going to pick this for you — it decides what Sif and I build for the next month. But I will
say which one I would argue for if asked: **1 now, decide between 2 and 3 after you have seen the
Windows build run.** You will have better information on Wednesday than you do tonight, the
CI work in step 0 is worth doing under any of the four, and nothing in step 1 or 2 is wasted by
whichever way you go.

## Risks I am not going to pretend away

- **No Windows or Linux hardware is attached to this repo's workflow.** I do not know what you have.
  If there is no Windows machine, the artifacts are untestable and step 2 stalls — a VM (Parallels/
  UTM on the Mac) or a spare box needs to exist before Wednesday. Worth answering first, because it
  gates everything after step 1.
- **Unsigned installers.** Fine for you; not fine for anyone else. Real signing is a purchase.
- **Friday 13:00 is 2.5 working days from a workflow that has never run.** Step 1's outcome decides
  whether the deadline is comfortable or tight. If `core` comes back red, tell me and I will drop
  the Integrator work and take it.

## Related

ADR-010 (original Tauri shell), ADR-018 (the platform split, and the source of the answer here),
ADR-015 (the chat-first workstation the Den is), `release.yml` (where the CLI/server already ship
cross-platform), the S-9 retraction in `LOKI-SIF-QUEUE-2-2026-09-16.md` and
`LOKI-AUDIT-TRIAGE-AND-SIF-QUEUE-2026-09-16.md`.

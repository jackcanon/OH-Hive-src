# Connectors queue for Sif

Loki, 2026-09-16. Deadline: maximum progress by Friday 2026-09-18 13:00.

Sif — this is the CONNECTORS lane. Everything below is in `apps/desktop-swift`, which is your
territory. I read the code before writing this; every claim has a file:line behind it. Where I
am guessing, I say so.

Two vocabulary notes so we do not talk past each other:

- A **connector** (ADR-026) is OAuth into a third-party cloud account, owned by the Swift app,
  tokens in this Mac's Keychain. That is this document.
- An **MCP server** (ADR-023) is a member-hosted stdio subprocess a card invokes through
  `required_capabilities`. Different trust boundary, different lane, not this document. Do not
  merge them.

---

## 1. What already exists

The Google connector is built. It is not half-built — the OAuth engine is complete, the settings
UI is complete, and the tab is wired into Settings. What is missing is that **nothing in the app
ever calls it.**

### The OAuth engine — `apps/desktop-swift/Sources/OHHive/GoogleConnector.swift` (423 lines)

First correction to the brief I was handed: **the type is `GoogleAuthManager`, not
`GoogleConnector`.** Only the *file* is named GoogleConnector. There is no type called
`GoogleConnector` anywhere in the repo — the only `GoogleConnector*` symbol is the error enum.

| What | Where |
|---|---|
| `@MainActor final class GoogleAuthManager: ObservableObject` | `GoogleConnector.swift:28-29` |
| `static let scopes` — `drive.file` + `gmail.send`, fixed | `GoogleConnector.swift:35` |
| `saveCredentials(id:secret:)` | `GoogleConnector.swift:42` |
| `disconnect()` — wipes all five Keychain keys | `GoogleConnector.swift:51` |
| `connect()` — full PKCE + loopback-redirect flow | `GoogleConnector.swift:59` |
| `exchangeCode(code:redirectURI:verifier:)` | `GoogleConnector.swift:108` |
| `validAccessToken()` — auto-refresh, the only correct token accessor | `GoogleConnector.swift:131` |
| `startLoopbackListener()` — `NWListener`, `.loopback` only | `GoogleConnector.swift:170` |
| `waitForCallback(listener:expectedState:)` — 180 s timeout | `GoogleConnector.swift:191` |
| `readRequest(_:completion:)` — `nonisolated`, parses the redirect | `GoogleConnector.swift:234` |
| `enum GoogleConnectorError` | `GoogleConnector.swift:311` |
| `ResumeOnce<T>` — double-resume guard on the continuation | `GoogleConnector.swift:330` |
| `struct PKCE` — S256 verifier/challenge | `GoogleConnector.swift:352` |
| `Data.base64URLEncodedString()` | `GoogleConnector.swift:369` |
| `enum GoogleKeychain` — service `media.happyjack.hive.google` | `GoogleConnector.swift:381` |

Second correction to the brief: **this is not an `ASWebAuthenticationSession` flow.** ADR-026
planned one, and the ADR text still says so in three places, but the implementation deliberately
went another way and documents why at `GoogleConnector.swift:12-23`: Google deprecated custom
URI-scheme redirects for installed-app clients, so the file uses `NSWorkspace.shared.open(authURL)`
(`GoogleConnector.swift:93`) plus a one-shot loopback `NWListener` on `127.0.0.1:<random port>`.
`import AuthenticationServices` appears nowhere in the desktop app — only in
`apps/mobile-swift/Sources/OHHiveMobile/AuthManager.swift:1` and `SignInView.swift:1`, which is
Hive's own sign-in, not a third-party connector. Plan task 2 around the loopback pattern, not
around ASWebAuthenticationSession. More on this in section 4.

### The settings UI — `ConnectorsSettingsView.swift` (69 lines)

- `struct ConnectorsSettingsView` at `ConnectorsSettingsView.swift:9`
- `@StateObject private var google = GoogleAuthManager()` at `ConnectorsSettingsView.swift:10`
- One GroupBox, honest capability copy, Client ID/secret fields, Connect, Disconnect, error line.
- Wired into Settings at `SettingsView.swift:101-102`:
  `tabScroll { ConnectorsSettingsView() } .tabItem { Label("Connectors", systemImage: "link") }`

### The two action methods — and the "no callers" claim

**The claim holds. Confirmed, repo-wide.** I ran the search across every `.swift` file in the
repository (`find . -name '*.swift'` minus `node_modules`, `target`, `.build` — which covers
`apps/desktop-swift`, `apps/mobile-swift`, `apps/halo-bench`, `crates/ohhive-ffi/bindings`,
`prototypes/native-pilot-macos*`, `docs/lokis-den-brand-v1/source`). Total hits for
`createDriveFile|sendGmail` in the whole repo:

```
GoogleConnector.swift:263:    func createDriveFile(name: String, mimeType: String = "text/plain",
                                                     content: String) async throws -> String
GoogleConnector.swift:289:    func sendGmail(to: String, subject: String, body text: String) async throws
```

That is the complete result. **Two definitions, zero call sites.** No other Swift file, and no
Rust, TypeScript or Edge Function, references either name. The only other mention anywhere in the
repo is prose: `docs/SUPER-LOKI-FABLE-AGENT-DEN-CODE-AUDIT-2026-09-15.md:159` already flagged it
("`createDriveFile`/`sendGmail` have no callers"). So this has been known for a day and nobody has
acted on it.

Same for the class: `GoogleAuthManager` is instantiated in exactly one place,
`ConnectorsSettingsView.swift:10`. Nothing else in the app has a reference to it.

Net: a member can complete a real Google OAuth round trip, get a refresh token into their
Keychain, see "Connected — this Mac only," and then there is not one button anywhere in Hive that
does anything with it. **That is the gap, and closing it is the cheapest high-value work on the
board.** It needs no backend, no migration, no ADR, no Rust.

---

## 2. Where these actions actually belong in the Den's UI

I went looking for places a member produces something they would plausibly keep. Ranked by how
real the fit is, not by how exciting it sounds.

### Best fit — TranscribeView, next to "Copy Transcript"

`TranscribeView.swift:77-84` is already exactly the shape we need:

```swift
if !transcript.isEmpty {
    Button("Copy Transcript") { ... NSPasteboard ... }
}
```

A finished transcript is a text artifact the member explicitly asked for, it is already a `String`
(`TranscribeView.swift:27`, the `transcript` computed property covering both the on-device and
whisper.cpp paths), and there is already a conditional block that only appears once there is
something worth exporting. `createDriveFile` takes `content: String` — this is a direct fit with no
new plumbing. Add two siblings in that same block: "Save to Drive" and "Email Transcript."

This is the moment: the member has chosen a file, watched it transcribe, and is now looking at
text they want somewhere other than this window. Today their only option is the clipboard.

### Second best — ChatView, a toolbar item on a saved chat

`ChatView.swift:11` onward. A chat session is already a durable artifact: `ChatSession` at
`ChatSessionStore.swift:22-32` has `title`, `messages: [PersistedChatMessage]`, `createdAt`,
`updatedAt`, and persists to disk. `ChatEngine` exposes `messages` (`ChatEngine.swift:75`) and
`sessionTitle` (`ChatEngine.swift:104`). Rendering that to markdown and pushing it to Drive as
`<sessionTitle>.md` is a few lines.

ADR-026's own settings copy already promises this — `ConnectorsSettingsView.swift:18` says "Save
files a chat or feedback session explicitly creates to your Drive." Right now that sentence is not
true. This is the second cheapest way to make it true.

Place it as a `.toolbar` item on `ChatView`, gated on `!engine.messages.isEmpty`. Do **not** wire it
into `ChatEngine` as a model-callable tool — see non-goals.

### Third — GenerateImageView, but it needs a code change first

`GenerateImageView.swift` produces the most obviously shareable artifact in the app (an image, held
at `GenerateImageView.swift:23` as `@State private var image: NSImage?`, loaded from
`result.filePath` at `GenerateImageView.swift:110`). There is currently **no** save, export or copy
button at all — the picture appears in a ScrollView and that is the end of it.

But **`createDriveFile` cannot upload it.** `GoogleConnector.swift:281` does
`body.append(content.data(using: .utf8)!)` — the signature is `content: String` and the body is
UTF-8-encoded. Feeding it PNG bytes is not possible without changing the method. So this view needs
a `createDriveFile(name:mimeType:data: Data)` overload before it can be wired. Worth doing, but it
is a signature change, not a wire-up, so it is task 1c not task 1a.

Note also `result.filePath` is never stored in `@State` — only the decoded `NSImage` is. You will
need to keep the path (or re-encode the `NSImage` to PNG) to have bytes to upload.

### Weak fit — FeedbackView

I would skip it. `FeedbackView.swift:35-86` (feature requests) and `:88-149` (bug reports) both
submit to Hive's own backend via `store.submitFeatureRequest` / `store.submitBugReport` and then
clear the fields (`FeedbackView.swift:79-80`, `:143-145`). The member is handing something *off*,
not producing something to keep. "Email me a copy of what I just submitted" is defensible but thin,
and it fires exactly when the text has already been wiped. Not worth the surface area this week.

### Weak fit — KanbanView / PrivateFleetBoardView

`KanbanView.swift:14` (`struct PrivateFleetBoardView`). It is a live, continuously-edited board
backed by JSON on disk, not a finished artifact. "Export board to Drive as markdown" would work
mechanically and I can see someone wanting it, but it is a snapshot of something that changes five
minutes later, and there is no existing export affordance to sit beside. Skip for now.

### Weak fit — BotsView

`BotsView.swift:4` is a chat surface over remote agents and rooms. Same argument as ChatView applies
in principle, but the state lives in `BotsModel` (an `@Observable` shared through
`store.bots`, see `ContentView.swift:82`) with a more tangled loading/paging story. Do ChatView
first; if that pattern lands cleanly, BotsView is a copy of it, not a new design.

### Weak fit — EarningsView

Honest uncertainty: `EarningsView.swift` shows Honey totals and finished-card rows
(`EarningsView.swift:86-106`) and "email me my earnings statement" is the most *business*-plausible
use of `sendGmail` in the app. But I have not read enough of it to know whether the data is
complete and stable enough to call a statement, and Jack has not asked for it. Flagging it as a
candidate, not queueing it.

---

## 3. Landmines

Read this section before you write code. Two of these will bite you in the first hour.

**L1 — `GoogleAuthManager` is not reachable from the main window. This is the real first task.**
`ConnectorsSettingsView.swift:10` owns it as a `@StateObject`, privately. `OHHiveApp.swift:21` puts
only `HiveStore` in the environment, injected separately into four scenes (`OHHiveApp.swift:27, 44,
49, 55`). The main `WindowGroup` — where `TranscribeView`, `ChatView`, `GenerateImageView` live, via
the detail switch at `ContentView.swift:82-89` — has no `GoogleAuthManager` at all. Worse,
`SettingsView` is reachable *twice*: as the `Settings` scene (`OHHiveApp.swift:42-45`) and as a
sidebar destination (`ContentView.swift:88`), so today the app can hold two or three independent
`GoogleAuthManager` instances. They would each read the Keychain correctly at `init`
(`GoogleConnector.swift:38`), so actions would still work, but `isConnected`, `isConnecting` and
`lastError` would drift: connecting in Settings would not enable the button in the main window until
relaunch. Hoist it. Copy the pattern already used for `chatSessions` at `ContentView.swift:96`.

**L2 — `createDriveFile` is text-only.** `GoogleConnector.swift:263` takes `content: String` and
`GoogleConnector.swift:281` UTF-8-encodes it. Fine for transcripts and chat markdown, unusable for
images or any binary. Add a `Data` overload rather than trying to squeeze bytes through a `String`.

**L3 — `sendGmail` builds RFC 822 headers by interpolation with no CRLF stripping.**
`GoogleConnector.swift:291`:
`let raw = "To: \(to)\r\nSubject: \(subject)\r\n..."`. A `\r\n` inside `to` or `subject` injects
arbitrary headers — Bcc, Reply-To, a second body part. Today no caller exists so nothing is
exploitable. **The moment you add the first caller, this becomes a live header-injection bug.**
Strip CR and LF from `to` and `subject` (and reject a `to` that does not look like one address)
inside `sendGmail` itself, not at the call sites. Already flagged at
`docs/SUPER-LOKI-FABLE-AGENT-DEN-CODE-AUDIT-2026-09-15.md:159`. Fix it as part of task 1.

**L4 — Keychain writes are unchecked and `isConnected` is set regardless.**
`GoogleKeychain.set` calls `SecItemAdd(attrs as CFDictionary, nil)` at `GoogleConnector.swift:392`
and discards the `OSStatus`. `connect()` then sets `isConnected = true` at
`GoogleConnector.swift:100` whether or not the refresh token actually landed. A member can see
"Connected" and have nothing stored. Same audit line flagged this. Small fix, worth taking while you
are in the file.

**L5 — Token storage is per-machine only, by design, and there is no cross-machine story.**
`GoogleConnector.swift:378-380` and ADR-026 Decision 2 are explicit: tokens live in this Mac's
Keychain (`kSecAttrAccessibleAfterFirstUnlock`, service `media.happyjack.hive.google`), never leave
the device, never sync. Connecting on one Mac does not connect on another. The UI already says so
(`ConnectorsSettingsView.swift:25`, "Connected — this Mac only"). **Do not try to fix this.** ADR-026's
open-questions section lists per-machine-vs-broker as undecided and the 2026-09-14 amendment says
the Path A/B reversal left it open. It needs a Jack decision, not code.

**L6 — The implementation contradicts ADR-026 on who owns the OAuth client, and I cannot resolve
it for you.** ADR-026's Context, question 2, decides: "Hive hosts one shared Google Cloud OAuth
client — members click 'Connect' and approve Hive's own app, not BYOK credentials they'd have to
generate themselves... Chosen over BYOK specifically because almost no member would actually do the
Google Cloud Console setup." What shipped is exactly BYOK: `ConnectorsSettingsView.swift:30-45` asks
the member for a Client ID and secret, and `GoogleConnector.swift:60-63` refuses to connect without
them. `GoogleConnector.swift:20-23` says Jack "still needs to create one" — implying a single Hive
client was intended and the fields are a stopgap. **This is a product decision, not a bug to fix in
passing. Flag it to Jack; do not change the credential model this week.** It does not block task 1:
Jack can paste his own client's ID/secret and test the whole round trip today.

**L7 — Test coverage for connectors is zero, and Swift is not in CI at all.**
`apps/desktop-swift/Tests/HiveTests/` contains exactly one file, `BotsModelTests.swift`. Nothing
tests `GoogleAuthManager`, `GoogleKeychain`, `PKCE` or either action method. And `.github/workflows/
ci.yml` has five jobs — `rust`, `pi-footprint`, `migrations`, `web`, `cargo-deny` — and **no Swift
job**; `swift build` and `swift test` never run in CI. So nothing will catch a regression here except
you running it locally. Budget for that: write the pure-function tests that do not need a network or
a live Keychain (PKCE challenge derivation, base64url encoding, and the header-sanitizing helper you
add for L3). Do not try to stand up a full OAuth integration test this week.

**L8 — The OAuth round trip has never been verified against a real Google client.**
`GoogleConnector.swift:19-23` states this plainly: "This has not been build-verified against a real
Google Cloud OAuth client yet." So task 1's acceptance criteria have to include an actual end-to-end
run with Jack's credentials, not just a compiling call site. Expect the first real run to surface a
redirect-URI registration problem — Google requires `http://127.0.0.1` loopback redirects to be
allowed on the client, and the port is random per attempt (`GoogleConnector.swift:70`), which is
correct for Desktop clients but is the classic first-run stumble.

**L9 — Build state, verified.** `swift build` in `apps/desktop-swift` **passes**:
`Build complete! (4.14 sec)`, exit 0, no warnings surfaced. No SwiftPM lock contention; it ran first
try. The working tree is dirty — `git status --porcelain` shows ~20 modified files including
`SettingsView.swift`, `TranscribeView.swift`, `GenerateImageView.swift`, `FeedbackView.swift`,
`ContentView.swift` and `Package.swift` — so **you are building on top of someone's uncommitted
work.** Pull/coordinate before you start editing those five files, or you will collide. I did not
commit, add or modify anything; this doc is the only file I wrote.

---

## Task C-1 — Give the Google connector callers (do this first)

The whole point: turn a connected account from a green checkmark into something that does work.

### C-1a — Hoist `GoogleAuthManager` to app scope

Add `@StateObject private var google = GoogleAuthManager()` in `OHHiveApp.swift` next to `store`
(`OHHiveApp.swift:21`) and `.environmentObject(google)` on every scene that needs it — at minimum
the `WindowGroup` (`OHHiveApp.swift:26-28`) and `Settings` (`OHHiveApp.swift:42-45`). Change
`ConnectorsSettingsView.swift:10` from `@StateObject private var google = GoogleAuthManager()` to
`@EnvironmentObject private var google: GoogleAuthManager`.

**Acceptance criteria**
- `swift build` passes.
- `swift run`: connect Google in Settings, then open a main-window view that shows connector state —
  the state is correct immediately, without relaunching the app.
- Reaching Settings via the sidebar (`ContentView.swift:88`) and via `⌘,` shows the same connection
  state, not two different ones.
- No view anywhere still constructs its own `GoogleAuthManager()`. Grep to prove it: the only hit for
  `GoogleAuthManager()` should be in `OHHiveApp.swift`.

### C-1b — Harden the two action methods before calling them

In `GoogleConnector.swift`: strip `\r` and `\n` from `to` and `subject` inside `sendGmail`
(`:289-297`); reject an empty or obviously-malformed `to`. Check `SecItemAdd`'s `OSStatus` in
`GoogleKeychain.set` (`:392`) and make `connect()` (`:100`) only set `isConnected = true` after
confirming the refresh token reads back.

**Acceptance criteria**
- A unit test in `Tests/HiveTests/` proves `sendGmail`'s sanitizer drops a `\r\nBcc:` payload from
  both `to` and `subject`. Extract the sanitizer as an internal function so it is testable without
  a network call.
- Disconnecting when the Keychain write failed does not leave the UI claiming "Connected."
- `swift test` passes locally. (It will not run in CI — see L7.)

### C-1c — Wire TranscribeView

In `TranscribeView.swift`, extend the `if !transcript.isEmpty` block at `:77-84` from one button to
three: "Copy Transcript" (unchanged), "Save to Drive," "Email Transcript."

- Save to Drive: `try await google.createDriveFile(name: "<source filename>-transcript.txt",
  content: transcript)`. Show the resulting Drive file id or a plain "Saved to Drive" confirmation
  using the existing `SettingsNote` component this view already uses (`TranscribeView.swift:46, 49,
  52`) — do not invent a new notification style.
- Email Transcript: a small sheet asking for the recipient address and letting the member edit the
  subject, then `try await google.sendGmail(...)`. **Never send without the member seeing and
  confirming the recipient.** ADR-026's open questions flag autonomous sending as the thing that
  needs its own decision; a member typing an address and pressing Send is not that.
- Both buttons hidden (not just disabled) when `!google.isConnected`, with a one-line "Connect
  Google in Settings to save or email transcripts." Match the tone of the existing copy —
  `ConnectorsSettingsView.swift:18` is the register to write in.
- Errors surface through `SettingsNote` the same way `networkError` does at `TranscribeView.swift:52`.
  `GoogleConnectorError` already has good member-facing strings (`GoogleConnector.swift:320-327`) —
  use `String(describing:)` on it, which hits `CustomStringConvertible`, not `localizedDescription`.

**Acceptance criteria**
- With Google disconnected, TranscribeView looks and behaves exactly as it does today plus one line
  of explanatory copy. No dead buttons.
- With Google connected, transcribing a short audio file and pressing "Save to Drive" produces a
  real file in the member's Drive, and the view reports success with the file id or a clear message.
- Pressing "Email Transcript" opens a confirmation step; the message arrives; the transcript body is
  intact including any non-ASCII characters.
- A failure (revoked token, no network) shows the `GoogleConnectorError` text and leaves the
  transcript on screen, not a blank view.
- Both the on-device and whisper.cpp sources work, since `transcript` (`TranscribeView.swift:27`)
  already abstracts over them. Test at least one of each.

### C-1d — Wire ChatView

A `.toolbar` item on `ChatView` (`ChatView.swift:35-51` is where the view's modifiers already sit),
gated on `google.isConnected && !engine.messages.isEmpty`. Render `engine.messages`
(`ChatEngine.swift:75`) to markdown — role heading, then text — and call `createDriveFile` with
`name: "\(engine.sessionTitle).md"` and `mimeType: "text/markdown"`.

**Acceptance criteria**
- Saving a chat with 20+ messages produces a readable markdown file in Drive with every message in
  order and roles distinguishable.
- The toolbar item does not appear on an empty chat, and does not appear when Google is not
  connected.
- Nothing about `ChatEngine`'s tool surface changes. `NodeStatusTool` (`ChatEngine.swift:50`) is
  still the only tool the model can call. Verify by grep: no Drive or Gmail symbol appears inside
  `ChatEngine.swift`.

### C-1e — Optional, only if C-1a–d are done and verified: GenerateImageView

Add `func createDriveFile(name: String, mimeType: String, data: Data) async throws -> String`
alongside the existing one at `GoogleConnector.swift:263`, sharing the multipart body construction.
Keep `result.filePath` (`GenerateImageView.swift:110`) in `@State` so there are bytes to upload, and
add a "Save to Drive" button next to the generated image.

**Acceptance criteria**
- The existing `String` overload is unchanged and its callers still compile.
- A generated PNG round-trips: the file in Drive opens and is byte-identical to the local file.
- `mimeType` is set correctly from the actual file, not hardcoded.

---

## Task C-2 — A GitHub connector, reusing the Google pattern

Second provider, chosen because it is the one with an obvious Hive use (a card, a bug report, a
repo) and because it proves the pattern generalizes before anyone tries to generalize it wrongly.

### Read this before you start: the pattern to copy is *not* what the brief says

The brief I was given, and ADR-026's own Consequences section, both say the reusable pattern is
"Desktop OAuth client, `ASWebAuthenticationSession`, Keychain storage." **The first two thirds of
that is wrong about the code as it exists.** There is no `ASWebAuthenticationSession` in
`apps/desktop-swift` — I grepped the whole app. What actually shipped, and what you should copy, is
documented in `GoogleConnector.swift:12-23`: system browser plus one-shot loopback listener.

Copy these specific pieces, by name and line:

| Copy this | From | Why it generalizes |
|---|---|---|
| `startLoopbackListener()` | `GoogleConnector.swift:170-189` | Provider-agnostic. `NWParameters.tcp` with `requiredInterfaceType = .loopback` — never binds anything but localhost. |
| `waitForCallback(listener:expectedState:)` | `GoogleConnector.swift:191-226` | Provider-agnostic, including the CSRF `state` check at `:209` and the 180 s abandon at `:222`. |
| `readRequest(_:completion:)` | `GoogleConnector.swift:234-258` | Provider-agnostic. Note the `nonisolated` and why, at `:229-233` — do not remove it, this toolchain's actor checking requires it. |
| `ResumeOnce<T>` | `GoogleConnector.swift:330-350` | Required. The timeout and the connection handler race for the same continuation. |
| `struct PKCE` | `GoogleConnector.swift:352-364` | S256. GitHub supports PKCE on OAuth apps; use it. |
| `Data.base64URLEncodedString()` | `GoogleConnector.swift:369-376` | Already a free extension on `Data`. |
| `GoogleKeychain`'s five static methods | `GoogleConnector.swift:381-422` | Copy the shape, **change the `service` string** at `:382`. Do not share `media.happyjack.hive.google` across providers — one provider's `removeAll()` must not wipe another's tokens. |
| `checkOK(_:_:)` and `formEncode(_:)` | `GoogleConnector.swift:160-168` and `:152-159` | Generic HTTP/form helpers. |
| The settings card's honest-capability copy | `ConnectorsSettingsView.swift:16-19` | The house voice for "here's exactly what this can and can't do." Match it. |

Do **not** copy: `GoogleAuthManager.scopes` (`:35`), `exchangeCode`'s Google endpoint (`:109`),
`validAccessToken`'s refresh endpoint (`:138`), or anything mentioning Drive/Gmail.

### Honest uncertainty on how far to abstract

ADR-026's Path A describes a `ConnectorProvider` config struct (client id, authorize/token URLs,
scopes, PKCE requirement) plus one generic flow. That is the right end state. **I do not think you
should build it this week.** With one provider you cannot tell which parts are general; with two you
can. My recommendation: write `GitHubAuthManager` as a deliberate near-copy of `GoogleAuthManager`,
note the duplication in a comment, ship it, and *then* extract the common flow in a third pass with
both real cases in front of you. If you disagree after reading the code, say so — you are in the
file and I am not. What I am confident about is that extracting an abstraction from exactly one
example before Friday is how we get a wrong abstraction we then have to live with.

### GitHub specifics I am reasonably but not fully confident about

Verify each against GitHub's current docs before you rely on it — my knowledge here is not
first-hand from this repo:

- Endpoints: authorize at `https://github.com/login/oauth/authorize`, token at
  `https://github.com/login/oauth/access_token`. The token endpoint returns form-encoded by default;
  send `Accept: application/json` to get JSON, or you will be parsing `access_token=...&scope=...`.
- A loopback `http://127.0.0.1:<port>/callback` redirect works, but GitHub matches the registered
  callback URL including the port for OAuth Apps. Random ports may not be accepted the way Google
  accepts them. **This is the highest-risk unknown in C-2** — check it first, before writing the
  rest. If GitHub requires a fixed port, bind that specific port instead of `.any`
  (`GoogleConnector.swift:172`) and handle "port already in use" with a clear error.
- Classic OAuth App tokens historically did not expire and had no refresh token. GitHub Apps
  (user-to-server) issue expiring tokens with refresh. Decide which you are building against and
  write it in the file header the way `GoogleConnector.swift:12-23` does. If tokens do not expire,
  `validAccessToken()` collapses to a Keychain read — say so explicitly rather than leaving dead
  refresh code.
- Scopes: start as narrow as `drive.file`/`gmail.send` were. `public_repo` or `repo` only if a
  named feature needs it. Do not request `admin:*` or `delete_repo` for any reason.
- GitHub OAuth Apps have a client secret. Same non-confidential-in-a-desktop-app caveat applies as
  `GoogleConnector.swift:20-23` describes, and the same unresolved product question from L6 applies:
  who owns the client. **Follow whatever Jack decides for Google; do not introduce a second,
  different credential model.**

### C-2 acceptance criteria

- A second GroupBox in `ConnectorsSettingsView` (`ConnectorsSettingsView.swift:15` is where the
  Google one starts), with the same connected/not, Connect/Disconnect, honest-capability shape. The
  "More connectors (providers 3+)" footer line at `ConnectorsSettingsView.swift:65` gets updated,
  not left contradicting the UI above it.
- A full OAuth round trip completes against a real GitHub client, and a token lands in the Keychain
  under a **different service string** from Google's.
- Disconnecting GitHub leaves Google connected, and vice versa. Test this explicitly — it is the
  bug the shared-service-string mistake produces.
- At least one real action exists and is called from a view. A connector with no callers is the
  exact problem this queue exists to fix; do not create a second one. My suggestion, lowest risk:
  "Open a GitHub issue from a bug report" in `FeedbackView`'s `BugReportForm`
  (`FeedbackView.swift:88`) — but confirm with Jack that he wants Hive bug reports mirrored to a
  repo before building it, because that is a product decision and I am inferring it.
- `swift build` passes. Unit tests for anything pure you added.
- The file carries a header comment in the same register as `GoogleConnector.swift:7-27`: what ADR
  it implements, what was decided, and what has *not* been verified yet. That header is the reason
  I could write this document quickly. Keep the practice.

---

## Non-goals — explicitly out of scope this week

- **No model-callable Drive/Gmail tools.** `ChatEngine` and `FeedbackAssistant` do not get
  tool-calling access to a connected account. ADR-026 Decision 5 says this is a "natural
  fast-follow, not part of this ADR's v1," and the open-questions section says the confirmation
  model is undecided. Every connector action in C-1 and C-2 is a button a human presses.

- **No scope widening.** `drive.file` and `gmail.send` stay exactly as they are at
  `GoogleConnector.swift:35`. ADR-026 Decision 3 is the reason: the tier is set by the most
  sensitive scope requested, and adding `gmail.readonly`, `gmail.modify` or full `drive` puts Hive
  into Google's CASA Tier 2 assessment — weeks, plus mandatory annual re-verification. That is a
  Jack decision driven by a real feature, not something to reach by accident.

- **No Google file picker.** ADR-026 Decision 4 defers it. `createDriveFile` creating a new file is
  the whole v1 Drive story.

- **No cross-machine token sync, no broker service, no Nango.** ADR-026's 2026-09-14 amendment
  reversed the Nango decision to Path A, custom in-house. Per-machine Keychain (L5) is the
  intended behavior. Do not build infrastructure to work around it.

- **No card/project-facing connectors.** ADR-026 Decision 1 explicitly scopes this to the Swift app
  and says a card's `required_capabilities` gaining a connector option "needs its own ADR extending
  ADR-023's ownership/trust model." Do not touch `required_capabilities`.

- **No generic `ConnectorProvider` abstraction yet.** See the reasoning under C-2. Two concrete
  implementations first.

- **No changes to the BYOK-vs-shared-client credential model** (L6) without Jack. Flag it, move on.

- **No refactor of `SettingsView.swift`.** It is a 14-tab junk drawer and the audit says so, but that
  is a separate queued item and it is one of the files with uncommitted changes right now (L9).
  Touch only lines 101-102 if you must.

---

## Suggested order given Friday 13:00

1. C-1a (hoist) and C-1b (harden) — both small, both block everything else.
2. C-1c (TranscribeView) — the demo. If only one thing ships, ship this one. It is the shortest path
   from "we built an OAuth engine" to "watch me transcribe this and put it in my Drive."
3. C-1d (ChatView) — makes the sentence already in the settings UI true.
4. C-2's riskiest unknown only: verify GitHub's loopback-redirect and port behavior. Knowing that
   answer is worth more before Friday than half a GitHub connector.
5. C-1e and the rest of C-2 if time remains.

## What I did not check

Stated plainly so you do not assume coverage I do not have:

- I did not run the app. `swift build` passes; I have not launched `swift run` or clicked anything.
- I did not test the OAuth round trip. Nobody has (L8).
- I did not read `HiveStore.swift`, `BotsModel.swift` or `EarningsView.swift` in full — my BotsView
  and EarningsView notes are from headers and greps, and are marked as weak fits for that reason.
- I did not verify any GitHub API claim against GitHub's live docs. Everything in the GitHub
  specifics list needs your confirmation.
- I did not read the 25-connector/25-MCP-server ranking doc that preceded this; my "cheapest
  high-value work" conclusion comes from the code, and it agrees with that doc's finding.

— Loki

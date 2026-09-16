# Loki → Sif: split work queue, 2026-09-16 (afternoon)

Jack asked us to split the remaining blockers and work them in parallel. This is your lane.
Mine is at the bottom so you know what *not* to pick up.

**Read `docs/CONTINUITY.md`, newest entry ("Loki registers both OAuth apps") first** — it has
both client IDs and the four gotchas. This doc is the split, not the context.

---

## The one hard constraint: we share a checkout

`OH Cloud-src` on this disk is the *same working tree* for both of us. When I looked just now
it held 15 uncommitted files of yours, including `SharedConnectorConfiguration.swift`.

So the rule for today: **you own everything under `apps/desktop-swift/`, `crates/`, and
`apps/desktop/src-tauri/`. I do not touch them.** I own `.github/workflows/` and `docs/`.
If you need something in my lane, say so rather than editing it — a silent concurrent edit in
a shared tree is how work gets lost.

I also left a stale `.git/index.lock` in this repo earlier today by running `git status`
without delete permission. I have cleaned it up and deletion is now enabled for my session. If
you ever hit `Unable to create '.git/index.lock': File exists` and no git process is running,
that was me — delete it and carry on.

---

## Your queue, in order. Nothing here is blocked.

### 1. HIGH — Wire the Google client ID and give the connector real callers

```
HIVE_GOOGLE_OAUTH_CLIENT_ID=372275843401-p7nbgvkjhcspbcg0kknlb1i55kug8dgf.apps.googleusercontent.com
```

This is the registration you asked for in your 2026-09-16 entry. It is done, along with
GitHub:

```
HIVE_GITHUB_OAUTH_CLIENT_ID=Iv23libERbZ9QJevTgu2
```

Both are public build configuration. Put them where you already put the GitHub one.

The actual work: the 2026-09-15/16 audit found `GoogleConnector.swift`,
`ConnectorsSettingsView.swift` and the Connectors tab all built and build-verified, with
`createDriveFile` and `sendGmail` having **no callers**. The OAuth plumbing was never the gap.
ADR-026 is one step from finished.

Done looks like: both client IDs in `SharedConnectorConfiguration` from build config, not
hardcoded twice; at least one real user-facing path invoking each of `createDriveFile` and
`sendGmail`; and a live round-trip against jack@happyjack.media — consent screen reads
"Loki's Den", token lands in Keychain, a file actually appears in Drive, a mail actually sends.

**Four things that will otherwise eat your afternoon:**

- The Google app is in **Testing** publishing status. Refresh tokens expire after **7 days**
  and only listed test users can connect. That is Google's testing-mode behavior, **not a
  defect in the connector.** Do not debug it as one.
- A Google client secret exists and **Jack holds it.** It must not enter the repo or the
  binary. Desktop-app clients authenticate with PKCE; Google's own docs say the installed-app
  secret "is obviously not treated as a secret." Code that depends on it quietly undoes
  ADR-036 decision 1.
- Stay on `drive.file` and `gmail.send`. `gmail.readonly` and full `drive` are restricted
  scopes that trigger a CASA assessment — a separately decided v2 per ADR-026 §3.
- Do **not** generate a GitHub private key. The settings page nags for one; that nag is about
  installation/JWT auth, which we do not use. Device flow returns a user access token
  directly, and a generated private key is a credential with nothing guarding it.

### 2. HIGH — Compiler-verify the two modules that never have been

While your toolchain is up:

```
cargo check -p hive-core --features <flag>   # subscription/  (ADR-033 Stage 1)
cargo check -p hive-core --features <flag>   # bots/          (ADR-035 C0)
```

Both are still **self-verified only** — no compiler has ever seen them. I would rather your
compiler find the problem than a user. This is cheap and I have no way to do it: the Linux VM
I reach this disk through has no Rust toolchain at all, and the cloud sandbox has no checkout.

Precedent for why this matters: an `@Published` on an `@Observable` class passed
`swiftc -parse` and only a real `swift build` caught it. Parse is not a build.

### 3. HIGH — `cargo fmt --all`, but **only after you commit**

Repo-wide `cargo fmt` is still keeping CI red (`cargo fmt --all -- --check` is a hard gate in
`ci.yml`). It is a one-shot mechanical fix and it belongs to whoever has cargo — that is you.

**Sequence matters.** Commit your 15 files first, *then* run fmt as its own commit. Running it
while your work is uncommitted mixes a thousand whitespace lines into a real diff and makes
your changes unreviewable.

### 4. MEDIUM — ADR-034 P0, Codex Stage 1 real acceptance

Already yours, already unblocked — your host has cargo 1.98.1 and codex-cli 0.149.0.

### 5. MEDIUM — ADR-034 P2, Copilot adapter conformance spike

Real-account testing is now possible, not just the SDK-compat question. Two constraints:
the App is `private_visibility` (installable only on @jackcanon — installing anywhere else
needs it flipped public, which is Jack's call, not a thing to flip); and there is no client
secret and no private key by design. If the Copilot SDK path appears to demand either, that
means the adapter is on the *installation-auth* path rather than the *user-auth* path —
flag it rather than generating credentials.

---

## My lane — do not pick these up

- `.github/workflows/ci.yml`: adding the Swift job. There is currently **no Swift anywhere in
  CI** — not in any workflow, not in any script. Your 21 Swift tests have never run on a
  machine that wasn't yours. That is the hole the `@Published` bug came through.
- Committing your 15 uncommitted files and today's continuity entries.
- Writing the model-fit gate spec (nodes still advertise models larger than their own RAM —
  the root cause of the 27B-on-16GB thrash on Jotunheim). I will spec it; **implementation
  will be yours**, because it needs a compiler.

## Jack's lane — neither of us can do these

- Upgrading Heimdall and chicago-hive off 0.3.0. Both my environments are cut off from the
  LAN: the VM that reaches this disk has no network at all (no DNS, no SSH), and the cloud
  sandbox cannot see 192.168.x.x. Needs Jack or a machine with LAN access.
- Deploying the S-1 migration baseline — needs Jack and a production window.
- "Agents visible in the Den" is still gated on that deployment. It is Jack's stated goal and
  it has now slipped twice; it is not a code problem.

---

Loki

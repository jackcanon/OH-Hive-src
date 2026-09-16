# Windows bring-up: compiling the Den on the Windows machine

**For:** Jack, 2026-09-17. **Author:** Loki (Claude Opus 5), written 2026-09-16 ~06:00.
**Confirmed:** Jack has a Windows machine and SSH into it is available.
**Deadline:** functioning and tested before Friday 2026-09-18 13:00.

Every command below is self-contained, including its own `cd`. Nothing assumes a previous command
ran in the same shell. PowerShell unless marked otherwise.

---

## What CI already told us overnight, so you don't repeat it

`hive-core` **compiles and passes its full test suite on Windows** (`x86_64-pc-windows-msvc`) with
the exact feature set the desktop app uses — `subscription-coordinator, hub, probe, llama-cpp,
sandbox, setup, tunnel, bots, local-hub` — and the `hive` CLI and `hive-server` build there too.
Same for Linux. **Nothing in the shared core is Mac-bound.** That question is closed.

What is *not* closed is the Tauri shell. The Linux bundle failed; the Windows bundle was still
building when I wrote this. Check `Actions → desktop (Windows + Linux)` for the verdict — the job
prints an explicit notice either way, and on failure it says the gap is in the shell rather than the
core.

### The first bundle blocker is found and fixed — don't spend tomorrow on it

The Linux bundle failure was one line, and it would have hit Windows identically:

```
resource path `resources/cloudflared-aarch64-apple-darwin` doesn't exist
```

`tauri.conf.json` declared a **hardcoded macOS-ARM `cloudflared` binary** as a required bundle
resource, while `build.rs` deliberately fetches it only for `apple-darwin` targets (correctly — it
was written macOS-only per ADR-010). So on Windows and Linux the bundler demanded a file nothing
creates. A config bug, not a portability problem in our code.

Fixed by inverting the default rather than overriding it: `bundle.resources` moved out of the base
config into a new `tauri.macos.conf.json`, which Tauri merges over the base **on macOS only**. macOS
behaviour is byte-identical; Windows and Linux now have nothing to override. `bundle.targets` also
went from the hardcoded `["app", "dmg"]` to `"all"`, so each host picks the bundle types valid for it.

I verified the merge is genuinely read rather than assuming it — the failure mode if Tauri ignored
that file would be a Mac app silently shipping *without* cloudflared, which is exactly the class of
bug this session was already about. Proof: I pointed the macOS config at a deliberately nonexistent
path, confirmed the build script failed with `resource path resources/PROVE-THE-MERGE-IS-READ doesn't
exist`, then restored the real path and confirmed `cargo check -p ohhive-desktop` passes clean.

`icons/icon.ico` exists, so the Windows icon is not a blocker either. **Expect a different, later
error than the one above** — that one is gone. If the Windows bundle still fails, it will be a new
finding worth reporting rather than this known one.

---

## Two paths, and I'd do both in this order

**Path A — install the CI-built installer. 10 minutes, no toolchain.** Proves the app *runs* on
Windows, which is a different question from whether it compiles, and it is the fastest way to learn
something real. Go to `Actions → desktop (Windows + Linux)` → newest run → download the
`den-windows` artifact → unzip → run the `.msi`. SmartScreen will warn because it is unsigned;
that is expected, choose "More info → Run anyway".

If that artifact does not exist, the bundle step failed and Path B is the only route.

**Path B — compile on the box.** What you asked for. It is also the only way to iterate on bundle
config without a 20-minute CI round trip per attempt.

---

## Step 1 — SSH in (do this first; it makes everything after it easier)

**On the Windows machine, once, in an Administrator PowerShell:**

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Set-Service -Name sshd -StartupType Automatic
Start-Service sshd
Get-NetFirewallRule -Name *ssh* | Select-Object Name, Enabled
```

Then get its address and user, still on the Windows machine:

```powershell
$env:USERNAME
(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '169.*' -and $_.IPAddress -ne '127.0.0.1' }).IPAddress
```

**From your Mac** (fresh Terminal), substituting the user and address you just got:

```bash
ssh USER@ADDRESS powershell -Command '"$env:COMPUTERNAME; [System.Environment]::OSVersion.Version"'
```

Once that answers, **I can drive the Windows box myself**: your Mac can reach your LAN, so I can run
the whole compile ladder over SSH from there and hand you the results, rather than you pasting each
command. Say the word and give me the user and address. (I cannot reach it directly — this session's
cloud container has no route to your network. It has to go through your Mac.)

---

## Step 2 — prerequisites on Windows

One Administrator PowerShell block. The C++ build tools are the part people forget: the MSVC linker
is required for the `msvc` Rust target, and `wasmtime`/`rusqlite`/`zstd` in our tree all build native
code.

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --accept-source-agreements --accept-package-agreements --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup --accept-source-agreements --accept-package-agreements
winget install --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
winget install --id Git.Git --accept-source-agreements --accept-package-agreements
```

Then, in a **new** PowerShell (so the PATH changes take effect):

```powershell
rustup default stable-x86_64-pc-windows-msvc; rustup show; node --version; npm install -g pnpm; pnpm --version; git --version
```

WebView2 is already present on Windows 11 and on current Windows 10; if the Tauri build complains
about it, `winget install --id Microsoft.EdgeWebView2Runtime`.

**A note on the repo:** our toolchain is pinned by `rust-toolchain.toml`, so `rustup` will fetch the
pinned version on first build rather than using whatever `stable` is that day. Expect the first
command in the repo to sit for a minute doing that.

---

## Step 3 — get the source

Substitute a path you actually want; everything after this assumes `C:\src\OH-Hive-src`.

```powershell
cd C:\; New-Item -ItemType Directory -Force -Path C:\src | Out-Null; cd C:\src; git clone https://github.com/jackcanon/OH-Hive-src.git; cd C:\src\OH-Hive-src; git log --oneline -3
```

---

## Step 4 — the compile ladder, cheapest first

Run these in order and stop at the first one that fails — each tells you something different, and a
failure at step 4.1 means something quite different from a failure at 4.4.

**4.1 — does the core compile?** (5–15 min cold; this is the big one and CI says it passes)

```powershell
cd C:\src\OH-Hive-src; cargo check -p hive-core --features subscription-coordinator,hub,probe,llama-cpp,sandbox,setup,tunnel,bots,local-hub
```

**4.2 — do its tests pass on Windows?** (CI says yes — this confirms it on *your* machine, which is
not the same claim: CI runners differ from real hardware in exactly the ways SQLite and filesystem
tests care about)

```powershell
cd C:\src\OH-Hive-src; cargo test -p hive-core --features subscription-coordinator,hub,probe,llama-cpp,sandbox,setup,tunnel,bots,local-hub
```

**4.3 — CLI and server** (these already ship for Windows from `release.yml`, so this should be
boring)

```powershell
cd C:\src\OH-Hive-src; cargo build --release -p hive -p hive-server
```

Then prove the CLI actually runs, which is more than compiling:

```powershell
cd C:\src\OH-Hive-src; .\target\release\hive.exe --help
```

**4.4 — the desktop app.** This is where I expect trouble, per the `bundle.targets` note above.

```powershell
cd C:\src\OH-Hive-src; pnpm install --frozen-lockfile
```

```powershell
cd C:\src\OH-Hive-src; pnpm --filter @hive/desktop tauri build --bundles msi
```

If the MSI is refused, try NSIS, which is more forgiving about missing config:

```powershell
cd C:\src\OH-Hive-src; pnpm --filter @hive/desktop tauri build --bundles nsis
```

And to separate "the Rust side builds" from "the bundler is misconfigured" — worth knowing, because
they lead to completely different fixes:

```powershell
cd C:\src\OH-Hive-src; cargo build --release -p ohhive-desktop
```

If *that* succeeds and only the bundling fails, the app compiles on Windows and the remaining work is
purely packaging config. That would be a good outcome.

---

## Step 5 — what to actually test once it launches

Compiling is not testing. In dependency order, because each step depends on the one before:

1. **Launch** — app starts, tray icon appears, no missing-DLL dialog.
2. **First-run Setup** — the hardware probe reports real CPU/RAM/disk. This is the single most
   likely wrong answer on Windows: `probe.rs` holds the only `target_os = "macos"` branch in the
   whole core, so it is the one place with a known platform fork.
3. **Sign in** against the live hub from a non-Mac client.
4. **Pair as a compute node** — expect this to be the second thing that breaks. Credential storage
   is per-platform (Keychain on macOS, Credential Manager on Windows), and nothing has ever
   exercised the Windows path.
5. **Run a card** end to end in local mode — this is what exercises wasmtime and the local SQLite
   hub on a new platform.
6. **Team chat** — send and receive against the hub.
7. **Server tab / tunnel** — Cloudflare tunnel automation is shell-scripted in places. Treat a
   failure here as expected, not alarming.
8. **Uninstall, then reinstall** — leaves no broken state.

Pass / fail / not-applicable and one line each. Anything that fails becomes a card, not a
conversation.

---

## Linux, same timeline

Same core answer (CI: compiles and tests pass). The Linux bundle failed overnight; the `.deb` and
`.AppImage` need the webkit2gtk toolchain, which the CI job installs and which any Ubuntu box would
need too:

```bash
sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev patchelf
```

If you have no Linux box, a VM or WSL2 on the Windows machine covers it — though WSL2 needs an X
server for the GUI, so for the *bundle* question a real VM is less trouble.

---

## What I'd expect by Friday, honestly

- **Very likely:** core compiles and tests pass on Windows on your own hardware (CI already did it),
  CLI and server build and run.
- **Likely:** `ohhive-desktop` compiles on Windows, and the remaining work is bundle configuration —
  hours, not days.
- **Less likely but possible:** the app launches and Setup/Sign-in work, with pairing or card
  execution as the first real break.
- **Not happening by Friday, and no route makes it happen:** feature parity with the Swift Den. The
  Tauri app is 7 React files to the Swift app's 41. See
  `LOKI-DEN-WINDOWS-LINUX-PLAN-2026-09-16.md` for the four options and the decision that is yours.

## Related

`docs/LOKI-DEN-WINDOWS-LINUX-PLAN-2026-09-16.md` (parity table, four costed options),
`.github/workflows/desktop-windows-linux.yml` (the CI job, dispatchable on demand), ADR-018 (the
platform split that makes Tauri the Windows/Linux shell), ADR-010 (original Tauri shell decision).

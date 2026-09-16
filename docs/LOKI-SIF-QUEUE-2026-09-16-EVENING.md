# Loki → Sif: refreshed queue, 2026-09-16 evening

Supersedes the priority order in `LOKI-SIF-SPLIT-QUEUE-2026-09-16.md`. The file-ownership rule
in that doc still stands: **you own `apps/desktop-swift/`, `crates/`, `apps/desktop/src-tauri/`;
I own `.github/workflows/` and `docs/`.**

---

## First: you were right, twice, and I was wrong twice

**Google client_secret.** My handoff said flatly that the secret must never enter the binary.
Your live test disproved it — the token endpoint returns `invalid_request / client_secret is
missing`. I reasoned "PKCE exists, therefore no secret needed," which is the *same* mistake I
made about GitHub earlier today, where you also caught me. PKCE and client authentication are
separate concerns and every vendor wires them differently. You were right to stop and escalate
rather than paste a secret in to clear the error.

I went looking for an escape hatch and there isn't one: I checked Google's limited-input device
flow specifically, since device flow is what saved us on GitHub. It requires the client secret
*too*, and it has **no Gmail scope at all** (it does support `drive.file`). So every Google
installed-app flow needs the secret. Full analysis and my recommendation:
`docs/LOKI-GOOGLE-CLIENT-SECRET-DECISION-2026-09-16.md`. **Jack has now ruled — Option A. See
item 1 below.**

**The "no callers" finding.** You were right that it was stale — `GoogleTextActions` and the
chat/transcript exports already call Drive/Gmail. I passed on an audit finding from 2026-09-15
without re-checking it against the code. Sorry for the wasted look.

---

## Queue, in order

### 1. UNBLOCKED — Jack ruled: **Option A. Ship the installed-app credential.**

Decided 2026-09-16. ADR-036 now carries an amendment saying what the rule actually is, so this
is not a quiet break of Decision 1 — read
`ADR/ADR-036-git-workspaces-and-github-workflows.md`, the 2026-09-16 amendment at the bottom.

**The rule, per vendor:**
- **GitHub** — device flow, no client secret, no private key. Unchanged.
- **Google** — installed-app client ID **and** client secret ship in the binary as **build
  configuration**, in `apps/desktop-swift/config/oauth-clients.sh` where you already put the
  public IDs. PKCE retained.

**Jack holds the secret. Ask him for it directly — do not request it in chat here and do not
paste it into any doc, commit message, or continuity entry.**

Constraints that come with the ruling:

- Treat both values as **build config and name them that way.** They do not go in Keychain, a
  secret store, or a `.env` that implies confidentiality. Mislabelling them would train the next
  person to treat a real secret the same way.
- Keep the empty-value override behaviour you already built — an explicitly empty value should
  still disable that connector.
- Scopes unchanged: `drive.file` and `gmail.send`. `gmail.readonly` and full `drive` remain a
  separately decided v2 with a CASA assessment.
- Your corrected callback HTML — the one that no longer claims success before token exchange —
  is right and should stay. The old page lied to the user about a connection that hadn't
  happened yet.

Once it's wired, item 6 (the approved one-file / one-email live test) unblocks too.

### 2. HIGH — `managed_login_correlation_visibility_cancel_and_logout` fails on CI
Cmd Work `9440cd80`. New: **CI now runs the Swift app and the full Rust suite**, and this is one
of two real failures it surfaced that the fmt noise was hiding.

```
panicked at crates/ohhive-core/src/subscription/account.rs:459:14:
called `Result::unwrap()` on an `Err` value: "Codex version check timed out"
```

242 passed, 1 failed. The test shells out to a real `codex` binary — yours has codex-cli
0.149.0, a GitHub runner has none, the check hangs, and the `unwrap` turns a timeout into a
panic.

**Look at the production path before you fix the test.** If that `unwrap` is reachable outside
tests, a wedged codex binary can panic a node because an *optional* adapter is stuck. That would
be a real product bug wearing a test failure as a disguise. If it isn't reachable, `#[ignore]`
plus running it in the adapter-acceptance path is the cheap correct answer.

### 3. HIGH — repo-wide `cargo fmt`
Still red on `rust (ubuntu-latest)`. You now have commits landed, so the sequencing constraint
from the earlier queue is satisfied: run it as **its own commit**, nothing else in it.

### 4. HIGH — model-fit gate
Cmd Work `7be35ffa`, spec at `docs/LOKI-MODEL-FIT-GATE-SPEC-2026-09-16.md`.

**Heimdall is now a second, better test case than Jotunheim.** It joined the fleet today:
Ryzen 9 5900X, 67 GB RAM, but an **RTX 4070 with only 12.9 GB VRAM** — and it advertises
`qwen3.8:27b`. With 67 GB of system RAM that model *completes* instead of dying, so it silently
falls back to CPU while still advertising itself as GPU-capable. Jotunheim showed this bug
failing loudly; Heimdall shows it failing quietly, which is worse because nothing logs it. The
gate must branch on `vram_bytes` where present, not `ram_bytes` — a rule written only against
Jotunheim would pass Heimdall and keep the silent failure.

### 5. MEDIUM — Windows clippy
Cmd Work `6e7862ba`. `variable does not need to be mutable` in `hive-core`, Windows-only, fatal
under `-D warnings`. Almost certainly a `let mut` mutated only in a non-Windows cfg branch.

Worth more than its size: per the `fail-fast: false` comment in `ci.yml`, the Windows leg was
being cancelled mid-build for weeks, so this may be **its first honest verdict**. With the MSI
test and Friday's deadline, a green Windows leg is worth having.

### 6. MEDIUM — the approved Drive/email live test
Jack approved one labeled Drive file and one email to jack@happyjack.media. Blocked behind
item 1.

### 7. MEDIUM — ADR-034 P2 Copilot adapter spike
Unchanged; real-account testing available now.

---

## What I've done that touches your world

- `44bdea9` + `559bb2f`: the Swift CI job. My first version was broken — it died on `cp` after
  an 11-minute build because `Sources/OHHiveFFI/` is gitignored and doesn't exist in a clean
  checkout. Fixed with `mkdir -p`. I also deleted a "bindings match the ones committed" step I'd
  written that could never fail, because those paths are gitignored: a green tick that proves
  nothing is worse than no check.
- `08992ef`: committed your 15 in-flight files on your behalf while you were idle, attribution
  yours, verification recorded as *yours* — I did not re-run any of it.
- Fleet: chicago-hive upgraded v0.3.0 → v0.4.0; heimdall's worker running. Also found that
  **release v0.4.0 was tagged with `Cargo.toml` still saying 0.3.0**, so every v0.4.0 binary
  reports `hive 0.3.0` and `--version` can't tell the two apart. That's what made heimdall look
  stale when it wasn't.

Loki

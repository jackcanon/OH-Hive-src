# CI run 35134981897 — every failure, separated

**Loki, 2026-09-16.** First run of the new Swift job, on commit `08992ef`.
Five jobs failed. They are five *different* problems and lumping them into "CI is red"
is how the fmt debt has hidden real failures for weeks.

| job | failing step | whose |
|---|---|---|
| `desktop (Swift, macOS)` | build hive-ffi + generate bindings | **mine — fixed** |
| `rust (macos-latest)` | build + test | **Sif** |
| `rust (windows-latest)` | clippy | **Sif** |
| `rust (ubuntu-latest)` | `cargo fmt --all -- --check` | Sif, already queued |
| `cargo-deny` | `cargo deny check` | Jack — `continue-on-error` by design |

---

## 1. Swift job — my bug, fixed

```
cp: apps/desktop-swift/Sources/OHHiveFFI/ohhive_ffi.swift: No such file or directory
```

The Rust build and uniffi bindgen both **succeeded** — eleven minutes of compile, then a
one-line failure on `cp`. `Sources/OHHiveFFI/` contains only generated output, and
`.gitignore` lines 39–40 exclude both generated files, so **the directory does not exist in a
clean checkout.** `build-app.sh` never hits this because on any machine that has built before,
the directory is already there. Fixed with `mkdir -p` on both destinations.

**I also deleted a check I had written that was worthless.** The job ended with:

```yaml
- name: generated bindings match the ones committed
  run: git diff --exit-code -- apps/desktop-swift/Sources/OHHiveFFI apps/desktop-swift/Sources/ohhive_ffiFFI
```

Those paths are gitignored *on purpose*. `git diff` over them can never report a change, so
that step would have passed forever while proving nothing — a green check that means nothing is
worse than no check, because it gets trusted. Removed, with the reasoning left in the file so
nobody re-adds it. The real guarantee is `swift build`: it compiles freshly generated bindings
against the committed `module.modulemap`, so Rust/Swift drift fails there for real.

---

## 2. `rust (macos-latest)` — a real test failure, and the most interesting one here

```
test subscription::account::tests::managed_login_correlation_visibility_cancel_and_logout ... FAILED
panicked at crates/ohhive-core/src/subscription/account.rs:459:14:
called `Result::unwrap()` on an `Err` value: "Codex version check timed out"
```

242 passed, 1 failed. **This test shells out to a real `codex` binary.** Sif's host has
codex-cli 0.149.0, so it passes for her; a GitHub runner has no codex, the version check hangs,
and the `unwrap` turns a timeout into a panic.

This is a genuine "works on my machine" test, and it is worth fixing properly rather than
pinning: a unit test in `ohhive-core` should not depend on an external binary being installed.
Options, in my order of preference:

1. `#[ignore]` it and run it in the adapter-acceptance path where a real codex is a stated
   prerequisite — matches how the Codex handshake test is already handled.
2. Inject the version-check as a trait/closure so the test can supply a fake.
3. Treat a timeout as a non-fatal "codex unavailable" rather than `unwrap`-ing — arguably the
   production code is wrong too, since a hung codex should not panic a node.

Option 3 may be a real product bug hiding behind a test failure. Worth a look before choosing 1.

---

## 3. `rust (windows-latest)` — clippy, one line

```
error: variable does not need to be mutable
error: could not compile `hive-core` (lib) due to 1 previous error
```

Windows-only, so it is almost certainly a `let mut` that is only mutated inside a
`#[cfg(unix)]`/non-Windows branch. `-D warnings` makes it fatal. Small, but it means **the
Windows leg has not been green**, and per the `fail-fast: false` comment in `ci.yml` that leg
was being silently cancelled for weeks before — so this may be its first honest verdict.

---

## 4. `rust (ubuntu-latest)` — the fmt debt

`cargo fmt --all -- --check` against a large diff across the Bots/conversation code. Already
queued in `docs/LOKI-SIF-SPLIT-QUEUE-2026-09-16.md` item 3, **to be run as its own commit after
Sif's work is committed** so a thousand whitespace lines do not bury a real diff.

---

## 5. `cargo-deny` — not a regression

`continue-on-error: true` by design, pending Jack's review of the first clean report. Note the
trap already recorded elsewhere: a `continue-on-error` step is reported as **succeeded** through
the steps API; only `outcome` holds the truth. Do not read this job's green as meaning anything.

---

## The point

Before today, `ci.yml` had no Swift coverage at all and its red was assumed to be "just fmt."
It was not. One run surfaced a real macOS test failure, a real Windows clippy failure, and a
bug in my own new job. The fmt noise was hiding all three.

Loki

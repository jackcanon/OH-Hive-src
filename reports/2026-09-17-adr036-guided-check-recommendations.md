# Guided acceptance-check recommendations

Tasks now offers Find suggested checks, plain-language explanations with configuration evidence, individual Add buttons and Use recommended checks. Selection is explicit and deduplicated, respects the 16-check limit, and edits only the unsaved draft. Custom program/argument fields remain under Advanced. Separate guidance identifies tests that need creating (including a bug-regression hint) and human review. These are advice, not falsely represented as runnable checks.

Inspection is a read-only operation through the existing GitHub connector token lifecycle. It reads the selected project's root directory at its configured ref/default branch and, if present, package.json by the blob SHA from that listing. Requests use fixed api.github.com routes, conservative owner/repository validation, encoded ref query, bounded response reading and timeouts; package payload has a smaller size cap. No script executes, no package installs, no model calls and no cloud coordinator context transmission. The current submission lifecycle remains intact: checks freeze only when Save task is used. Recommendations reflect inspected configuration and do not pin or certify the later task checkout revision.

Supported recommendations:
- JavaScript: existing nonempty test/build/typecheck/lint scripts, only with an unambiguous supported npm/pnpm/yarn manager evidenced by lockfile or packageManager. Common placeholders, watch and auto-fix scripts are withheld; this heuristic is not a general script analyzer.
- Rust: cargo check/test when Cargo.toml exists; explicitly warns tests may contain zero cases.
- Swift packages: swift build with Package.swift, and swift test when Tests/ also exists.
- Python: setup guidance only until the actual runner/environment can be reliably identified.

Root-only inspection; nested projects/workspaces, platform-specific build selection, dependency/tool installation, task-specific test generation, browser/visual tests and manual review persistence remain future work. Config detection does not prove that a check is runnable, sufficient, read-only, or successful; the UI explains prerequisites and ordinary task execution permissions still apply. No repository script text is interpolated as a shell command: selected JS checks invoke a fixed script name through the detected manager.

Validation: standalone Swift test harness passes for detection, package-manager conflicts, missing manager, placeholder/watcher/autofix suppression, Swift test-directory evidence, Rust/Python behavior, encoded refs, blob-SHA fetching, hostile repository URLs and response bounds. Evidence /private/tmp/hive-suggested-checks-tests.log. Native release build, matching bindings, signing and isolated bundle engine-load probe passed; UI interaction and live connector inspection not claimed.

API source: https://docs.github.com/en/rest/repos/contents (read-only root listing, ref parameter and Contents-read permission). Existing connector refresh/generation checks are retained.

Sif your friendly Codex Agent

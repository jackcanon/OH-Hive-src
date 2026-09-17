# Private task acceptance-check editor

Private Coding projects → Tasks now includes a collapsible acceptance-check editor. Users can add up to 16 named checks, each with a program and arguments entered one per line. Fields are frozen with the submitted task, alongside its repository selection. Empty names/programs prevent submission in the UI and are rejected by the core; existing core bounds also apply.

The new typed PrivateTaskCheck FFI record preserves argument boundaries without shell splitting. Every UI-authored check is required, runs from the task checkout root and expects exit code zero. Existing worker execution, timeout/process cleanup, acceptance evidence and failure gating are reused. Tasks with no checks retain the existing behavior and are visibly marked unverified. Status now reports the saved check count; it does not infer success merely from the presence of checks.

Scope: the editor creates new-task checks only. It does not edit a submitted task, retry a failed task, select custom working directories/expected exit codes/advisory checks, or automatically publish code. Blank argument lines are omitted; empty-string arguments and multiline single arguments are not supported by this first UI. Users can explicitly select a shell program if their check requires shell syntax; ordinary argument fields do not interpret it. These are executable commands running with the user's account permissions, explained in the form.

Validation: 1 FFI regression verifies argument preservation and fixed required/root/zero semantics; 1 core regression verifies stored count, frozen-check idempotency conflicts and rejection without extra rows; 9 acceptance execution regressions cover pass/fail/unverified evidence, actual task gating, timeout/cancellation and process cleanup. All 11 pass. Strict FFI Clippy passes. Native release build, generated matching Swift bindings, signing and isolated bundle engine-load probe passed. Logs: /private/tmp/hive-check-editor-{tests,staging,acceptance,clippy,build}.log.

Interactive native testing remains unverified because prior app inspection timed out. Previous live private model smoke remains valid but had no acceptance checks; do not represent it as a live test of this editor. No live token use, model spend, fleet deployment or GitHub publication in this change.

Next: native manual test, then explicit interrupted/failed-task recovery controls.

Sif your friendly Codex Agent

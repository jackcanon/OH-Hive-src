# Git and GitHub in Hive: audit and implementation recommendation

Date: 2026-09-15. Author: Sif your friendly Codex Agent.
Scope: repository/ADR inspection and current official Git/GitHub documentation. No credentials inspected, GitHub App registered, remote refs modified, or production settings changed. Working checkout was at `b2bc3be` during inspection. Findings describe inspected code, not a live private-repository authentication test.

## Recommendation

Build a first-class Repository Service in Hive. Agents ask it to prepare a task workspace, save a checkpoint, publish a branch and open a review. Hive performs and verifies those operations deterministically. The user should connect GitHub and choose repository policy once, then work in terms of missions, tasks, changes and reviews.

Make GitHub-backed repositories the standard for coding projects, as Jack requested. Existing folders should be importable; disconnected machines should retain local work and queue publication. Connecting GitHub must not imply purchasing Copilot. A GitHub identity is shared product infrastructure; repository authorization, Copilot entitlement and permission to merge remain distinct capabilities.

The most urgent engineering work is workspace preservation and verified publication. Another prompt telling Claude how to push will not repair missing credentials, an inaccessible network, a shared dirty checkout, or an unknown outcome after a timeout.

## Current evidence

| Area | Inspected state | Consequence |
|---|---|---|
| ADR-036 | Proposed, not accepted; describes repo picker, persistent clone/worktrees and PRs | Direction is sound but not an implemented workflow |
| `coder.rs:155`, CodeSessionSpec | Local path, repo URL and optional ref; local path wins | No durable provider/repository/installation identity or task workspace record |
| `coder.rs:487`, prepare_workspace | Existing directory used directly; managed clone removed and re-created on retry | Retry can discard agent edits; two tasks can share a mutable user checkout |
| `coder.rs:540`, run_git | Direct process arguments, bounded output and timeout; relies on machine Git authentication | Good process foundation; no managed account lifecycle or actionable auth recovery |
| `coder.rs:824`, run_command_tool | Runs caller-selected programs with inherited environment | Agents can potentially invoke Git themselves; cwd containment is not OS sandboxing or credential isolation |
| `coder.rs:1718`, CodeSessionOutcome | Text, turn/lease state and child-wait result | Does not itself prove committed SHA, remote branch state, PR or CI result |
| ADR-030/032, CLI submissions | Repository-based coding work can be submitted/coordinated | Useful task entry points, not a repository publication service |
| Native/Tauri/FFI surfaces searched | No end-user repository account/picker/publish/PR lifecycle found in inspected surfaces | A successful coding run is not an integrated GitHub delivery |
| Current development practice | Shared checkout; one agent commits others' work by convention | Avoids some overlap but requires manual coordination and leaves attribution/staging fragile |

The ADR calls the old implementation “raw shell git.” More precisely, preparation invokes Git directly without a shell; arbitrary program execution remains available separately. Do not discard the existing argument-array, bounded-output and timeout safeguards.

## The end-user flow

1. **Connect GitHub.** Browser account selection, permission explanation and repository selection. Show organization approval/SSO requirements as actionable states, not “Git failed.”
2. **Create or import a project.** Pick an existing repository, link a folder, or create a repository with a clear owner/visibility choice. Never publish an existing private folder merely because it was imported. New-repository creation has distinct permissions; do not assume an installation restricted to existing repositories can create and access a new one automatically.
3. **Choose automation once.** Recommended project option: allow Hive to checkpoint locally, push its task branches and create/update draft PRs automatically. It must explicitly cover repository and branch namespace. Initial default for merging: one user approval after the diff and required checks are ready. Direct-to-default-branch publication stays an advanced, explicitly selected policy.
4. **Assign work.** Each independent task gets an isolated branch/worktree from a recorded base SHA. Its agents may collaborate intentionally within that task under a workspace lease. Independent tasks do not share a checkout.
5. **Review changes.** One screen shows what changed, relevant test results and blockers. “Publish for review” handles commit, push verification and draft PR creation when automatic publication is not enabled. Failed tests can be published as clearly marked draft work under policy, never represented as passing.
6. **Approve and merge.** Respect GitHub rules and required checks. Show queued, merged or blocked states accurately. Do not hide failures behind a green “task complete.”

Persistent status vocabulary: **Saved locally → Queued to upload → Uploaded → Review open → Checks running → Ready to merge → Merged**. Auth expired, permission denied, conflict, network unavailable and protected-branch rejection each get a specific recovery action. Users should not paste error logs into a model to find out what happened.

## Authentication architecture

Use a Hive-owned GitHub App. GitHub recommends Apps for fine-grained permissions and repository selection. This fits a project-scoped product better than asking every novice to create a broad personal token. [GitHub App versus OAuth App](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps).

Start with repository Metadata read, Contents read/write and Pull requests read/write. Request Actions/Checks/Commit statuses read only as needed for CI display, and Issues access when issue workflows ship. Do not request administration or ruleset bypass by default. Workflow-file changes need an explicit permission decision and test case. App permissions, rather than OAuth `repo` scopes, govern this connection. [Permissions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app).

**Desktop first, headless next:** the documented GitHub App device flow is a practical desktop/headless implementation without embedding an app secret. Repository listing must use installations/repositories accessible to both app and user; being signed into GitHub alone is insufficient. Default expiring user access tokens last eight hours. [User authorization](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app).

Refresh device-issued user tokens locally: the current refresh documentation expressly exempts device-issued tokens from the client-secret requirement. Refresh tokens rotate; serialize refresh across consumers/processes and atomically save the replacement before further operations. Do not disable expiration to reduce support work. [Refresh requirements](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens).

A browser-based authorization-code flow with PKCE can improve desktop UX. However, GitHub's current best-practice discussion favors PKCE while the App token-exchange table still lists a client secret as required. Do not ship a secretless authorization-code implementation on inference alone: prove the exact registered-App flow, or use a small trusted token-exchange broker. Never ship the App secret/private key inside a public binary. [App best practices](https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app).

For unattended publishing, add a trusted broker that mints installation tokens narrowed to one repository and required permissions. These expire after one hour; they represent the installation, not the signed-in human. Keep the App private key in the broker, never on volunteer machines. This is an optional later service, not a prerequisite for the first desktop release. [Installation token minting](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app).

**Copilot correction to ADR-036:** a GitHub App user token can be a supported Copilot credential, but repository connectivity does not establish a Copilot subscription or organization permission. Share account management where possible, not an assumption that every token is interchangeable. Copilot's user-subscription path and organization installation-token path have different configuration and billing semantics. Probe and expose each capability separately. [Copilot authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate), [server-to-server mode](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens).

Keep existing SSH/Git credential-helper support as an advanced import path. Git Credential Manager is a maintained cross-platform option for existing local Git authentication; it does not replace Hive's repository picker, task policy or PR orchestration. [Git Credential Manager](https://github.com/git-ecosystem/git-credential-manager).

## Local workspace and publication service

Use the installed, supported Git executable behind typed Rust operations initially. It already fits the code and common Git extensions better than immediately replacing Git with a new library. Verify/version it on macOS, Windows and Linux; make installation discovery actionable. Do not require `gh` for normal users: use Git for repository data and GitHub APIs for metadata/reviews.

Keep one managed clone per repository **per machine**, then one linked worktree/unique task branch. Git worktrees share repository data while providing separate working directories; cleanup and locking need to respect their linked metadata. Do not sync `.git` directories through iCloud/Drive or copy linked-worktree directories between computers. [Git worktree documentation](https://git-scm.com/docs/git-worktree).

Proposed core records:

- `RepositoryBinding`: provider host, immutable repository ID, installation/account reference, default branch, project policy; URLs contain no credentials.
- `TaskWorkspace`: task/attempt, node, worktree path, branch, base SHA, last commit, state and workspace ownership lease.
- `PublishOperation`: idempotency key, expected branch tip, intended commit, remote result, PR number/URL, check SHA, approval/policy revision and retry state.

Implement `repo_connect/list`, `workspace_prepare/status`, `checkpoint`, `publish`, `review_status`, `merge` and `recover` as typed operations exposed consistently through Rust, UniFFI, Tauri, CLI and agent tools. Keep Git argument validation, timeout/process cleanup, output caps and redaction centralized. Set noninteractive Git authentication explicitly. Limit protocols and validate refs; avoid user-controlled option injection and token-bearing URLs.

Serialize metadata mutations per managed repository and writes per worktree. An OS lock protects a local checkout; a durable task ownership lease with a fencing generation prevents a stale fleet worker publishing after reassignment. Inference capacity locks are unrelated and insufficient. New attempts reuse verified progress or get a new attempt branch; never wipe an existing workspace on retry. Preserve dirty imported folders and ask the user how to incorporate pre-existing edits before publication.

Publication algorithm:

1. Verify workspace ownership, repository identity and allowed branch; collect tracked and untracked changes without accidentally staging another task's edits.
2. Record the exact candidate SHA and run configured checks against that revision. If anything changes, invalidate stale validation.
3. Commit using configured human/bot attribution, recording agent/task provenance honestly. Keep failed-check checkpoints recoverable locally.
4. Push the explicit task ref with ordinary fast-forward semantics. On rejection, fetch and reconcile in the isolated task workspace; no automatic force-push or destructive reset.
5. Read remote ref state. An ambiguous timeout means reconcile first, not “failed” or “success” by guess. On an exclusive task branch, require the intended tip; if others advanced it, classify and verify ancestry before deciding.
6. Find/create/update the task's PR idempotently using repository plus head/base identity; recover after a crash between push and PR creation. Persist the PR receipt.
7. Associate checks and approval with the current head SHA. Merge using expected-head protection, GitHub rules and permitted merge method; verify the merged result. If the base moves, use required checks/merge queue where available instead of treating yesterday's test run as current.

GitHub rules can require checks and constrain their trusted source. Hive should honor them and explain restrictions, not request a bypass token. Availability of rules/merge-queue features varies by repository/account; detect capabilities. [Rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets).

## Fleet and credential boundaries

Default to a designated trusted publisher for each project. Agents on trusted owned machines receive workspace/task access; community workers receive only explicitly shared project material, never personal GitHub credentials. Return a commit bundle or bounded patch/artifact with base SHA and provenance to the publisher. Verify it in an integration worktree before tests and publication. Do not give every agent a long-lived write token simply to avoid prompts.

GitHub repository permissions are not branch-scoped task permissions. Hive's branch policy must be enforced by the publisher, and GitHub rules provide additional server-side protection. A bearer token on a worker can exceed the intended task branch.

The current general command runner inherits the host environment and can launch arbitrary programs. Hiding tokens in a helper is not sufficient if generated code runs with the same OS user's access to credentials. For untrusted workloads, isolate execution from the publisher with real OS/container permissions, fresh credentials-free environment and restricted filesystem/network access. Git hooks, filters, submodules, dependency installers and test scripts are executable input. Configure trusted checkout behavior; do not blanket-enable dubious repository ownership or execute hooks while handling credentials. Avoid claiming sandbox isolation based solely on cwd validation.

## Delivery order and acceptance

**G0 — Preserve work and own the workflow.** Replace wipe/reclone with durable task workspaces; add ownership, journal and local checkpoint receipts. Keep existing auth usable. Prove retry/crash recovery, dirty-import handling and two concurrent agents on the same repository. This can start before App registration.

**G1 — Ship the useful vertical slice.** App account manager, repo picker and project binding; credential isolation; typed publish; automatic task-branch/draft-PR policy; one review screen. Acceptance: a novice connects a private repository, assigns a task and gets a verified PR without terminal commands. Test account expiry/rotation, wrong account, org approval, denied access, disconnect, offline retry and ambiguous push outcomes on all desktop platforms.

**G2 — Fleet integration.** Trusted publisher, task fencing, transfer verification and integration queue. Acceptance: Heimdall and Overgaard work from the same recorded base in separate workspaces; one integrates both, resolves or clearly reports conflicts, runs checks and creates a verified PR. A stale worker cannot publish over its replacement. Never claim a physical fleet pass from an HTTP fixture.

**G3 — Advanced repository operations.** Create/fork repositories, issues-to-tasks, Git LFS/submodules, signing requirements, multiple repositories per mission and enterprise hosts. Show clear unsupported states until validated. Large model files/build artifacts should use appropriate artifact storage rather than being silently committed; secrets must stay outside repository content.

Useful product metrics: first successful private-repo PR completion rate, time to first PR, manual prompts per successful task, publication retry recovery, lost-work incidents, duplicate PRs and failures categorized by auth/network/conflict/policy. Do not optimize merely for number of commits.

## Requested ADR changes and immediate practice

Retain ADR-036 as Proposed until Jack accepts the revised design. Add durable publication/verification and fleet ownership; change “one underlying token” to “shared identity with separately verified capabilities”; document user-token refresh and the optional installation broker. Separate enabling automatic task-branch publication from permission to merge. Add create/import/organization permission states and the no-work-loss migration guarantee.

For Hive development today, use task-specific worktrees and a single integration/publishing owner per repository. Builders should hand off a commit SHA, changed-file summary and test results; the publisher integrates those commits and verifies remote refs. This preserves the current one-committer convention while removing the shared-dirty-checkout bottleneck. Diagnose future push failures by category with redacted evidence; this audit cannot establish the cause of every past Claude push failure without those failures' logs.

Claude: please review this audit and revise ADR-036 before implementing G1. Start with G0 and a minimal end-to-end G1 slice; do not block repository usability on Copilot or the complete Bots UI.

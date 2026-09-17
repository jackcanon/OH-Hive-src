# ADR-036: authenticated private checkout preparation and activation

Added `LocalHubStore::prepare_private_code_task` to connect the staged-request backend to authenticated Git preparation and ready-queue activation. This is trusted host administration, not a paired-worker HTTP endpoint or an exposed desktop Run action.

## Operation

1. Load the staged card and its private submission receipt; require the executing node to match the assigned node and still have an unrevoked enrollment. Only awaiting-preparation or already-prepared ready cards qualify. Running/completed/other-blocked work cannot be reset through this API.
2. Resolve the coding specification from the card's frozen repository/ref. The current project default is irrelevant.
3. For a new task, perform a fresh GitHub clone with the one-operation token using the isolated Git environment from the preflight. Clone uses `--no-checkout --template=`. Credentials never enter stored Git config, card data, command arguments or model environment. Only the fresh clone subprocess receives them.
4. After that subprocess exits, resolve the base commit, create the task's `hive/<card UUID>` branch and checkout, and atomically write the existing ownership receipt including original base commit. These later commands receive no connector token.
5. Recheck card payload/status and enrollment, then record the canonical managed workspace path and move the card to ready. Release the preparation lock before publication so a fast worker does not fail on the host's lock; the worker reacquires it and validates the receipt itself.

The ready card no longer requires internet for repository setup. Its prepared workspace path prevents fallback cloning if the checkout/receipt disappears or execution uses a different data directory. The target-node filter still applies. A failed or interrupted clone preserves files, leaves the card blocked, and requires recovery if a partial checkout has no receipt. A completed receipt after a crash before activation can be reused offline, without a token. Ready-state retries preserve dirty work. Standalone reuse now additionally checks origin and rejects a symlinked Git directory.

## Tradeoff and limits

Authenticated tasks currently use dedicated standalone clones. Ordinary unauthenticated tasks retain the shared repository cache/worktree implementation. This avoids injecting credentials into an existing cache whose local Git configuration could have been changed by a coding task. A future authenticated cache transport needs equivalent configuration isolation; do not simply pass the token into an arbitrary existing checkout.

An empty Git repository has no base commit to check out; first-commit bootstrap remains unsupported. No private repository was cloned live during development. The new clone path is compiled and uses the existing credential configuration; actual GitHub token/contents permissions still need a live check.

The method's executing-node parameter is for trusted host code; the eventual FFI caller must derive it from verified Private Fleet identity, never UI input. No token registry, persistent credential grant, cloud service, cross-machine delegation, publication/push/PR, or model run was added.

## Verification

Two new integration regressions use real local Git to simulate completed preparation before a crash, then verify offline activation, dirty-file preservation, idempotent ready retry, wrong-target rejection, missing-receipt/wrong-data-directory rejection without fallback, assigned-node-only offline claim, and refusal to reprepare running work. Failure tests show invalid credentials leave the job blocked and partial checkouts survive retries. Existing shared-cache/worktree regressions exercise the extracted reference resolver.

Final full-workspace/lint results are in continuity; logs /private/tmp/hive-private-prepare-{tests,full,clippy}.log.

Next: primary-local FFI/UI submission/status/recovery, and a private LocalHub worker/session route. The native supervised coding worker still targets the community HubClient. No app rebuild or fleet deployment in this backend slice.

Sif your friendly Codex Agent

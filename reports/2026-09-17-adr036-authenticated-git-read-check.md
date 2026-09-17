# ADR-036: authenticated Git read preflight

The project repository editor now offers **Check saved repository access** when GitHub is connected and the project has a saved URL. This performs a real, read-only Git request on this Mac, using the connector's existing Keychain token and refresh flow. It checks HEAD accessibility, including an empty repository, without cloning or changing the repository.

## Why this is a preflight, not a fleet credential grant

The new project controls do not yet submit coding jobs. An automatic global token registry keyed only by URL would make the token available to unrelated jobs (including community work) that name that URL. This implementation deliberately adds no such registry. The next integration must bind credential use to a trusted, owner-approved private task and its immutable repository identity.

## Implemented path

- Swift connector obtains/refreshes the access token, checks its connection generation before/after the operation, and never publishes it as observable state.
- Typed UniFFI accepts a project ID and in-memory token. It requires local verified Private Fleet identity, rejects a selected remote primary, and reads the URL from the stored binding. Caller-supplied URLs are not accepted by this FFI operation.
- Primary-selection lock is held throughout the owned runtime operation. Blocking SQLite lookup stays off the UI thread.
- Core Git probe accepts only credential-free `https://github.com/owner/repository` URLs with conservative path validation. It invokes Git directly with fixed `ls-remote -- URL HEAD` arguments.
- The authorization header is supplied in child-only Git runtime configuration, scoped to that URL. It is absent from arguments, repository settings, task data and temporary files.
- A fresh temporary working directory and Git discovery ceiling avoid project-local config. System/global config and inherited tracing/proxy/config variables are excluded. Prompts, credential helpers and redirects are disabled, HTTPS certificate verification remains enabled, other protocols are denied.
- Existing bounded output, timeout and process-tree cleanup are reused. Failures are reduced to fixed messages; success output must be empty or a valid HEAD object ID. Remote output is never returned to the UI.
- An already-started check can finish after disconnect, but its UI success is discarded by the connector generation check. No credential is installed for subsequent operations.

This protects the application data flow; environment-held credentials are not a security boundary against privileged or same-user processes. No claim of OS isolation or reduced GitHub-side token scope is made. The token retains the permissions granted to the GitHub App/user. A successful public-repo check does not prove account identity or private access.

## Verification and limits

47 coding/workspace tests pass, including three new regressions for URL/token validation, configuration/argument shape, output parsing and subprocess error redaction. Existing real-Git worktree, timeout and process cleanup regressions still pass. Strict FFI Clippy passed. Build outcome is recorded in continuity; logs `/private/tmp/hive-git-access-{tests,clippy,build}.log`.

No live OAuth token was read by the development agent and no live authenticated repository request was made. The user can run the new check in the rebuilt app. Worker clone/fetch credentials, persistent grants, remote-machine credential delegation, job submission, write/push/PR permissions and publication are not enabled by this check. Existing worker Git behavior is unchanged.

## References

- Git runtime configuration and URL-scoped HTTP options: https://git-scm.com/docs/git-config
- Git environment controls: https://git-scm.com/docs/git
- GitHub authentication/token types: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-authentication-to-github

Sif your friendly Codex Agent

# Coder command failure feedback

The automatic correction control exhausted its original eight turns after duplicated executable arguments, absolute working directories and a combined executable/argument string. Existing system guidance was already explicit; the failure response lacked actionable recovery guidance.

This change adds structured feedback at command dispatch: malformed arguments, rejected working directories and process spawn failures state that nothing executed, preserve the original error and explain how to correct the next call. Tool descriptions include a concrete Python invocation and tell agents to omit the working directory at the checkout root.

The host does not split commands, rewrite arguments, retry failures, expand access or extend turn budgets. Actual executable paths containing spaces remain valid. Acceptance-command execution remains unchanged. Missing programs and missing directories remain real failures; the hint distinguishes both possibilities rather than claiming a program is absent.

Verification: the already-compiled workspace core suite passed 424 tests, with one ignored, in 14.85 seconds. New invalid-directory/missing-executable feedback and real executable paths containing spaces passed. The command-line and coordinator suites passed. Local foreign-interface tests include five pre-existing failures caused by reading this Mac's saved remote-primary selection, also reproduced serially; clean hosted checks are pending. The initial restricted workspace invocation failed on local test networking and was rerun with that access allowed. A passing small live task will not establish that the previous failed correction now succeeds or that larger project autonomy is reliable.

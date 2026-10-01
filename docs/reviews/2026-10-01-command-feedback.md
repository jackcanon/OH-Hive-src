# Coder command failure feedback

The automatic correction control exhausted its original eight turns after duplicated executable arguments, absolute working directories and a combined executable/argument string. Existing system guidance was already explicit; the failure response lacked actionable recovery guidance.

This change adds structured feedback at command dispatch: malformed arguments, rejected working directories and process spawn failures state that nothing executed, preserve the original error and explain how to correct the next call. Tool descriptions include a concrete Python invocation and tell agents to omit the working directory at the checkout root.

The host does not split commands, rewrite arguments, retry failures, expand access or extend turn budgets. Actual executable paths containing spaces remain valid. Acceptance-command execution remains unchanged. Missing programs and missing directories remain real failures; the hint distinguishes both possibilities rather than claiming a program is absent.

Verification: pending. A passing small live task will not establish that the previous failed correction now succeeds or that larger project autonomy is reliable.

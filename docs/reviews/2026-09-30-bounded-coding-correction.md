# Bounded correction after coding acceptance failure

Owner-declared coding sessions may set `max_acceptance_repairs` to 1–3. The default is zero, preserving existing one-pass jobs and desktop submissions. This is a host-owned setting; the agent cannot grant itself retries or alter the declared checks. The private task staging transport carries and freezes the selection in the submission receipt. Zero is omitted from the request representation for legacy compatibility.

When a nonempty final response arrives, the host executes declared checks. If required checks fail with ordinary exits, and no check timed out or reported an execution error, the host may return the receipt to the model for another correction. This continues in the same checkout, lease, capacity slot and maximum-turn budget. All check results remain in activity events; each correction is separately recorded. The latest failure is retained if correction runs out of turns. Passing checks and advisory-only failures do not trigger repair. Missing executables, timeouts and expired leases do not trigger repair.

Check output is labelled untrusted evidence rather than instructions. Existing tool policies and process cancellation remain in force. This change does not add checkpoints, renew leases, replay terminal tasks or grant extra tools. Host checks can have side effects, which is why replay is explicit rather than globally enabled.

The current native desktop form continues to submit zero repairs. The new selection is available on owner-bound private task staging for controlled testing; a guided user setting remains future interface work. No desktop binding signature changed.

Verification includes real subprocess checks and an agent-tool file repair, execution counts across success/failure and exhausted correction limits, turn exhaustion retaining the failed gate, expired lease stopping before model invocation, missing-program errors, staging limit rejection and omitted-field compatibility. The model in these regression tests is scripted; real Helheim model acceptance remains a separate gate. Neither this loop nor the earlier empty-reply guard proves autonomous teamwork.

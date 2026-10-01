# Bounded correction after independent code review

An owner can select Request correction when a completed independent checker returns changes_required. The hub creates a separate coding card on the original execution computer using the original agent, model, reasoning policy, turn limit, saved persona and required checks. It preserves both earlier outputs and checkouts. A stable request identifier makes repeated submissions return the same card.

The correction checkout starts at the original protected baseline commit. Saved before/after files and exact checker findings are supplied as bounded untrusted context; they are not copied directly into the checkout. The coder must recreate the intended changes and fix the findings. This avoids copying unchecked commands, symlinks or files from a mutable checkout. Each correction chain permits at most three rounds. Preparation and running remain explicit, followed by another explicit independent review.

The owner, source/checker identities, source package digest, baseline, exact findings, completed status and active agent registrations are checked before staging. Immutable source evidence is revalidated before preparation and claim. Changes to source output or verdict require a fresh request. The current profile establishes eligibility; it does not replace saved instructions. Existing worker readiness, lease, cancellation, capacity and host acceptance controls still apply.

A passing checker or inconclusive checker cannot start correction. The checker still evaluates frozen code and prior host test receipts; it does not independently rerun tests. This change does not enable automatic dispatch, merges, publication or project completion. A user can make separate explicit correction branches; the three-round bound applies to each chain.

Validation: core regression suite, authenticated transport, tampered review/source rejection, archived checker rejection, frozen persona/settings, request replay and fresh baseline checkout isolation. A real model correction and subsequent review on Helheim are the installation acceptance gate; their results are recorded separately.

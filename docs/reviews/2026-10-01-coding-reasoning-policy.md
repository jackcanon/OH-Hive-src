# Owner-selected coding reasoning — 2026-10-01

Hel Coder passed three identical fresh asset-classifier fixtures with reasoning disabled and the existing 4096-token cap. Model-default reasoning at 8192 still returned an empty final message. Those calibration runs used a temporary proxy; this change carries the choice through the production task path.

Private staging accepts optional boolean `coding_think`, freezes it in the existing submission receipt, and applies it to local tool-calling requests. Omitted fields preserve legacy receipts and model-default behavior. False sends `think:false` plus `reasoning_effort:"none"`; true sends `think:true` without inventing a reasoning effort level. An invalid raw capability value fails rather than silently selecting another policy. Existing native methods retain their signatures and behavior; the additional method carries the choice from the desktop.

Coding tasks expose Model default / Reasoning off under Checks and options. The control starts at Model default; calibration guidance is explicitly limited to the tested model and small fixture. This does not change the response cap, tool grants, lease, turn limit, correction selection, empty-final guard or truncated-response rejection.

Validation: 356 core tests pass, including real local server body capture for omitted/off/on policies and length rejection, frozen policy conflict detection, malformed serialized value rejection and legacy omission. Native bridge compile and production lint pass. Signed app compilation and direct Helheim acceptance are recorded in the continuity log when complete. No model inference on Midgaard. Larger-project reliability, checker handoff and graphical acceptance are separate follow-ups.

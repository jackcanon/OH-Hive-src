# Hive community openness — Jack's product decision

Authoritative user clarification, 2026-09-15. Supersedes owner-only confidentiality assumptions for community Hive projects in the earlier Sif transcription design. Implementation is not claimed by this document.

## Membership is the boundary; community work is inspectable

A Hive is invitation-only. Inside that Hive, every member can inspect every community project and its jobs, inputs, artifacts, outputs and execution history. There is zero expectation of privacy from other Hive members, by default. Access to shared compute carries this disclosure. Transcription audio and transcripts follow the same rule; they are not special private attachments. Any eligible member-operated worker may execute them. Do not add per-owner or trusted-operator-only read restrictions to community projects.

Selecting Hive execution must plainly explain: “Shared with everyone in this Hive, including your input files and results.” This is a disclosure of the product's default, not an invitation into a Hive, and does not itself create membership. This decision does not change private-fleet/local project confidentiality or publish those projects into community snapshots. It also does not authorize exposing account API keys, private-fleet secrets or unrelated files.

## Attribution and removal

Every job attempt must leave an authenticated execution receipt: Hive/project/job and attempt identifiers, requesting member, executing member and node, claimed/start/finish timestamps, outcome, input/output references and usage/charges. Derive actor identity from verified membership/node credentials, not a client-supplied display name. Preserve failed, cancelled and retried attempts and historical attribution after account removal. This is the required execution signature; whether to additionally use cryptographic signatures is a separate implementation choice, not a claimed existing feature.

Authorized community administrators must be able to remove a member. Removal must prevent new community requests/claims and invalidate their account/node authorization for future access; outstanding leases and in-flight completion must follow the same revocation policy. Preserve audit evidence. Already downloaded shared material cannot be recalled by removing membership.

Openness means members can inspect work, not impersonate another member, rewrite attribution, spend another member's balance, grant themselves administrative authority, or retain authorization after removal. Identity, job/ledger integrity and revocation support the user-requested accountability model; they are not confidentiality between members.

## Effect on T-5 transcription

Drop the proposed owner-only audio/output access and worker-only artifact-read tickets as privacy requirements. Use community-wide visibility consistently. Artifact transport still needs to preserve truthful attribution and invitation/removal boundaries. The existing unauthenticated public-by-hash artifact route is not equivalent to membership-gated openness; this is an outsider/revocation concern, not an objection to fellow members seeing audio. Do not turn a general artifact-transport redesign into an unexplained privacy prerequisite for speech execution.

Remaining functional work: a versioned speech-card payload, project/card submission with retries, staged audio transfer, a one-pass speech worker, completion/usage rules and attribution receipts, progress/results/cancellation, then FFI/UI integration and end-to-end verification. Build to this openness policy, keeping private-fleet work separate.

Sif your friendly Codex Agent

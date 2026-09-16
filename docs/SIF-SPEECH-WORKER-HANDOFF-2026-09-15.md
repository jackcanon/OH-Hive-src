# T-5 speech execution foundation

Implemented `crates/ohhive-core/src/speech.rs`, exported by the whisper feature, plus a speech branch in Worker::run_card and HubClient::artifact_fetch_bounded. Not deployed or advertised to the fleet; no submit UI/RPC yet.

SpeechInput v1 contains SHA-256 artifact hash, byte length, MIME and optional language. Unknown fields, paths/URLs in the hash field, invalid language, unsupported declared audio type and >64 MiB inputs are rejected. This validates the declared type, not a full codec decode; Whisper remains the media decoder. Download is capped at 64 MiB before buffering; actual length/hash are verified before inference.

Each call stages bytes under a random worker-created directory, private permissions on Unix, and a fixed extension derived from the allowed MIME list. No remote filename/path is used. A drop guard cleans the input on success, failure, timeout, cancellation or dropped future (not a process crash; stale-directory recovery remains a follow-up). One inference call produces a transcript. Streamed output is limited to 2 MiB/65,536 chunks, must have a terminal chunk, and must not be marked truncated. Adapter failure details are not copied into community card output.

The worker dispatches speech before the text draft/review loop, loads its locally configured Whisper endpoint/model, retrieves community audio and completes through the existing authenticated hub RPC. No owner-only privacy rule or trusted-operator allowlist is added. Private/local hub jobs cannot silently invoke the community artifact service. Builds without whisper fail speech cards explicitly rather than routing audio through a text model.

The full operation is bounded by the smaller of the remaining lease and 10 minutes. Stop/deadline release the card; completion checks stop/expiry again. Existing hub lease checks still govern publication. This does not add the separately outstanding same-node lease-generation fence. Existing card_outputs record authenticated node, content/model and usage; a comprehensive all-attempt immutable member/node receipt and membership-removal audit remain separate required integration work.

Pricing update: Jack selected adding Honey pricing before launch. Local SQL migrations now freeze an explicitly approved processing-time rate and cap, and settle speech through both completion paths. See `SIF-SPEECH-HONEY-PRICING-2026-09-15.md` for API wiring, tests and launch limits. No numeric rate is configured and the migrations are not deployed; do not enable paid submission yet. Worker capabilities remain intentionally unadvertised.

Verification: `cargo test -p hive-core --features hub,whisper --lib --offline` passed all 49 tests, including 5 new speech tests. Tests cover manifest/path rejection, length/hash mismatch, success and usage, oversized/truncated/unterminated output, cancellation, deadline, dropped-future cleanup and a real loopback HTTP request through WhisperCppBackend to a mock server. `cargo check -p hive-core --features hub --offline` passes without speech enabled. These are not real audio decoding or a full live hub claim/completion test. No release FFI library or app rebuild was needed/claimed for this unenabled core slice; existing local app still uses its previous library.

Next: project/card submission with retry receipts; audio upload association and community disclosure; speech capability advertisement/model match; chosen billing policy and execution-attempt attribution; FFI/UI progress/result integration; membership removal and real invited-community acceptance. Jack's community-wide inspection rule remains authoritative.

Sif your friendly Codex Agent

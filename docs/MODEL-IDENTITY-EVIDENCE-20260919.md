# Model identity evidence — September 19

Issue 076c99d2 asks whether incorrect agent self-identification comes from model confabulation or a difference between the requested model and the server handling that turn. Do not treat a matching configuration file or a model's self-description as proof of actual routing.

## Observations

- Installed-source trace: FFI `model_pref()` reads HIVE_MODEL; `drain_once` passes that value to LocalModelTurnRunner. The runner uses the same string in its identity record, requirements.model_id, and Library-tool model argument. This establishes source consistency, not the environment captured by a historical running process.
- Both streaming SseChunk and non-streamed ToolChatResponse previously discarded the response's model field. Existing local Bots log had no requested/reported model receipts. Historical turns cannot be classified from that evidence.
- Read-only Niflheim configuration: qwen3.6:27b at the default local Ollama endpoint. No loaded models before the probe.
- One direct synthetic endpoint probe on Niflheim completed at **2026-09-19 16:07:32.842501 UTC**, in 30.46 seconds. Requested **qwen3.6:27b**; every reported response model was **qwen3.6:27b**, completion ID **chatcmpl-549**. Generated answer repeated Tyr, Niflheim and qwen3.6:27b accurately.
- The probe used a minimal identity prompt and the adapter's request shape, with 96 output tokens and thinking disabled. It was not a saved Den conversation, did not use real history or Library tools, and does not establish why earlier Den turns were wrong. No inference ran on Midgaard or Alfheim. No agent settings/bios were changed.

## Diagnostic change

The local adapter now logs metadata-only request/completion correlation, requested_model, reported_model, and literal model_name_matches. Streaming responses log their first reported name and changes, not every token; omitted model metadata remains unavailable. Tool-enabled responses also log the reported name. Prompts, responses, tool arguments, endpoint URLs and credentials are excluded. This does not change model selection, prompt behavior, or acceptance of aliased model names. Server-reported names are evidence, not cryptographic proof of loaded weights.

Next: package/install the diagnostic-enabled build and observe a real Den turn with its request/completion identifiers. Compare the requested and reported names before choosing a routing or prompt fix. New logging is not yet in the installed apps. Remaining agents have not been live-probed in this pass.

## Native app verification after host correction

The first app-only attempt stayed pending: Tyr was assigned to legacy CLI host `799cfdea-29f4-4ee5-8b6e-aeb2adb4a540` (Niflheim), while the authenticated native app registration was `d2cd4b0f-e9b1-45e7-a4d9-b0a9339cfb8e` (Jack’s Mac Studio). The legacy CLI eventually answered that first test after its temporary pause ended; that response cannot validate native routing.

On September 19, corrected only Tyr's preferred_host through the authenticated primary API. Verified enrollment identity before the patch, saved the original profile on Niflheim, and compared all other profile fields unchanged (host_name is derived; updated_at changes normally). No biography or tool-grant updates were sent.

A new message in conversation `82a4c200-6f3e-4bff-83de-5319c8468db1`, sequence 7, was claimed and completed by the native app. Sequence 8 replied: “I am Tyr, hosted on Jack's Mac Studio, running the qwen3.6:27b model.”

Native app metadata at **2026-09-19T18:34:35.993837Z**: request `14e685ce-af48-4da5-bcf7-a3fd9e718a86`, completion `chatcmpl-730`, requested_model **qwen3.6:27b**, reported_model **qwen3.6:27b**, model_name_matches **true**. No inference request ran on Midgaard. The CLI worker no longer owns Tyr's assignment and was not paused again for this corrected test.

This verifies Tyr's current native-app delivery and model-name agreement. It does not retroactively classify historical CLI replies or prove the other agents' model identity. The current app registration's human name is still Jack’s Mac Studio; no machine rename was performed.

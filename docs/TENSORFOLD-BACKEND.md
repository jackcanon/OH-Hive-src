# Optional TensorFold backend (native Loki’s Den)

The native Mac app can select an authenticated loopback TensorFold server independently of its existing Ollama/llama-server URL. Settings → backend shows the engine choice. Stop the coding worker before selecting; the app drains its prior session and validates model discovery before atomically replacing the profile. Selecting Ollama/llama-server restores the existing URL. A failed selection retains the previous profile; a subsequently unavailable selected engine fails closed, without silently switching providers.

The profile lives alongside node.env as text-backend.json. It stores kind (legacy or tensorfold), origin (an HTTP(S) IP-loopback origin without paths/query/userinfo), and an absolute credential_file path. The credential file must be a regular same-user private file; Unix symlinks and non-private modes are rejected. Its contents never cross FFI/MCP. No proxy or redirect is used with the credential. Profile persistence is private and atomic; keys are not copied into node.env or model messages.

Run TensorFold separately with an API key file and a compatible checkpoint. This app does not download models, launch or update TensorFold. Engine/model licenses and hardware compatibility remain distinct. Missing tool/thinking metadata stays unknown; it is not a compatibility guarantee. The native node, private coding readiness/execution, model inspection and Bots loopback runner share the selected adapter. CLI/Tauri setup/install and model download remain their existing flows; changing the profile does not migrate model stores or replace Ollama.

Owner MCP tools den_backend_profile and den_backend_select expose the same app-owned profile operation. They require the existing opted-in owner socket. Selection requires a stopped, drained native private coding worker; inference/model discovery then uses the existing authenticated fleet permissions. This adds no public listener or distributed-user credential mechanism. Arbitrary third-party job isolation and Happy Jack human SSO/member authorization remain separate distribution requirements.

Qualify a model using real coding host checks, an independently owned review, and cancellation/disabled-worker/process/hub recovery through MCP. A model responding to a synthetic tool call is insufficient. Compare speed only for matched checkpoints/quantization, prompts, context, output budgets, concurrency and warm/cold conditions; decode speed alone is not end-to-end task speed.

## Helheim pilot default and exclusive workflow admission

A selected profile may store `default_model`, `thinking_budget` (1–4096),
`default_coding_think` and `default_checker_think`. These are native per-engine
preferences; requests explicitly selecting another model or policy remain explicit.
MCP task/review submission resolves omitted values before freezing the task. The
thinking budget is sent per TensorFold request; it does not change another
client's server default or allocate extra output tokens.

The Helheim machine policy is a separate private `inference-workflow-policy.json`
next to node.env: `{ "schema_version": 1, "owner_file": "/absolute/private/owner.json" }`.
It overlays every backend selection, including legacy Ollama, so changing the
engine cannot remove admission checks. Existing installations without this policy
retain their existing behavior. The owner file is maintained by the separately
provisioned Helheim inference controller; it is not a public fleet authorization
service.

Only `owner="den"` under schema version 1 permits Den agent execution. A worker
captures that ownership generation and rechecks it at every tick/inference.
Metadata remains readable without inference ownership. Missing, invalid,
insecure, transitioned or stale state rejects work. Native Bots and coding share
a whole-workflow gate; the gated pilot does not start a community compute worker.

`worker.drain` / `den_worker_drain` first writes a private persistent sibling
`den-drained.json` fence for the current generation, then stops and joins private
worker cleanup and waits for the whole-native-agent gate. Its response includes
`owner`, `generation`, `drained`, `active_workflows`, `workerEnabled` and
`new_admissions_closed`. The same generation cannot reopen admissions, including
after an app restart. A later ownership generation is required. The owner
controller must independently call the same-UID app socket, match the expected
generation, and verify zero hub leases and native inference before restoring
Hermione. It must not trust a caller-provided boolean or use a TTL to steal work.

The controller/keyring and Hermione ingress/drain path require their own live
qualification. Profile metadata alone does not prove strict one-agent admission.
